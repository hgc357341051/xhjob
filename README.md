# xhjob

XHJob 是一个基于 Rust（ext-php-rs 0.15）开发的高性能 PHP 异步任务调度扩展，为 PHP-FPM 应用提供 **master-worker 多进程架构**（参考 PHP-FPM），独立的 daemon 守护进程在 PHP 请求结束后继续运行，无需任何外部依赖（不依赖 Redis / RabbitMQ / Swoole / Supervisor / crontab）。

支持 **两种任务执行池模式**：
- **async 模式（默认）**：tokio M:N 调度，N 个 tokio worker 线程复用跑 M 个 async task，适合 IO 密集型
- **thread 模式**：1:1 OS 线程，每个任务独占线程，适合 CPU 密集型或需严格隔离

## 特性

- 跨平台 daemon（Unix `double-fork` + Windows `CreateProcessW`），PHP 退出后任务继续运行
- 两种任务执行池：async（tokio M:N）与 thread（1:1 OS 线程）
- 链式 API：`TaskBuilder::shell('echo hi')->cron('*/5 * * * *')->dispatch()`
- Cron 调度（5/6 段表达式）、IntervalTrigger、DateTrigger、一次性 runAt
- HTTP 与 Shell 双执行器，支持超时、重试、指数退避、软超时
- 任务编排：chain（顺序流水线）、group（并行批处理）、chord（header+body 回调）
- 可选 SQLite 持久化（`persist` cargo feature），daemon 重启后自动恢复任务
- 可观测性：事件流、任务列表、inspect 聚合查询、进度上报
- 多服务实例、HTTP/SOCKS5 代理、Shell 编码转换、Cron 自定义时区

## 安装

### 依赖

- Rust 工具链（推荐 stable，需支持 ext-php-rs 0.15）
- PHP 8.x 开发包（`php-config` 在 PATH 中）
- 编译时需要 `libclang`（bindgen 依赖）

### 编译

```bash
# 默认编译（仅内存存储）
cargo build --release

# 启用 SQLite 持久化
cargo build --release --features persist
```

产物：

- Unix：`target/release/libxhjob.so`
- Windows：`target/release/xhjob.dll`

### 加载扩展

在 `php.ini` 中添加：

```ini
extension=/path/to/libxhjob.so
```

或运行时通过 `-d extension=target/release/libxhjob.so` 加载。

### 确认加载的 .so 版本（重要）

PHP-FPM 替换 `.so` 后**必须重启 FPM**，否则 worker 进程仍持有旧 mmap 的 `.so`。可通过 `xhjob_diag()` 返回的 JSON 字段确认实际加载的版本：

```php
$diag = json_decode(xhjob_diag('your-service', '/tmp/xhjob-your-service'), true);

// 1. 检查 xhjob_so_info 字段是否存在
//    - 若不存在 → 加载的是 v2 之前的旧 .so，需重新安装 + 重启 FPM
if (!isset($diag['xhjob_so_info'])) {
    echo "加载的是旧版 .so，请重新安装并重启 FPM\n";
} else {
    $so = $diag['xhjob_so_info'];
    echo "实际加载的 .so 路径: " . ($so['path'] ?? '(null)') . "\n";
    echo "文件大小: " . ($so['size_bytes'] ?? 0) . " bytes\n";
    echo "mtime: " . date('Y-m-d H:i:s', $so['mtime_epoch'] ?? 0) . "\n";
    // 比对 releases/xhjob-php8.2-linux-x86_64.so 的 size + mtime
}

// 2. 检查 php_binary_raw 字段是否存在
//    - 若不存在 → 同样是旧版 .so
//    - 若存在但 == php_binary → CLI 解析未触发替换（可能本就是 CLI 上下文）
//    - 若存在且 != php_binary → FPM 上下文已正确替换为 CLI php
echo "php_binary (resolved): " . $diag['php_binary'] . "\n";
echo "php_binary_raw (current_exe): " . ($diag['php_binary_raw'] ?? '(字段不存在→旧版.so)') . "\n";

// 3. 检查 php_binary_candidates 字段（v2 新增）
//    列出所有探测过的 CLI php 候选 + 失败原因，便于定位为什么 resolved 是某路径
if (isset($diag['php_binary_candidates'])) {
    foreach ($diag['php_binary_candidates'] as $c) {
        echo "  candidate: {$c['path']} valid=" . ($c['valid'] ? 'true' : 'false') . " reason={$c['reason']}\n";
    }
}
```

