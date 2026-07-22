# xhjob ThinkPHP 8 扩展集成

> 基于 Rust（ext-php-rs 0.15）内核的 PHP 扩展，为 ThinkPHP 8 提供**单机异步任务调度**能力，零外部依赖（不依赖 Redis / RabbitMQ / Swoole / Supervisor），支持**两种任务执行池模式**（async task 池 / 1:1 线程池）按场景切换。

本集成包以 `extend/` 第三方库形态接入 ThinkPHP 8，封装 24 个 PHP 扩展函数与 38+ 个链式构建方法，覆盖触发器、重试、超时、并发控制、任务编排（chain / group / chord）、持久化恢复、可观测性等完整能力。

---

## 目录

- [核心特性](#核心特性)
- [ThinkPHP 8 引入](#thinkphp-8-引入)
- [两种任务执行池模式](#两种任务执行池模式)
- [使用说明](#使用说明)
- [配置项](#配置项)
- [环境变量](#环境变量)
- [测试套件](#测试套件)

---

## 核心特性

- **两种任务执行池模式**（重点能力）
  - async task 池（`async`，默认；兼容别名 `coroutine`）：基于 tokio M:N 调度，IO 密集型场景高并发、低资源占用，最大并发 1024。
  - 1:1 线程池（`thread`）：基于 `std::thread` + `crossbeam-channel`，每个任务在独立工作线程中 `block_on` 执行，适合 CPU 密集型或需严格并发控制的场景。
- **零外部依赖**：不引入 Redis / MQ / Swoole / Supervisor，Rust daemon 进程内调度，部署形态仅多一个 `.so` 扩展。
- **全触发器谱**：CronTrigger（5/6 段，含秒级）/ IntervalTrigger / DateTrigger（run_at / countdown）/ start_date / end_date。
- **完整调度控制**：retry + retry_backoff（指数退避）/ timeout + soft_timeout（SIGTERM→SIGKILL 升级链）/ maxInstances + allowOverlap + coalesce / jitter / expires / misfire_grace_time / rate_limit / priority / max_executions。
- **任务编排三件套**：chain（顺序流水线）/ group（并行批处理）/ chord（group + 回调，对齐 Celery chord）。
- **可靠性与恢复**：persist（SqliteStore + WAL）/ acks_late（crash recovery）/ ignore_result / result_ttl / 10 类事件。
- **可观测性**：inspect（active / registered / scheduled / stats）/ pull_events / report_progress。

---

## ThinkPHP 8 引入

### 前置条件

- PHP ≥ 8.0（推荐 8.2+）
- ThinkPHP 8.x（`topthink/framework ^8.0`）
- 已编译的 xhjob 扩展（`xhjob-php8.2-linux-x86_64.so`）

### 步骤 1：安装 PHP 扩展

将编译产物 `xhjob.so` 放入 PHP 扩展目录，并在 `php.ini` 启用：

```ini
extension=xhjob
```

验证扩展是否加载：

```bash
php -m | grep xhjob
# 输出：xhjob
```

### 步骤 2：复制集成包到 ThinkPHP 项目

假设 ThinkPHP 8 项目根目录为 `tp/`：

```bash
# 1. 复制 extend 第三方库（PSR-0 自动加载）
cp -r Xhjob/  tp/extend/Xhjob/

# 2. 复制配置文件
cp config/xhjob.php  tp/config/xhjob.php

# 3. 复制控制器（可选，提供 HTTP 路由管理 daemon）
cp controller/XhjobTask.php  tp/app/controller/XhjobTask.php

# 4. 合并路由到 tp/route/app.php
cat route/app.php >> tp/route/app.php

# 5. 注册服务提供者（在 tp/app/service.php 追加）
echo '\Xhjob\ServiceProvider::class,' >> tp/app/service.php
```

### 步骤 3：确认 composer 自动加载

ThinkPHP 8 的 `composer.json` 默认配置了 `psr-0` 自动加载 `extend/` 目录：

```json
{
    "autoload": {
        "psr-4": { "app\\": "app" },
        "psr-0": { "": "extend/" }
    }
}
```

如果 `extend/` 目录是新增的，需执行：

```bash
composer dump-autoload
```

### 步骤 4：配置环境变量（可选）

在 `.env` 文件或系统环境中配置：

```bash
export XHJOB_POOL_MODE=async   # 或 thread（coroutine 为 async 兼容别名）
export XHJOB_PERSIST=1         # 启用 SQLite 持久化
export XHJOB_DATA_DIR=/var/lib/xhjob
```

### 步骤 5：验证安装

启动 ThinkPHP 内建服务器并访问路由验证：

```bash
php think run -H 0.0.0.0 -p 8000

# 另一个终端验证
curl http://localhost:8000/xhjob/index
curl -X POST http://localhost:8000/xhjob/start
curl http://localhost:8000/xhjob/status
```

返回 JSON 含 `running: true` 即表示集成成功。

---

## 两种任务执行池模式

xhjob 内核提供两种**可切换的任务执行池模式**，针对不同负载特征做出取舍。模式选择在 daemon 启动时确定，运行期间不可热切换；如需切换需 `xhjob_stop` 后以新的 `XHJOB_POOL_MODE` 重新 `xhjob_start`。

> **术语澄清**：Rust 没有语言级"协程"——只有 `async/await` + `Future`（编译期转为状态机）由 runtime（tokio）poll 驱动。这与 Go goroutine / Python coroutine 的"协程"概念不同。`coroutine` 作为兼容别名保留，等价于 `async`，建议新代码使用 `async`。

### 模式一：async task 池（async，默认）

- **底层实现**：基于 `tokio` multi-thread async runtime，任务以 `Future` 形式 `tokio::spawn` 到共享调度器，由 tokio worker 线程复用执行（线程名 `xhjob-tokio`）。
- **并发模型**：M:N 调度——N 个 tokio worker 线程（N = CPU 核数）复用跑 M 个 async task，每个 task 在 `.await` 让出点主动切换，不阻塞工作线程。
- **资源占用**：单 task 状态机按需分配（典型几 KB），内存占用低；worker 线程数固定，不随任务数线性增长。
- **最大并发**：默认 `1024`，可通过 `XHJOB_ASYNC_POOL_SIZE` 覆盖（兼容别名 `XHJOB_COROUTINE_POOL_SIZE`；向上调优需关注文件描述符上限）。
- **适用场景**：
  - IO 密集型任务：HTTP 请求、shell 命令等待、数据库 / 缓存往返。
  - 高并发短任务：定时轮询、心跳上报、批量 webhook 回调。
  - 资源敏感环境：容器、低配 VPS、需与 PHP-FPM 共享内存的部署。
- **启用方式**：`XHJOB_POOL_MODE=async`（或不设置，默认值；`coroutine` 为兼容别名）。

### 模式二：1:1 线程池（thread）

- **底层实现**：基于 `std::thread` + `crossbeam-channel`，启动时创建固定数量的工作线程，任务通过 channel 派发，每个任务在独立工作线程中通过 `block_on` 执行其 async future。
- **并发模型**：1:1 调度——同一时刻每个工作线程只执行一个任务，任务间无抢占，CPU 时间片由 OS 调度器分配。
- **资源占用**：每工作线程固定占用 OS 线程栈（典型 2~8 MB），线程数 = CPU 核数（默认）或 `XHJOB_THREAD_POOL_SIZE`。
- **适用场景**：
  - CPU 密集型任务：压缩、加解密、大文件处理、计算密集 shell 脚本。
  - 需严格并发控制的场景：限制对外部系统（DB / API）的并发连接数，避免压垮下游。
  - 任务间需强隔离：避免某个任务的 CPU 长时间占用拖累其他任务。
- **启用方式**：`XHJOB_POOL_MODE=thread`。

### 模式对比

| 维度 | async task 池（async，默认） | 1:1 线程池（thread） |
|---|---|---|
| 底层实现 | tokio async runtime | std::thread + crossbeam-channel |
| 调度模型 | M:N（多 task 复用少线程） | 1:1（一线程一任务） |
| 执行线程名 | `xhjob-tokio`（=CPU 核数） | `xhjob-worker-N`（=线程数） |
| 默认并发上限 | 1024（`XHJOB_ASYNC_POOL_SIZE`） | CPU 核数（`XHJOB_THREAD_POOL_SIZE`） |
| 单任务内存开销 | 几 KB（task 状态机按需分配） | 2~8 MB（OS 线程栈） |
| IO 密集型表现 | 优秀（await 让出，高吞吐） | 一般（线程被 IO 阻塞时占位） |
| CPU 密集型表现 | 一般（少数 tokio 线程易被打满） | 优秀（每任务独占线程，OS 调度公平） |
| 任务隔离性 | 弱（共享线程，长 CPU 任务会饿死其他） | 强（独立线程，互不抢占） |
| 上下文切换成本 | 低（用户态切换） | 较高（内核态线程切换） |
| 推荐场景 | HTTP / shell 等待 / 高并发短任务 | CPU 计算 / 严格限流 / 强隔离 |

### 实测差异验证

通过 `test_xhjob_pool_diff.php`（位于集成包根目录）实测，派发 4 个 `sleep(3)` 任务，CPU 核数=3，thread 模式 `XHJOB_THREAD_POOL_SIZE=2`：

| 验证维度 | async 模式 | thread 模式 |
|---|---|---|
| daemon 线程名 | `xhjob-tokio` ×3 | `xhjob-worker-0`, `xhjob-worker-1` |
| 4 任务时间戳分布（delta ms） | 0, 102, 204, 307 | 0, 100, 3013, 3111 |
| 4 任务并发情况 | **全部同时执行** | **分 2 批执行**（前 2 个立即，后 2 个延迟 3s） |
| 总耗时 | **3.61s**（≈ 1 × sleep 时间） | **6.31s**（≈ 2 × sleep 时间） |

**实测结论**：

- **async 模式**：任务在 tokio async runtime 上调度，通过 Semaphore 控制并发。4 个 `sleep(3)` 全部同时执行（async/await 在 sleep 时 yield 让出线程），总耗时 ≈ 3s。
- **thread 模式**：每个任务在独立 OS 线程中 `block_on` 执行，并发度受限于线程数。4 个 `sleep(3)` 分 2 批执行（2 个线程 × 2 批），总耗时 ≈ 6s。
- **本质区别**：async = M:N 高并发（IO 密集型最优）；thread = 真并行受限于线程数（CPU 密集型或需严格隔离时使用）。

运行差异验证脚本：

```bash
EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so
php -d extension=$EXT test_xhjob_pool_diff.php
```

---

## 使用说明

### 1. 启动 daemon

daemon 是 Rust 内核进程，由 PHP 进程 fork 创建，负责调度任务执行。启动方式有两种：

**方式 A：通过 XhjobService 显式管理**（推荐用于 CLI 脚本）

```php
use Xhjob\XhjobService;

$svc = new XhjobService('default', '/var/lib/xhjob');
$pid = $svc->start();          // 启动 daemon，返回 PID
$svc->ensureRunning();         // 或确保已启动（未运行则启动）
$svc->wait(10, true);          // 等待进入 running 状态

// 停止 / 重启
$svc->stop();
$svc->ensureStopped();
$newPid = $svc->restart();
```

**方式 B：通过 helper 函数管理**（推荐用于 Web 请求）

```php
// helper.php 提供 xhjob_service() / xhjob_manager() / xhjob_task() 辅助函数
xhjob_service()->ensureRunning();   // 确保 daemon 已启动
```

### 2. 创建任务

通过 `TaskManager` + `TaskBuilder` 创建任务：

```php
use Xhjob\TaskManager;
use Xhjob\TaskBuilder;

$mgr = new TaskManager('default', '/var/lib/xhjob');

// 示例 1：每分钟执行 shell 命令
$id = $mgr->create(
    TaskBuilder::shell('echo hello-xhjob')
        ->cron('*/1 * * * *')
        ->withRetry(3, 2)
        ->tag('demo')
);

// 示例 2：每 60 秒调用 HTTP webhook
$id = $mgr->create(
    TaskBuilder::http('POST', 'https://api.example.com/webhook')
        ->withHeaders(['X-Token: secret'])
        ->withBody('{"event":"tick"}')
        ->every(60)
        ->timeout(10)
);

// 示例 3：一次性延迟任务（3 秒后执行）
$id = $mgr->create(
    TaskBuilder::shell('php /app/bin/cleanup.php')
        ->countdown(3)
        ->withRetry(2, 1)
);
```

### 3. 查询状态与结果

```php
// 查询任务状态
$state = $mgr->state($id);
// 返回：['state' => 'success', 'attempts' => 1, 'execution_count' => 5, ...]

// 查询任务结果
$result = $mgr->result($id);
// 返回：['stdout' => '...', 'stderr' => '...', 'exit_code' => 0, ...]

// 等待任务进入指定状态（最长 30 秒）
$ok = $mgr->waitForState($id, 'success', 30);

// 等待任务终态并返回结果
$result = $mgr->waitForResult($id, 30);
```

### 4. 通过 Facade 使用（ThinkPHP 上下文）

Facade 绑定到容器中的 `xhjob.manager`，即 `TaskManager` 实例：

```php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

// 启动 daemon
xhjob_service()->ensureRunning();

// 创建并派发任务
$id = Xhjob::create(
    TaskBuilder::shell('echo hello')
        ->cron('*/1 * * * *')
);

// 查询
$state  = Xhjob::state($id);
$result = Xhjob::result($id);
```

### 5. 任务编排

#### chain（顺序流水线）

按顺序执行每个子任务，**上一个任务的 stdout 作为下一个任务的 stdin**。任一步失败则链终止。

```php
$chainId = $mgr->createChain([
    TaskBuilder::shell('echo step1-data'),
    TaskBuilder::shell('tr a-z A-Z'),         // 接收 step1 的 stdout
    TaskBuilder::shell('wc -c'),              // 接收 step2 的 stdout
]);

$state = $mgr->chainState($chainId);
```

#### group（并行批处理）

并行执行所有子任务，全部完成后返回 group 结果汇总。

```php
$groupId = $mgr->createGroup([
    TaskBuilder::http('GET', 'https://api.example.com/user/1'),
    TaskBuilder::http('GET', 'https://api.example.com/user/2'),
    TaskBuilder::http('GET', 'https://api.example.com/user/3'),
]);

$state = $mgr->groupState($groupId);
```

#### chord（group + 回调）

并行执行所有 header 任务，**全部成功后**执行 callback 任务。任一 header 失败时 chord 转 `partial_failed` 终态，不派发 callback。参考 Celery chord。

```php
$chordId = $mgr->createChord(
    [
        TaskBuilder::shell('echo header-1'),
        TaskBuilder::shell('echo header-2'),
        TaskBuilder::shell('echo header-3'),
    ],
    TaskBuilder::shell('echo callback-done')
        ->withMeta('{"source":"chord"}')
);

$state = $mgr->chordState($chordId);
```

### 6. 可观测性

#### inspect（运行时检查）

对齐 Celery `inspect`，提供 4 种查询模式：

```php
$active     = $mgr->inspect('active');      // 运行中任务
$registered = $mgr->inspect('registered');  // cron 注册任务
$scheduled  = $mgr->inspect('scheduled');   // 未来触发任务
$stats      = $mgr->inspect('stats');        // 聚合统计（默认）
// stats 含 total / success / failed / pending / running 等字段
```

#### pull_events（事件拉取）

拉取 daemon 全局事件流，支持按事件类型过滤。10 类事件：`started` / `succeeded` / `failed` / `missed` / `cancelled` / `paused` / `resumed` / `expired` / `max_instances_reached` / `rate_limited`。

```php
// 拉取最近 5 分钟的所有事件
$events = $mgr->pullEvents(time() - 300);

// 仅拉取失败事件
$failedEvents = $mgr->pullEvents(0, 'failed');

// 拉取单任务事件日志
$taskEvents = $mgr->logs($taskId, time() - 3600);
```

#### report_progress（进度上报）

对齐 Celery `update_state(state='PROGRESS', meta=...)`，长任务可向前端反馈进度。

```php
$mgr->reportProgress($taskId, 75, '{"processed":750,"total":1000}');

// state 字段中会携带 progress 信息
$state = $mgr->state($taskId);
// ['progress' => 75, 'progress_meta' => '{"processed":750,"total":1000}', ...]
```

### 7. 持久化与恢复

启用 `XHJOB_PERSIST=1` 后，daemon 使用 SqliteStore + WAL 持久化任务状态。daemon 重启后：

- 未完成的任务（`Pending`）会自动恢复。
- `acks_late=true` 的 `Running` 任务会被**自动重置为 `Pending` 并重新触发**（crash recovery）。
- 已完成的任务（`Success` / `Failed`）保持终态，`execution_count` 不丢失。

```php
// 关键业务任务：持久化 + acks_late + 失败不 ack
TaskBuilder::shell('php /app/bin/sync-orders.php')
    ->cron('*/10 * * * *')
    ->persist(true)          // 持久化任务状态
    ->acksLate(true)         // 完成后才 ack，crash 后重投
    ->acksOnFailure(false)   // 失败时不 ack，便于重试
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->dispatch();
```

### 8. 控制任务

```php
$mgr->stop($id);          // 取消正在执行的任务
$mgr->restart($id);       // 重新入队
$mgr->pause($id);         // 暂停（cron / interval 任务）
$mgr->resume($id);        // 恢复
$mgr->remove($id);        // 删除任务
$mgr->reschedule($id, '0 * * * *');  // 修改 cron 表达式
```

### 9. HTTP 路由（可选）

集成包提供 `XhjobTask` 控制器，路由前缀 `/xhjob`，常用接口：

| Method | Path | 说明 |
|---|---|---|
| GET | `/xhjob/index` | 首页 / 状态 |
| GET | `/xhjob/status` | daemon 状态 |
| GET | `/xhjob/health` | 健康检查 |
| POST | `/xhjob/start` | 启动 daemon |
| POST | `/xhjob/stop` | 停止 daemon / 任务 |
| GET | `/xhjob/list` | 任务列表 |
| POST | `/xhjob/createShell` | 创建 shell 任务 |
| POST | `/xhjob/createHttp` | 创建 HTTP 任务 |
| POST | `/xhjob/createCron` | 创建 cron 任务 |
| POST | `/xhjob/createChain` | 创建任务链 |
| POST | `/xhjob/createGroup` | 创建任务组 |
| GET | `/xhjob/state` | 任务状态 |
| GET | `/xhjob/result` | 任务结果 |
| DELETE | `/xhjob/delete` | 删除任务 |

完整路由列表见 `route/app.php`。

---

## 配置项

`config/xhjob.php` 全量配置说明：

| 配置键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `service_name` | string | `'default'`（`XHJOB_SERVICE`） | 服务名，多实例时通过该值区分 daemon 与 sock 文件。 |
| `data_dir` | string\|null | `null`（`XHJOB_DATA_DIR`） | 数据目录，`null` 时由扩展内置默认值；持久化模式下 SQLite 文件位于此目录。 |
| `api_token` | string\|null | `null`（`XHJOB_API_TOKEN`） | API Token，可选，用于后续 HTTP 网关鉴权。 |
| `pool_mode` | string | `'async'`（`XHJOB_POOL_MODE`） | 任务执行池模式：`'async'`（async task 池，默认）或 `'thread'`（1:1 线程池）。`'coroutine'` 为 `'async'` 的兼容别名。 |

完整配置文件示例：

```php
return [
    'service_name' => env('XHJOB_SERVICE', 'default'),
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    'api_token'    => env('XHJOB_API_TOKEN', null),
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

> 注意：`pool_mode` 仅在 daemon 启动时读取一次。切换模式需先 `xhjob_stop` 再以新模式 `xhjob_start`。

---

## 环境变量

所有 `XHJOB_*` 环境变量在 daemon 启动前读取，运行期间修改不生效。

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `XHJOB_POOL_MODE` | `async` | 任务执行池模式：`async`（async task 池，默认）或 `thread`（1:1 线程池）。`coroutine` 为 `async` 的兼容别名。 |
| `XHJOB_ASYNC_POOL_SIZE` | `1024` | async task 池模式下的最大并发任务数。兼容别名：`XHJOB_COROUTINE_POOL_SIZE`。 |
| `XHJOB_THREAD_POOL_SIZE` | CPU 核数 | 1:1 线程池模式下的工作线程数。 |
| `XHJOB_PERSIST` | `0` | 是否启用持久化：`1` 启用 SqliteStore + WAL，`0` 使用 InMemoryStore。 |
| `XHJOB_SERVICE` | `default` | 服务名，多实例隔离标识。 |
| `XHJOB_DATA_DIR` | 内置默认 | 数据目录，存放 sock / SQLite / 日志文件。 |
| `XHJOB_API_TOKEN` | 空 | HTTP 网关鉴权 Token（可选）。 |

---

## 测试套件

集成包提供 6 套测试脚本，覆盖深度功能、持久化、异步队列与池模式对比。

### 测试脚本一览

| 脚本 | 步数 | 覆盖范围 |
|---|---|---|
| `test_xhjob.php` | 12 步 | 基础 CLI 测试：daemon 生命周期 + 任务 CRUD。 |
| `test_xhjob_deep.php` | 29 步 | 深度功能测试，覆盖全部任务参数属性：循环任务、maxExecutions、withRetry、allowOverlap、timeout/softTimeout/expires/jitter、retry_backoff/ignore_result/acks_late、misfire_grace_time/timezone/tags/meta、result_ttl/rate_limit/coalesce、start_date/end_date/persist、withId+replaceExisting、priority/chain/group。 |
| `test_xhjob_persist.php` | 8 步 | 持久化深度测试：daemon 重启后任务恢复、execution_count 不丢失、循环任务继续触发、acks_late Running 任务重投、SQLite 文件持久存储。 |
| `test_xhjob_async_queue.php` | 13 步 | 后台任务队列异步测试（对标 APScheduler/Celery）：非阻塞派发、批量并发、进度上报、countdown 延迟、chord 全部成功 / 部分失败、inspect active/registered/scheduled/stats、pull_events。 |
| `test_xhjob_pool_mode.php` | — | 两种池模式功能对比测试：async 与 thread 模式下的功能行为一致性验证（各 10 步）。 |
| `test_xhjob_pool_diff.php` | — | 两种池模式真实差异验证：通过 `/proc/{pid}/task` 读取线程名 + 时间戳分析，证明 async（M:N）与 thread（1:1）的并发模型差异。 |

### 运行命令

```bash
EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so

# 1. 基础测试（12 步）
php -d extension=$EXT test_xhjob.php

# 2. 深度功能测试（29 步）
php -d extension=$EXT test_xhjob_deep.php

# 3. 持久化测试（8 步）
php -d extension=$EXT test_xhjob_persist.php

# 4. 异步队列测试（13 步）
php -d extension=$EXT test_xhjob_async_queue.php

# 5. 池模式功能对比测试
XHJOB_POOL_MODE=async   php -d extension=$EXT test_xhjob_pool_mode.php
XHJOB_POOL_MODE=thread  php -d extension=$EXT test_xhjob_pool_mode.php

# 6. 池模式真实差异验证（线程名 + 并发时间戳分析）
php -d extension=$EXT test_xhjob_pool_diff.php
```

### 测试结果解读

每个脚本以 `[N] 描述 ... PASS/FAIL` 形式逐行输出，末尾汇总 `PASS=N FAIL=M`。所有测试脚本均使用独立 service 名与独立 data_dir，互不污染。

---

## 目录结构

```
xhjob-thinkphp8-extend/
├── Xhjob/                              # extend 第三方库（复制到 tp/extend/Xhjob/）
│   ├── Exception/                      # 异常体系
│   ├── facade/
│   │   └── Xhjob.php                   # ThinkPHP Facade（绑定 xhjob.manager）
│   ├── Client.php                      # 跨环境客户端
│   ├── ServiceProvider.php             # ThinkPHP 服务提供者
│   ├── TaskBuilder.php                 # PHP fluent builder（38+ 链式方法）
│   ├── TaskManager.php                 # 任务管理门面
│   ├── XhjobService.php                # daemon 生命周期管理
│   └── helper.php                      # 全局辅助函数
├── controller/
│   └── XhjobTask.php                   # 控制器
├── route/
│   └── app.php                         # 路由
├── config/
│   └── xhjob.php                       # 配置
├── service.php                         # ServiceProvider 注册
├── test_xhjob.php                      # 基础测试（12 步）
├── test_xhjob_deep.php                 # 深度功能测试（29 步）
├── test_xhjob_persist.php              # 持久化测试（8 步）
├── test_xhjob_async_queue.php          # 异步队列测试（13 步）
├── test_xhjob_pool_mode.php            # 池模式功能对比测试
├── test_xhjob_pool_diff.php           # 池模式真实差异验证
├── EVALUATION.md                       # APScheduler / Celery 对标评估
└── README.md                           # 本文件
```

---

## APScheduler / Celery 对标

xhjob 在单机 PHP 场景下与 Python 生态两套成熟调度方案的能力对标详见 [EVALUATION.md](./EVALUATION.md)。简要结论：

| 对照系 | 总项数 | 已实现/等价 | 部分实现 | 不适用 | 未实现 |
|---|---|---|---|---|---|
| APScheduler（单机调度器） | 25 | 21 | 1 | 3 | 0 |
| Celery（分布式队列） | 35 | 22 | 8 | 5 | 0 |

核心结论：单机调度核心语义对齐度高，不追求分布式能力（多 Broker / 多 worker / Flower 等），避免复杂度膨胀。详细特性矩阵请参阅 [EVALUATION.md](./EVALUATION.md)。
