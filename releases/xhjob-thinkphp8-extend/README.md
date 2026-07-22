# xhjob ThinkPHP 8 扩展集成

> 基于 Rust（ext-php-rs 0.15）内核的 PHP 扩展，为 ThinkPHP 8 提供**单机异步任务调度**能力，零外部依赖（不依赖 Redis / RabbitMQ / Swoole / Supervisor），支持**两种任务执行池模式**（async task 池 / 1:1 线程池）按场景切换。

本集成包以 `extend/` 第三方库形态接入 ThinkPHP 8，封装 24 个 PHP 扩展函数与 38+ 个链式构建方法，覆盖触发器、重试、超时、并发控制、任务编排（chain / group / chord）、持久化恢复、可观测性等完整能力，是 APScheduler / Celery 在单机 PHP 场景下的等价替代方案。

---

## 目录

- [核心特性](#核心特性)
- [两种任务执行池模式](#两种任务执行池模式)
- [快速开始](#快速开始)
- [配置项](#配置项)
- [环境变量](#环境变量)
- [TaskBuilder API](#taskbuilder-api)
- [任务编排](#任务编排)
- [可观测性](#可观测性)
- [持久化与恢复](#持久化与恢复)
- [测试套件](#测试套件)
- [APScheduler / Celery 对标](#apscheduler--celery-对标)
- [目录结构](#目录结构)

---

## 核心特性

- **两种任务执行池模式**（重点能力）
  - async task 池（`async`，默认；兼容别名 `coroutine`）：基于 tokio M:N 调度，IO 密集型场景高并发、低资源占用，最大并发 1024。
  - 1:1 线程池（`thread`）：基于 `std::thread` + `crossbeam-channel`，每个任务在独立工作线程中 `block_on` 执行，适合 CPU 密集型或需严格并发控制的场景。
  - 通过 `XHJOB_POOL_MODE` 环境变量或 `config/xhjob.php` 的 `pool_mode` 项在 daemon 启动前切换。
- **零外部依赖**：不引入 Redis / MQ / Swoole / Supervisor，Rust daemon 进程内调度，部署形态仅多一个 `.so` 扩展。
- **全触发器谱**：CronTrigger（5/6 段，含秒级）/ IntervalTrigger / DateTrigger（run_at / countdown）/ start_date / end_date。
- **完整调度控制**：retry + retry_backoff（指数退避）/ timeout + soft_timeout（SIGTERM→SIGKILL 升级链）/ maxInstances + allowOverlap + coalesce / jitter / expires / misfire_grace_time / rate_limit / priority / max_executions。
- **任务编排三件套**：chain（顺序流水线）/ group（并行批处理）/ chord（group + 回调，对齐 Celery chord）。
- **可靠性与恢复**：persist（SqliteStore + WAL）/ acks_late（crash recovery）/ ignore_result / result_ttl / 10 类事件。
- **可观测性**：inspect（active / registered / scheduled / stats）/ pull_events / report_progress。
- **多实例与隔离**：named services（同时运行多个独立 daemon）/ data_dir（自定义数据目录）。
- **HTTP / Shell 双模式 + 增强**：HTTP 代理（http / https / socks5 / socks5h + Basic Auth）/ Shell 编码转换（GBK / Big5 / auto）/ timezone。
- **PHP 扩展函数（24 个）**：`xhjob_start/stop/restart/status/dispatch/state/result/remove/pause/resume/cancel/list/requeue/reschedule/get/events/chain/chain_state/group/group_state/chord/chord_state/report_progress/pull_events/inspect`。
- **TaskBuilder 链式方法（38+ 个）**：覆盖任务类型、触发器、重试、超时、并发、持久化、编排、事件、代理、编码、时区等全维度。

---

## 两种任务执行池模式

xhjob 内核提供两种**可切换的任务执行池模式**，针对不同负载特征做出取舍。模式选择在 daemon 启动时确定，运行期间不可热切换；如需切换需 `xhjob_stop` 后以新的 `XHJOB_POOL_MODE` 重新 `xhjob_start`。

### 模式一：async task 池（async，默认）

> **术语澄清**：Rust 没有语言级"协程"——只有 `async/await` + `Future`（编译期转为状态机）由 runtime（tokio）poll 驱动。这与 Go goroutine / Python coroutine 的"协程"概念不同。`coroutine` 作为兼容别名保留，等价于 `async`，建议新代码使用 `async`。

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
| 启动延迟 | 极低（提交 future） | 略高（线程已预创建，无 fork） |
| 推荐场景 | HTTP / shell 等待 / 高并发短任务 | CPU 计算 / 严格限流 / 强隔离 |

### 架构示意

```
┌─────────────────────────────────────────────────────────────────┐
│                    PHP 进程（FPM / CLI）                         │
│   Xhjob\Facade / TaskManager  ──IPC──┐                          │
└──────────────────────────────────────┼──────────────────────────┘
                                       │ xhjob_dispatch / state ...
┌──────────────────────────────────────▼──────────────────────────┐
│              Rust daemon（xhjob 内核进程）                       │
│                                                                  │
│   ┌─────────────┐   ┌─────────────┐   ┌─────────────────────┐   │
│   │ Scheduler   │──▶│ Trigger     │──▶│ Execution Pool      │   │
│   │ (cron/      │   │ Evaluator   │   │                     │   │
│   │  interval/  │   │ (jitter/    │   │  XHJOB_POOL_MODE =  │   │
│   │  date)      │   │  coalesce)  │   │  async | thread   │   │
│   └─────────────┘   └─────────────┘   └─────────┬───────────┘   │
│                                                  │               │
│              ┌───────────────────────────────────┴──────────┐    │
│              ▼                                              ▼    │
│   ┌──────────────────────────┐              ┌────────────────────┐│
│   │ async 模式（M:N）        │              │ thread 模式（1:1） ││
│   │ ┌──────────────────────┐ │              │ ┌────────────────┐ ││
│   │ │ tokio runtime        │ │              │ │ worker thread 1│ ││
│   │ │  ├── worker thread A │ │              │ │  block_on(fut) │ ││
│   │ │  │   ├── task₁.await │ │              │ ├── worker thread 2│ ││
│   │ │  │   ├── task₂.await │ │              │ │  block_on(fut) │ ││
│   │ │  │   └── task₃.await │ │              │ ├── ...          │ ││
│   │ │  └── worker thread B │ │              │ └── worker thread N│ ││
│   │ │      └── task₄.await │ │              │   (N = CPU 核数) │ ││
│   │ └──────────────────────┘ │              └────────────────────┘││
│   │ 上限 1024（可配）        │              │ 上限 = 线程数       ││
│   └──────────────────────────┘              └────────────────────┘│
│                                                                  │
│   ┌──────────────────────────────────────────────────────────┐   │
│   │ Store：InMemoryStore（默认） / SqliteStore + WAL（persist）│  │
│   └──────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

### 配置示例

通过环境变量（推荐用于容器 / systemd 部署）：

```bash
# async task 池（默认），自定义并发上限
export XHJOB_POOL_MODE=async
export XHJOB_ASYNC_POOL_SIZE=2048

# 或：1:1 线程池，自定义线程数
export XHJOB_POOL_MODE=thread
export XHJOB_THREAD_POOL_SIZE=8
```

通过 ThinkPHP 配置文件 `config/xhjob.php`：

```php
return [
    'service_name' => env('XHJOB_SERVICE', 'default'),
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    'api_token'    => env('XHJOB_API_TOKEN', null),
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'), // 'async' | 'thread'（coroutine 为 async 兼容别名）
];
```

> 注意：`pool_mode` 仅在 daemon 启动时读取一次。切换模式需先 `xhjob_stop` 再以新模式 `xhjob_start`。

---

## 快速开始

### 1. 安装 xhjob PHP 扩展

将编译产物 `xhjob.so` 放入 PHP 扩展目录，并在 `php.ini` 启用：

```ini
extension=xhjob
```

或运行时临时加载（CLI 测试场景）：

```bash
php -d extension=/path/to/xhjob.so your-script.php
```

### 2. 复制集成包到 ThinkPHP 项目

假设 ThinkPHP 8 项目根目录为 `tp/`：

```bash
# 复制 extend 第三方库
cp -r Xhjob/  tp/extend/Xhjob/

# 复制配置
cp config/xhjob.php  tp/config/xhjob.php

# 复制控制器
cp controller/XhjobTask.php  tp/app/controller/XhjobTask.php

# 合并路由（追加到 tp/route/app.php）
cat route/app.php >> tp/route/app.php

# 注册服务提供者（在 tp/app/service.php 追加）
echo '\Xhjob\ServiceProvider::class,' >> tp/app/service.php
```

### 3. 配置环境变量（可选）

```bash
export XHJOB_POOL_MODE=async   # 或 thread（coroutine 为 async 兼容别名）
export XHJOB_PERSIST=1             # 启用 SQLite 持久化
export XHJOB_DATA_DIR=/var/lib/xhjob
```

### 4. 第一个任务

通过 Facade 创建并派发一个 cron 任务（Facade 绑定到容器中的 `xhjob.manager`，即 `TaskManager` 实例）：

```php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

// 启动 daemon（Facade 不暴露 daemon 生命周期方法，通过 helper 或 XhjobService 管理）
xhjob_service()->ensureRunning();   // 见 helper.php

// 创建一个每分钟执行一次的 shell 任务
$id = Xhjob::create(
    TaskBuilder::shell('echo hello-xhjob')
        ->cron('*/1 * * * *')
        ->withRetry(3, 2)
        ->tag('demo')
);

// 查询状态与结果
$state  = Xhjob::state($id);
$result = Xhjob::result($id);
```

通过 `TaskManager` 显式管理 daemon 生命周期：

```php
use Xhjob\XhjobService;
use Xhjob\TaskManager;
use Xhjob\TaskBuilder;

$svc = new XhjobService('default', '/var/lib/xhjob');
$svc->ensureRunning();           // 确保 daemon 已启动
$pid = $svc->start();            // 或显式启动，返回 daemon PID

$mgr = new TaskManager('default', '/var/lib/xhjob');
$id  = $mgr->create(
    TaskBuilder::http('POST', 'https://api.example.com/webhook')
        ->withHeaders(['X-Token: secret'])
        ->withBody('{"event":"tick"}')
        ->every(60)
        ->timeout(10)
);

$mgr->waitForState($id, 'success', 30);
$r = $mgr->result($id);
```

### 5. 通过 HTTP 路由验证

启动 ThinkPHP 内建服务器后访问：

```bash
curl http://localhost:8000/xhjob/index    # 状态
curl -X POST http://localhost:8000/xhjob/start
curl "http://localhost:8000/xhjob/list"
```

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

---

## 环境变量

所有 `XHJOB_*` 环境变量在 daemon 启动前读取，运行期间修改不生效。

| 环境变量 | 默认值 | 说明 |
|---|---|---|
| `XHJOB_POOL_MODE` | `async` | 任务执行池模式：`async`（async task 池，默认）或 `thread`（1:1 线程池）。`coroutine` 为 `async` 的兼容别名。 |
| `XHJOB_ASYNC_POOL_SIZE` | `1024` | async task 池模式下的最大并发任务数。兼容别名：`XHJOB_COROUTINE_POOL_SIZE`。 |
| `XHJOB_THREAD_POOL_SIZE` | CPU 核数 | 多线程池模式下的工作线程数。 |
| `XHJOB_PERSIST` | `0` | 是否启用持久化：`1` 启用 SqliteStore + WAL，`0` 使用 InMemoryStore。 |
| `XHJOB_SERVICE` | `default` | 服务名，多实例隔离标识。 |
| `XHJOB_DATA_DIR` | 内置默认 | 数据目录，存放 sock / SQLite / 日志文件。 |
| `XHJOB_API_TOKEN` | 空 | HTTP 网关鉴权 Token（可选）。 |

---

## TaskBuilder API

`TaskBuilder` 提供 fluent 链式 API，最终输出 `xhjob_dispatch` / `xhjob_chain` / `xhjob_group` / `xhjob_chord` 所需的 JSON。所有链式方法返回 `$this`。

### 静态工厂

| 方法 | 说明 |
|---|---|
| `TaskBuilder::shell(string $cmd)` | 创建 shell 任务 |
| `TaskBuilder::http(string $method, string $url)` | 创建 HTTP 任务 |
| `TaskBuilder::chain(array $builders)` | 创建任务链（dispatch 时调用 `xhjob_chain`） |
| `TaskBuilder::group(array $builders)` | 创建任务组（dispatch 时调用 `xhjob_group`） |
| `TaskBuilder::chord(array $headerBuilders, self $callback)` | 创建 chord（dispatch 时调用 `xhjob_chord`） |
| `TaskBuilder::fromJson(string $json)` | 从 JSON 字符串反序列化 |

### 任务类型与传输

| 方法 | 说明 |
|---|---|
| `viaHttp(string $method, string $url)` | 设置为 HTTP 任务 |
| `viaShell(string $cmd)` | 设置为 shell 任务 |
| `withHeaders(array $h)` | HTTP 请求头（仅 http 任务） |
| `withBody(string $b)` | HTTP 请求体（仅 http 任务） |
| `proxy(string $p)` | HTTP 代理（http/https/socks5/socks5h + Basic Auth） |
| `encoding(string $enc)` | Shell 输出编码转换（GBK / Big5 / auto） |

### 触发器

| 方法 | 说明 |
|---|---|
| `cron(string $expr)` | 5/6 段 cron 表达式，如 `0 * * * *` 或秒级 `*/30 * * * * *` |
| `every(int $secs)` | 固定间隔（秒）触发 |
| `runAt(int $ts)` | 一次性运行时间戳（Unix 秒） |
| `countdown(int $secs)` | 相对当前延迟 N 秒触发，等价 `runAt(time() + $secs)`，与 `runAt` 同时设置时 `runAt` 优先 |
| `startAt(int $ts)` | 任务起始时间戳 |
| `endAt(int $ts)` | 任务结束时间戳 |

### 重试与超时

| 方法 | 说明 |
|---|---|
| `withRetry(int $max, int $delay = 1)` | 最大重试次数与重试间隔（秒） |
| `retryBackoff(bool $on = true)` | 启用指数退避重试 |
| `timeout(int $secs)` | 硬超时（秒） |
| `softTimeout(int $secs)` | 软超时（SIGTERM→SIGKILL 升级链） |

### 并发与调度控制

| 方法 | 说明 |
|---|---|
| `priority(int $p)` | 优先级（数值越大越优先） |
| `maxInstances(int $n)` | 同任务最大并发实例数 |
| `allowOverlap(bool $on = true)` | 允许重叠执行 |
| `coalesce(bool $on = true)` | 错过多次触发时合并为 1 次 |
| `maxExecutions(int $n)` | 最大执行次数（0 = 无限） |
| `jitter(int $secs)` | 随机抖动避免惊群 |
| `expires(int $secs)` | 任务过期自动丢弃 |
| `misfireGraceTime(int $secs)` | 容忍迟到的宽限时间 |
| `rateLimit(int $count, int $window)` | 速率限制：窗口内允许的次数 / 窗口大小（秒） |

### 可靠性

| 方法 | 说明 |
|---|---|
| `persist(bool $on = true)` | 启用持久化（SqliteStore + WAL） |
| `acksLate(bool $on = true)` | 延迟确认（任务完成后才 ack，crash 后重投） |
| `acksOnFailure(bool $on = true)` | 失败时是否 ack |
| `ignoreResult(bool $on = true)` | 不存储结果 |
| `resultTtl(int $secs)` | 结果 TTL（秒） |

### 元数据与标识

| 方法 | 说明 |
|---|---|
| `id(string $id)` | 指定任务 ID |
| `replaceExisting(bool $on = true)` | 同 ID 任务替换策略 |
| `tag(string $tag)` | 追加单个标签 |
| `tags(array $tags)` | 设置标签数组 |
| `meta(?string $json)` | 元数据（任意 JSON 字符串） |
| `timezone(string $tz)` | 时区标识符，如 `Asia/Shanghai` |

### 服务与目录

| 方法 | 说明 |
|---|---|
| `service(string $name)` | 指定服务名（多实例） |
| `dataDir(string $dir)` | 指定数据目录 |

### 终结方法

| 方法 | 说明 |
|---|---|
| `dispatch(?string $service = null, ?string $dataDir = null)` | 派发到 daemon，返回 task_id / chain_id / group_id / chord_id |
| `toArray()` | 导出为关联数组 |
| `toJson()` | 导出为 JSON 字符串 |

### 综合示例

```php
use Xhjob\TaskBuilder;

$id = TaskBuilder::shell('curl -s https://api.example.com/sync')
    ->cron('*/5 * * * *')
    ->withRetry(5, 3)
    ->retryBackoff(true)
    ->timeout(30)
    ->softTimeout(25)
    ->maxInstances(1)
    ->coalesce(true)
    ->jitter(2)
    ->misfireGraceTime(60)
    ->persist(true)
    ->acksLate(true)
    ->priority(10)
    ->tag('sync')
    ->tag('critical')
    ->timezone('Asia/Shanghai')
    ->dispatch();
```

---

## 任务编排

### chain（顺序流水线）

按顺序执行每个子任务，**上一个任务的 stdout 作为下一个任务的 stdin**。任一步失败则链终止。

```php
use Xhjob\TaskBuilder;
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

$chainId = $mgr->createChain([
    TaskBuilder::shell('echo step1-data'),
    TaskBuilder::shell('tr a-z A-Z'),         // 接收 step1 的 stdout
    TaskBuilder::shell('wc -c'),              // 接收 step2 的 stdout
]);

$state = $mgr->chainState($chainId);
```

或通过 TaskBuilder 工厂：

```php
$chainId = TaskBuilder::chain([
    TaskBuilder::shell('echo step1-data'),
    TaskBuilder::shell('tr a-z A-Z'),
    TaskBuilder::shell('wc -c'),
])->dispatch();
```

### group（并行批处理）

并行执行所有子任务，全部完成后返回 group 结果汇总。

```php
$groupId = $mgr->createGroup([
    TaskBuilder::http('GET', 'https://api.example.com/user/1'),
    TaskBuilder::http('GET', 'https://api.example.com/user/2'),
    TaskBuilder::http('GET', 'https://api.example.com/user/3'),
]);

$state = $mgr->groupState($groupId);
```

### chord（group + 回调）

并行执行所有 header 任务，**全部成功后**执行 callback 任务，callback 的 `meta` 携带所有 header 结果。任一 header 失败时 chord 转 `partial_failed` 终态，不派发 callback。参考 Celery chord。

```php
$chordId = $mgr->createChord(
    // header：3 个并行任务
    [
        TaskBuilder::shell('echo header-1'),
        TaskBuilder::shell('echo header-2'),
        TaskBuilder::shell('echo header-3'),
    ],
    // callback：header 全部成功后执行
    TaskBuilder::shell('echo callback-done')
        ->withMeta('{"source":"chord"}')
);

$state = $mgr->chordState($chordId);
// state 包含 id / header_task_ids / callback_json / callback_task_id /
//                  state / created_at / updated_at
```

---

## 可观测性

### inspect（运行时检查）

对齐 Celery `inspect`，提供 4 种查询模式：

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

$active     = $mgr->inspect('active');      // 运行中任务
$registered = $mgr->inspect('registered');  // cron 注册任务
$scheduled  = $mgr->inspect('scheduled');   // 未来触发任务
$stats      = $mgr->inspect('stats');       // 聚合统计（默认）
```

### pull_events（事件拉取）

拉取 daemon 全局事件流，支持按事件类型过滤。10 类事件：`started` / `succeeded` / `failed` / `missed` / `cancelled` / `paused` / `resumed` / `expired` / `max_instances_reached` / `rate_limited`。

```php
// 拉取最近 5 分钟的所有事件
$events = $mgr->pullEvents(time() - 300);

// 仅拉取失败事件
$failedEvents = $mgr->pullEvents(0, 'failed');

// 拉取单任务事件日志（logs 是 pullEvents 的按 id 过滤版本）
$taskEvents = $mgr->logs($taskId, time() - 3600);
```

### report_progress（进度上报）

对齐 Celery `update_state(state='PROGRESS', meta=...)`，长任务可向前端反馈进度。

```php
// 在任务运行中上报进度（典型在 shell 脚本或 HTTP 回调中调用）
$mgr->reportProgress($taskId, 75, '{"processed":750,"total":1000}');

// state 字段中会携带 progress 信息
$state = $mgr->state($taskId);
```

### 轮询等待

```php
// 等待任务进入指定状态（最长 30 秒）
$ok = $mgr->waitForState($taskId, 'success', 30);

// 等待任务终态并返回结果（最长 30 秒）
$result = $mgr->waitForResult($taskId, 30);
```

---

## 持久化与恢复

### 持久化模式

xhjob 内核支持两种 store：

| Store | 启用方式 | 说明 |
|---|---|---|
| `InMemoryStore` | 默认 | 进程内存储，daemon 重启后任务丢失。 |
| `SqliteStore` + WAL | `XHJOB_PERSIST=1` 或 `persist` feature | SQLite 文件持久存储，daemon 重启后恢复未完成任务状态。 |

启用持久化的方式：

```bash
# 通过环境变量（推荐）
export XHJOB_PERSIST=1
export XHJOB_DATA_DIR=/var/lib/xhjob

# 或在编译时启用 persist feature（默认即 SqliteStore）
```

```php
// 单任务级别启用持久化
TaskBuilder::shell('echo durable')
    ->cron('*/1 * * * *')
    ->persist(true)        // 该任务持久化
    ->dispatch();
```

### Crash Recovery（acks_late）

`acks_late=true` 的任务采用"完成后才确认"语义：

1. 任务派发后进入 `Pending` 状态。
2. 被调度执行时进入 `Running` 状态，**此时未 ack**。
3. 任务成功完成后才 ack，状态转为 `Success`。
4. 若 daemon 在任务 `Running` 期间崩溃 / 被强制重启，daemon 重启后会扫描持久化存储：
   - `acks_late=true` 且状态为 `Running` 的任务会被**自动重置为 `Pending`** 并重新触发。
   - 普通任务（`acks_late=false`）的 `Running` 状态保持，不会重投（视为已确认）。

适用场景：

- 不希望因 daemon 重启而丢失正在执行的任务。
- 任务对幂等性有保证，重复执行不会产生副作用。
- 关键业务任务（数据同步、订单处理、报表生成）。

```php
TaskBuilder::shell('php /app/bin/sync-orders.php')
    ->cron('*/10 * * * *')
    ->persist(true)         // 持久化任务状态
    ->acksLate(true)        // 完成后才 ack，crash 后重投
    ->acksOnFailure(false)  // 失败时不 ack，便于重试
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->dispatch();
```

### 持久化测试

`test_xhjob_persist.php` 覆盖以下语义：

1. 任务在 daemon 重启后仍存在，`execution_count` 不丢失。
2. 循环任务在重启后继续触发。
3. `acks_late=true` 的 `Running` 任务在重启后被重新触发（crash recovery）。
4. SQLite db 文件持久存储数据。

---

## 测试套件

集成包提供 4 套测试脚本，覆盖深度功能、持久化、异步队列与池模式对比。

### 测试脚本一览

| 脚本 | 步数 | 覆盖范围 |
|---|---|---|
| `test_xhjob_deep.php` | 29 步 | 深度功能测试，覆盖全部任务参数属性：循环任务、maxExecutions、withRetry、allowOverlap、timeout/softTimeout/expires/jitter、retry_backoff/ignore_result/acks_late、misfire_grace_time/timezone/tags/meta、result_ttl/rate_limit/coalesce、start_date/end_date/persist、withId+replaceExisting、priority/chain/group。 |
| `test_xhjob_persist.php` | 8 步 | 持久化深度测试：daemon 重启后任务恢复、execution_count 不丢失、循环任务继续触发、acks_late Running 任务重投、SQLite 文件持久存储。 |
| `test_xhjob_async_queue.php` | 13 步 | 后台任务队列异步测试（对标 APScheduler/Celery）：非阻塞派发、批量并发、进度上报、countdown 延迟、chord 全部成功 / 部分失败、inspect active/registered/scheduled/stats、pull_events。 |
| `test_xhjob_pool_mode.php` | — | 两种池模式功能对比测试：async 与 thread 模式下的功能行为一致性验证（各 10 步）。 |
| `test_xhjob_pool_diff.php` | — | 两种池模式真实差异验证：通过 `/proc/{pid}/task` 读取线程名 + 时间戳分析，证明 async（M:N）与 thread（1:1）的并发模型差异。 |

### 运行命令

```bash
# 假设 .so 路径
EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so

# 1. 深度功能测试（29 步）
php -d extension=$EXT test_xhjob_deep.php

# 2. 持久化测试（8 步）
php -d extension=$EXT test_xhjob_persist.php

# 3. 异步队列测试（13 步）
php -d extension=$EXT test_xhjob_async_queue.php

# 4. 池模式功能对比测试
XHJOB_POOL_MODE=async   php -d extension=$EXT test_xhjob_pool_mode.php
XHJOB_POOL_MODE=thread  php -d extension=$EXT test_xhjob_pool_mode.php

# 5. 池模式真实差异验证（线程名 + 并发时间戳分析）
php -d extension=$EXT test_xhjob_pool_diff.php

# 基础 CLI 测试（12 步，daemon 生命周期 + CRUD）
php -d extension=$EXT test_xhjob.php
```

### 测试结果解读

每个脚本以 `[N] 描述 ... PASS/FAIL` 形式逐行输出，末尾汇总 `PASS=N FAIL=M`。失败步骤会打印异常消息。所有测试脚本均使用独立 service 名（如 `deep-test`）与独立 data_dir（如 `/tmp/xhjob-deep-test`），互不污染。

---

## APScheduler / Celery 对标

xhjob 在单机 PHP 场景下与 Python 生态两套成熟调度方案的能力对标详见 [EVALUATION.md](./EVALUATION.md)。简要结论：

| 对照系 | 总项数 | 已实现/等价 | 部分实现 | 不适用 | 未实现 |
|---|---|---|---|---|---|
| APScheduler（单机调度器） | 25 | 21 | 1 | 3 | 0 |
| Celery（分布式队列） | 35 | 22 | 8 | 5 | 0 |

核心结论：

- **APScheduler 维度**：单机调度核心语义对齐度约 84%，未实现项均为分布式专属存储（Mongo/Redis）或进程池，单机不需要。
- **Celery 维度**：单机任务/编排/重试/超时/事件主线对齐度约 63%；剔除"不适用"的分布式项后对齐度约 73%。
- **标志性补齐项**：chord（group + 回调）/ countdown（相对延迟）/ progress（进度上报）/ inspect（运行时检查）/ 事件订阅。
- **明确边界**：不追求分布式能力，主动规避多 Broker / 多 worker / 多 backend / Flower 等分布式专属特性，避免复杂度膨胀。

详细特性矩阵、单机场景"不值得实现"清单、下一步优化方向请参阅 [EVALUATION.md](./EVALUATION.md)。

---

## 目录结构

```
xhjob-thinkphp8-extend/
├── Xhjob/                              # extend 第三方库（复制到 tp/extend/Xhjob/）
│   ├── Exception/                      # 异常体系
│   │   ├── InvalidTaskConfigException.php
│   │   ├── ServiceNotRunningException.php
│   │   ├── TaskNotFoundException.php
│   │   └── XhjobException.php
│   ├── facade/
│   │   └── Xhjob.php                   # ThinkPHP Facade（绑定 xhjob.manager）
│   ├── Client.php                      # 跨环境客户端（retry / setTimeout 容错增强）
│   ├── ServiceProvider.php             # ThinkPHP 服务提供者（注册 service / manager）
│   ├── TaskBuilder.php                 # PHP fluent builder（38+ 链式方法）
│   ├── TaskManager.php                 # 任务管理门面（create/state/result/inspect...）
│   ├── XhjobService.php                # daemon 生命周期管理（start/stop/restart/wait）
│   └── helper.php                      # 全局辅助函数（xhjob_task/manager/service）
├── controller/
│   └── XhjobTask.php                   # 控制器（25 个 action，复制到 tp/app/controller/）
├── route/
│   └── app.php                         # 路由（合并到 tp/route/app.php）
├── config/
│   └── xhjob.php                       # 配置（复制到 tp/config/）
├── service.php                         # ServiceProvider 注册（合并到 tp/app/service.php）
├── test_xhjob.php                      # 基础 CLI 测试（12 步：daemon 生命周期 + CRUD）
├── test_xhjob_deep.php                 # 深度功能测试（29 步：全部任务参数属性）
├── test_xhjob_persist.php              # 持久化测试（8 步：daemon 重启恢复 + acks_late）
├── test_xhjob_async_queue.php          # 异步队列测试（13 步：chord/inspect/events/progress）
├── EVALUATION.md                       # APScheduler / Celery 对标评估
└── README.md                           # 本文件
```

### HTTP 路由（/xhjob 前缀）

| Method | Path | 说明 |
|---|---|---|
| GET | `/xhjob/index` | 首页 / 状态 |
| GET | `/xhjob/status` | daemon 状态 |
| GET | `/xhjob/health` | 健康检查 |
| POST | `/xhjob/start` | 启动 daemon |
| POST | `/xhjob/stop` | 停止 daemon / 任务（带 id 时操作单任务） |
| POST | `/xhjob/restart` | 重启 daemon / 任务（带 id 时操作单任务） |
| GET | `/xhjob/list` | 任务列表 |
| GET | `/xhjob/get` | 任务详情 |
| POST | `/xhjob/create` | 创建任务（raw JSON） |
| POST | `/xhjob/createShell` | 创建 shell 任务 |
| POST | `/xhjob/createHttp` | 创建 HTTP 任务 |
| POST | `/xhjob/createCron` | 创建 cron 任务 |
| POST | `/xhjob/createChain` | 创建任务链 |
| POST | `/xhjob/createGroup` | 创建任务组 |
| GET | `/xhjob/state` | 任务状态 |
| GET | `/xhjob/result` | 任务结果 |
| GET | `/xhjob/logs` | 任务日志 |
| POST | `/xhjob/update` | 编辑任务 |
| POST | `/xhjob/pause` | 暂停任务 |
| POST | `/xhjob/resume` | 恢复任务 |
| POST | `/xhjob/reschedule` | 重新调度 |
| DELETE | `/xhjob/delete` | 删除任务 |
| GET | `/xhjob/chainState` | 链状态 |
| GET | `/xhjob/groupState` | 组状态 |
| GET | `/xhjob/demo` | 完整演示 |