**如果 `xhjob_so_info` 或 `php_binary_raw` 字段不存在**，说明 PHP-FPM 加载的是 v2 之前的旧 `.so`。修复步骤：

```bash
# 1. 把新 .so 复制到 PHP 扩展目录（用 php-config --extension-dir 查路径）
cp releases/xhjob-php8.2-linux-x86_64.so $(php-config --extension-dir)/xhjob.so

# 2. 重启 PHP-FPM（宝塔环境）
/etc/init.d/php-fpm-82 reload
# 或 systemctl restart php-fpm

# 3. 验证
php -r 'echo xhjob_diag("test", "/tmp/xhjob-test");' | jq .xhjob_so_info
```

## 两种任务执行池模式

### 术语澄清

> Rust **没有语言级"协程"**——只有 `async/await` + `Future`（编译期转状态机）由 runtime（tokio）poll 驱动。这与 Go goroutine / Python coroutine 的"协程"概念不同。
>
> `coroutine` 作为 `async` 的兼容别名保留（等价），新代码建议使用 `async`。

### 模式一：async（默认，M:N 调度）

- **底层实现**：tokio multi-thread async runtime，任务以 `Future` 形式 `tokio::spawn` 到共享调度器
- **线程**：N 个 tokio worker 线程（N = CPU 核数，线程名 `xhjob-tokio`）
- **并发模型**：M 个 async task 复用 N 个线程，task 在 `.await` 让出点主动切换
- **资源占用**：单 task 状态机按需分配（几 KB），worker 线程数固定
- **最大并发**：默认 `1024`，可通过 `XHJOB_ASYNC_POOL_SIZE` 覆盖（兼容别名 `XHJOB_COROUTINE_POOL_SIZE`）
- **启用方式**：`XHJOB_POOL_MODE=async`（或不设置，默认值）

### 模式二：thread（1:1 调度）

- **底层实现**：`std::thread` + `crossbeam-channel`，任务通过 `block_on` 在工作线程中执行
- **线程**：固定数量的 worker 线程（线程名 `xhjob-worker-N`），默认 = CPU 核数
- **并发模型**：每个任务独占一个 OS 线程，`block_on` 阻塞整个线程直到任务完成
- **资源占用**：单任务 ~2-8 MB（OS 线程栈）
- **线程数**：默认 CPU 核数，可通过 `XHJOB_THREAD_POOL_SIZE` 覆盖
- **启用方式**：`XHJOB_POOL_MODE=thread`

### 真实差异验证（4×sleep(3) 基准）

测试环境：2 CPU 核数，4 个 `sleep 3` 并发任务。完整脚本见 `tp/test_xhjob_pool_diff.php`。

| 维度 | async 模式 | thread 模式 |
|------|-----------|-------------|
| 执行线程名 | `xhjob-tokio`（2 个） | `xhjob-worker-0`, `xhjob-worker-1`（2 个） |
| 4 个任务时间差（delta） | **0~304ms**（全部同时） | **0~3105ms**（前 2 个立即，后 2 个等 3s） |
| 总耗时 | **3.61s**（≈ 1 × sleep 时间） | **6.31s**（≈ 2 × sleep 时间，分 2 批） |
| 并发模型 | M:N（2 线程跑 4 任务） | 1:1（2 线程跑 2 任务，另 2 排队） |
| 适用场景 | IO 密集型（HTTP / shell 等待） | CPU 密集型 / 强隔离 / 严格并发控制 |

**本质区别**：async 模式在 `sleep` 时 `await` 让出线程，4 个任务在 2 个 tokio worker 上全部并发完成；thread 模式每个任务通过 `block_on` 独占线程，受限于 2 个 worker，必须分 2 批执行。

### 完整对比表

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

### 架构图

```
            ┌──────────────────────────────────────────────────────┐
            │                    xhjob daemon                      │
            │  scan_once (cron/interval/date triggers)             │
            │     │                                                │
            │     ▼                                                │
            │  ┌─────────────┐  ┌─────────────┐  ┌────────────────┐│
            │  │ TaskQueue   │  │ OverlapCtrl │  │ Dispatch Pool  ││
            │  │ (priority)  │→ │ (max_inst / │→ │  async | thread││
            │  │             │  │  coalesce)  │  └─────────┬──────┘│
            │  └─────────────┘  └─────────────┘            │       │
            │                                               │       │
            │              ┌────────────────────────────────┴───┐   │
            │              ▼                                     ▼   │
            │   ┌──────────────────────────┐         ┌─────────────────┐
            │   │ async 模式（M:N）       │         │ thread 模式(1:1)│
            │   │ ┌──────────────────────┐│         │ ┌──────────────┐│
            │   │ │ tokio runtime        ││         │ │ worker thr 1 ││
            │   │ │  ├── worker A        ││         │ │  block_on() ││
            │   │ │  │   ├── task₁.await ││         │ ├── worker 2  ││
            │   │ │  │   ├── task₂.await ││         │ │  block_on() ││
            │   │ │  │   └── task₃.await ││         │ └── ...       ││
            │   │ │  └── worker B        ││         └──────────────┘│
            │   │ └──────────────────────┘│                           │
            │   └──────────────────────────┘                           │
            └──────────────────────────────────────────────────────────┘
```

## ThinkPHP 8 集成

`releases/xhjob-thinkphp8-extend/` 提供开箱即用的 ThinkPHP 8 集成包，封装为 ServiceProvider + Facade + Builder，集成方式如下。

### 目录结构

```
xhjob-thinkphp8-extend/
├── Xhjob/
│   ├── ServiceProvider.php      # ThinkPHP 服务提供者
│   ├── facade/Xhjob.php         # 静态代理（\Xhjob\facade\Xhjob）
│   ├── TaskManager.php          # 任务管理门面（create/state/result/waitForState...）
│   ├── TaskBuilder.php          # 链式构建器（shell/http/chain/group/chord）
│   ├── XhjobService.php         # daemon 生命周期（start/stop/status/ensureRunning）
│   ├── Exception/               # 业务异常
│   └── helper.php               # 全局辅助函数（xhjob_task / xhjob_manager）
├── config/xhjob.php             # 配置文件模板
└── test_xhjob_*.php             # 测试脚本
```

### 第 1 步：安装扩展与集成包

```bash
# 1. 编译 .so
cd /path/to/xhjob
cargo build --release --features persist

# 2. 安装到 PHP 扩展目录
cp target/release/libxhjob.so $(php-config --extension-dir)/xhjob.so

# 3. 在 php.ini 中启用
echo "extension=xhjob.so" >> $(php --ini | grep "Loaded Configuration" | awk '{print $4}')

# 4. 将集成包复制到 ThinkPHP 项目
cp -r /path/to/xhjob/releases/xhjob-thinkphp8-extend/Xhjob  /path/to/your-tp/extend/
cp    /path/to/xhjob/releases/xhjob-thinkphp8-extend/config/xhjob.php /path/to/your-tp/config/
```

### 第 2 步：注册 ServiceProvider

在 `app/service.php` 中注册：

```php
<?php
// app/service.php
return [
    \app\service\SomeService::class,
    \Xhjob\ServiceProvider::class,  // 添加这一行
];
```

### 第 3 步：配置

编辑 `config/xhjob.php`：

```php
return [
    'service_name' => env('XHJOB_SERVICE', 'default'),
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    'api_token'    => env('XHJOB_API_TOKEN', null),
    // 任务执行池模式：'async'（默认）或 'thread'（'coroutine' 为 'async' 兼容别名）
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

对应 `.env`：

```ini
# 默认 async 模式（IO 密集型推荐）
XHJOB_POOL_MODE=async
XHJOB_ASYNC_POOL_SIZE=1024

# 或切换为 thread 模式（CPU 密集型推荐）
# XHJOB_POOL_MODE=thread
# XHJOB_THREAD_POOL_SIZE=4

# 持久化（daemon 重启后任务恢复）
XHJOB_PERSIST=1
XHJOB_DATA_DIR=/var/lib/xhjob
```

### 第 4 步：启动 daemon

daemon 通常在 PHP-FPM 第一次 dispatch 任务时自动启动；也可显式预热：

```php
<?php
// 在 CLI 启动脚本或 controller 中
use Xhjob\facade\Xhjob;

// 启动 daemon（已运行则 no-op）
app('xhjob.service')->ensureRunning();

// 查询状态
$status = app('xhjob.service')->status();
// ['running' => true, 'pid' => 12345]
```

### 第 5 步：在 Controller / 命令中派发任务

```php
<?php
namespace app\controller;

use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

class Report
{
    public function schedule()
    {
        // 一次性 shell 任务
        $id = Xhjob::create(
            TaskBuilder::shell('tar czf /tmp/backup.tgz /var/data')
                ->withRetry(3, 5)
                ->timeout(120)
        );

        // cron 周期任务（每 5 分钟）
        $cronId = Xhjob::create(
            TaskBuilder::http('GET', 'https://api.example.com/health')
                ->cron('*/5 * * * *')
                ->withTimezone('Asia/Shanghai')
                ->persist(true)
                ->tag('monitor')
        );

        // chain 流水线
        $chainId = Xhjob::createChain([
            TaskBuilder::shell('curl -s https://api.example.com/raw > /tmp/raw.json'),
            TaskBuilder::shell('jq ".items" /tmp/raw.json > /tmp/clean.json'),
            TaskBuilder::shell('aws s3 cp /tmp/clean.json s3://bucket/'),
        ]);

        // 等待结果（最多 60 秒）
        $result = Xhjob::waitForResult($id, 60);

        return json([
            'task_id'   => $id,
            'cron_id'   => $cronId,
            'chain_id'  => $chainId,
            'result'    => $result,
        ]);
    }

    public function query(string $id)
    {
        return json([
            'state'  => Xhjob::state($id),
            'result' => Xhjob::result($id),
        ]);
    }
}
```

### 全局辅助函数（可选）

在 `composer.json` 中注册 `helper.php`：

```json
{
    "autoload": {
        "files": ["extend/Xhjob/helper.php"]
    }
}
```

然后执行 `composer dump-autoload`，即可使用：

```php
<?php
// 快捷派发 shell 任务
$id = xhjob_task('echo hello');

// 获取 TaskManager 实例
$mgr = xhjob_manager();
```

### ThinkPHP 8 完整使用示例

```php
<?php
// 启动 daemon + 派发 + 等待 + 查询 完整流程
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

// 1. 启动 daemon（若未运行）
app('xhjob.service')->ensureRunning();

// 2. 派发一个 async 模式下高并发的 HTTP 任务组
$ids = [];
for ($i = 0; $i < 50; $i++) {
    $ids[] = Xhjob::create(
        TaskBuilder::http('GET', "https://api.example.com/ping?n={$i}")
            ->withRetry(2, 1)
            ->ignoreResult(true)
    );
}

// 3. async 模式下 50 个任务会在 2 个 tokio worker 上全部并发
//    thread 模式下则受限于 XHJOB_THREAD_POOL_SIZE 线程数

// 4. 查询任务状态
foreach ($ids as $id) {
    $state = Xhjob::state($id);
    echo "{$id}: {$state['state']}\n";
}

// 5. inspect 聚合查询
$active = Xhjob::list('RUNNING');
echo "running tasks: " . count($active) . "\n";
```

## 基础用法（不使用 ThinkPHP）

```php
<?php
// 1. 启动 daemon
xhjob_start();

// 2. 链式 API 派发 HTTP 任务
$id = Xhjob::task()
    ->viaHttp('POST', 'https://httpbin.org/post')
    ->withHeaders(['X-Foo' => 'bar'])
    ->withBody(json_encode(['k' => 'v']))
    ->withRetry(3, 2)
    ->timeout(30)
    ->dispatch();

// 3. 查询状态与结果
$state  = xhjob_state($id);
$result = xhjob_result($id);
var_dump($state, $result);

// 4. 关闭 daemon
xhjob_stop();
```

## 任务编排

### chain（顺序流水线）

```php
<?php
// ETL：extract → transform → load，任一步失败则中断
$chainId = xhjob_chain(json_encode([
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s https://api.example.com/raw > /tmp/raw.json']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'jq ".items" /tmp/raw.json > /tmp/clean.json']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'aws s3 cp /tmp/clean.json s3://bucket/']],
]));

// 轮询状态
$state = json_decode(xhjob_chain_state($chainId), true);
// ['state' => 'running', 'current_step' => 1, 'tasks' => [...]]
```

### group（并行批处理）

```php
<?php
// 3 个任务并行执行
$groupId = xhjob_group(json_encode([
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/a.jpg https://example.com/a.jpg']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/b.jpg https://example.com/b.jpg']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/c.jpg https://example.com/c.jpg']],
]));

$state = json_decode(xhjob_group_state($groupId), true);
// ['state' => 'running', 'summary' => ['total' => 3, 'succeeded' => 2, 'failed' => 0, 'pending' => 1]]
```

### chord（header 并行 + body 回调）

```php
<?php
// 3 个 header 任务全部成功后触发 callback
$chordId = xhjob_chord(
    json_encode([
        ['task_type' => 'shell', 'payload' => ['cmd' => 'compute-a']],
        ['task_type' => 'shell', 'payload' => ['cmd' => 'compute-b']],
        ['task_type' => 'shell', 'payload' => ['cmd' => 'compute-c']],
    ]),
    json_encode(['task_type' => 'shell', 'payload' => ['cmd' => 'aggregate-results']])
);

$state = json_decode(xhjob_chord_state($chordId), true);
// header 任一失败 → state='partial_failed'，不触发 callback
```

### countdown（延迟派发）

```php
<?php
// 5 秒后触发一次（等价 Celery apply_async(countdown=5)）
$id = xhjob_countdown(
    json_encode(['task_type' => 'shell', 'payload' => ['cmd' => 'echo delayed']]),
    5
);
```

## API 参考

### 顶层函数

| 函数 | 说明 |
|------|------|
| `xhjob_start($name="default", $data_dir=null): bool` | 启动（或确认已启动）指定服务的 daemon |
| `xhjob_stop($name="default", $data_dir=null): bool` | 停止 daemon |
| `xhjob_restart($name="default", $data_dir=null): bool` | 重启 daemon |
| `xhjob_status($name="default", $data_dir=null): array` | 查询 daemon 运行状态（`running`、`pid`） |
| `xhjob_dispatch($task_json, $name="default", $data_dir=null): string` | dispatch 任务，返回 task_id 或 `error: <msg>` |
| `xhjob_state($id, $name="default", $data_dir=null): array` | 查询任务状态 |
| `xhjob_result($id, $name="default", $data_dir=null): array` | 查询任务结果（body/status_code/stdout/stderr/exit_code） |
| `xhjob_get($id, $name="default", $data_dir=null): ?string` | 查询单任务完整 Task JSON |
| `xhjob_list($name="default", $state_filter=null, $data_dir=null): string` | 列出任务摘要（JSON 字符串） |
| `xhjob_pause($id, ...) / xhjob_resume($id, ...)` | 暂停 / 恢复 cron 作业 |
| `xhjob_cancel($id, ...)` | 取消任务（Pending→Cancelled；Running→停止后续重试与触发） |
| `xhjob_remove($id, ...)` | 删除任务定义（不影响运行中实例） |
| `xhjob_requeue($id, ...)` | 把终态任务重新置为 PENDING 重新调度 |
| `xhjob_reschedule($id, $cron, ...)` | 在线修改 cron 表达式，保留 state/execution_count |
| `xhjob_events($since_ts, $task_id=null, ...)` | 查询任务执行事件流 JSON |
| `xhjob_chain($tasks_json, ...)` | 创建顺序流水线（任一步失败则中断） |
| `xhjob_chain_state($chain_id, ...)` | 查询 chain 状态 |
| `xhjob_group($tasks_json, ...)` | 创建并行批处理 |
| `xhjob_group_state($group_id, ...)` | 查询 group 状态 + summary |
| `xhjob_chord($header_json, $callback_json, ...)` | 创建 chord（header 并行 + body 回调） |
| `xhjob_chord_state($chord_id, ...)` | 查询 chord 状态 |
| `xhjob_countdown($task_json, $secs, ...)` | 延迟 `$secs` 秒后派发一次性任务 |
| `xhjob_report_progress($id, $progress, $meta=null, ...)` | 上报任务进度（0-100） |
| `xhjob_pull_events($since_ts, ...)` | 拉取事件流（短轮询） |
| `xhjob_inspect($mode="stats", ...)` | 聚合查询：active/registered/scheduled/stats |

### Xhjob 类（链式 API，Rust 扩展内置）

| 方法 | 说明 |
|------|------|
| `Xhjob::task(): Xhjob` | 创建新 builder |
| `viaHttp($method, $url) / viaShell($cmd)` | 设置任务类型 |
| `withHeaders / withBody / withProxy / withEncoding` | HTTP / Shell 配置 |
| `withTimezone($tz)` | Cron 时区 |
| `withRetry($max, $delay) / retryBackoff(bool)` | 重试 + 指数退避 |
| `cron($expr) / every($secs) / runAt($ts)` | 调度方式（优先级 runAt > cron > every） |
| `timeout($secs) / softTimeout($secs)` | 硬 / 软超时 |
| `priority($p)` | 优先级（越大越先执行） |
| `allowOverlap / maxInstances / coalesce` | 重叠 / 并发 / misfire 控制 |
| `persist(bool)` | SQLite 持久化 |
| `maxExecutions($n)` | 最大执行次数（0=无限） |
| `startAt($ts) / endAt($ts)` | 起始 / 结束时间窗口 |
| `resultTtl($secs) / ignoreResult(bool)` | 结果保留 / fire-and-forget |
| `withMeta($json) / tag($tag)` | 元数据 / 标签 |
| `jitter($secs) / expires($secs)` | 随机抖动 / Pending 超时 |
| `acksLate(bool) / acksOnFailure(bool)` | 崩溃恢复 / 失败不放弃 |
| `misfireGraceTime($secs)` | per-job misfire 宽限窗口 |
| `id($id) / replaceExisting(bool)` | 幂等 dispatch |
| `rateLimit($count, $windowSecs)` | 滑动窗口限流 |
| `dispatch(): string` | 提交任务，返回 task_id |

### ThinkPHP 集成包类

| 类 | 说明 |
|---|---|
| `\Xhjob\ServiceProvider` | ThinkPHP 服务提供者，注册到 `app/service.php` |
| `\Xhjob\facade\Xhjob` | 静态代理（`Xhjob::create() / state() / list()`） |
| `\Xhjob\TaskManager` | 任务管理门面（容器标识 `xhjob.manager`） |
| `\Xhjob\TaskBuilder` | 链式构建器（`shell() / http() / chain() / group() / chord()`） |
| `\Xhjob\XhjobService` | daemon 生命周期（容器标识 `xhjob.service`） |

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `XHJOB_POOL_MODE` | `async` | 任务执行池模式：`async`（默认）或 `thread`；`coroutine` 为 `async` 兼容别名 |
| `XHJOB_ASYNC_POOL_SIZE` | `1024` | async task 池模式下的最大并发任务数（兼容别名 `XHJOB_COROUTINE_POOL_SIZE`） |
| `XHJOB_THREAD_POOL_SIZE` | `num_cpus` | thread 模式下的工作线程数 |
| `XHJOB_DATA_DIR` | 平台默认 | 统一数据目录（PID/sock/db/log 同时落入此目录） |
| `XHJOB_PID_DIR` / `XHJOB_SOCK_DIR` / `XHJOB_DB_DIR` / `XHJOB_LOG_DIR` | `/tmp` | 细粒度目录覆盖（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_PERSIST` | `0` | 设为 `1` 启用 SQLite 持久化 |
| `XHJOB_MAX_TASKS_PER_CHILD` | `0` | daemon 累计执行 N 次任务后自我退出；0=不限 |
| `XHJOB_MAX_MEMORY_PER_CHILD` | `0` | daemon RSS 超过 N 字节后自我退出；0=不限 |
| `XHJOB_SHELL_TIMEOUT` | `300` | Shell 任务默认超时（秒） |

## 测试

```bash
# 编译
cargo build --release --features persist

# Rust 单元测试
cargo test --features persist

# PHP 集成测试
EXT=target/release/libxhjob.so
php -d extension=$EXT tests/run-tests.php tests/

# 两种池模式功能对比测试（各 10 步）
XHJOB_POOL_MODE=async   php -d extension=$EXT tp/test_xhjob_pool_mode.php
XHJOB_POOL_MODE=thread  php -d extension=$EXT tp/test_xhjob_pool_mode.php

# 两种池模式真实差异验证（线程名 + 并发时间戳分析）
php -d extension=$EXT tp/test_xhjob_pool_diff.php

# 异步队列功能测试（chord/countdown/inspect/pull_events）
php -d extension=$EXT releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php

# 持久化测试（daemon 重启后状态恢复）
php -d extension=$EXT tp/test_xhjob_persist.php
```

## TaskState 枚举

| 状态 | 说明 | 是否终态 |
|------|------|----------|
| `PENDING` | 已入队等待执行 | 否 |
| `RUNNING` | 正在执行 | 否 |
| `INTERRUPTED` | daemon 异常退出时被中断（可恢复） | 否 |
| `SUCCESS` | 执行成功 | 是 |
| `FAILED` | 执行失败（重试耗尽） | 是 |
| `CANCELLED` | 被 `xhjob_cancel` 取消 | 是 |
| `EXPIRED` | Pending 任务超过 `expires` 上限 | 是 |

## 许可证

参见 [LICENSE](LICENSE)。
