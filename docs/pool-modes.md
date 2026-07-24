---
title: 双线程池模式对比
parent: 进阶
nav_order: 41
---

# 双线程池模式对比

xhjob 内核提供两种**可切换的任务执行池模式**——`async`（默认）与 `thread`，针对不同负载特征做出取舍。模式选择在 daemon 启动时确定，运行期间不可热切换。

{: .warning }
> `coroutine` 是 `async` 的**兼容别名**，二者完全等价。Rust 没有语言级"协程"——只有 `async/await` + `Future`（编译期转为状态机）由 tokio runtime poll 驱动，与 Go goroutine / Python coroutine 的"协程"概念不同。建议新代码统一使用 `async`。

## 配置项

模式通过环境变量 `XHJOB_POOL_MODE` 选择，daemon 启动时读取一次。

| 环境变量 | 可选值 | 默认 | 说明 |
|------|------|------|------|
| `XHJOB_POOL_MODE` | `async` / `thread` / `coroutine` | `async` | 任务执行池模式。`coroutine` 为 `async` 的兼容别名 |
| `XHJOB_ASYNC_POOL_SIZE` | 正整数 | `1024` | async 模式最大并发任务数。兼容别名：`XHJOB_COROUTINE_POOL_SIZE`。向上调优需关注文件描述符上限（`ulimit -n`） |
| `XHJOB_THREAD_POOL_SIZE` | 正整数 | CPU 核数（`num_cpus`） | thread 模式工作线程数，即并发上限 |

## 读取时机与切换方式

`XHJOB_POOL_MODE` 在 daemon 的任务派发路径中读取（读取点位于 `src/scheduler/queue.rs` 约 779 行）：

```rust
// Pool mode selection: async (default) or thread.
// XHJOB_POOL_MODE=thread → ThreadPool (1:1 OS thread, std::thread + block_on)
// XHJOB_POOL_MODE=async (default) or legacy alias `coroutine`
//   → async task pool (M:N tokio scheduling, max 1024)
let pool_mode = std::env::var("XHJOB_POOL_MODE").unwrap_or_else(|_| "async".to_string());
```

由于 daemon 是从 PHP 进程经 double-fork 拉起的独立 Rust 进程，它**继承 PHP 进程在 `xhjob_start` 调用时的环境快照**。因此：

- daemon 一旦启动，其 `XHJOB_POOL_MODE` 即被固定，运行期间修改 `putenv` 不影响已运行的 daemon。
- 切换模式必须先 `xhjob_stop` 停掉当前 daemon，再以新的 `XHJOB_POOL_MODE` 重新 `xhjob_start`。

```php
<?php
// 切换到 thread 模式：先停再启
$svc = new \Xhjob\XhjobService('default', '/var/lib/xhjob');
$svc->ensureStopped();          // 停掉旧 daemon

putenv('XHJOB_POOL_MODE=thread');          // 切换为 1:1 线程池
putenv('XHJOB_THREAD_POOL_SIZE=8');        // 8 个工作线程
$pid = $svc->start();                      // 以新环境拉起 daemon，返回 PID
echo "daemon pid={$pid}, pool_mode=thread\n";
```

## 实现差异

| 维度 | async（默认） | thread |
|------|--------------|--------|
| 底层实现 | tokio 多线程异步运行时 | `std::thread` + `crossbeam-channel` |
| 调度模型 | M:N（N 个 tokio worker 线程复用 M 个异步任务，`.await` 让出） | 1:1（每个任务独占一个 worker 线程，`block_on`） |
| worker 线程名 | `xhjob-tokio` | `xhjob-worker-N` |
| 默认并发上限 | 1024 | CPU 核数 |
| 每任务内存 | 几 KB（task 状态机按需分配） | 2-8 MB（OS 线程栈） |
| IO 密集表现 | 优秀（await 让出，高吞吐） | 一般（线程阻塞在 IO 上被占住） |
| CPU 密集表现 | 一般（少量 tokio 线程易被打满） | 优秀（每任务独占线程，OS 调度公平） |
| 任务隔离性 | 弱（长 CPU 任务会饿死其他任务） | 强（独立线程，互不抢占） |
| 上下文切换成本 | 低（用户态切换） | 较高（内核态线程切换） |

派发路径的分发逻辑（`src/scheduler/queue.rs`）：

```rust
if pool_mode == "thread" {
    // 1:1 线程池：每任务提交到一个 OS 线程，block_on 执行其 async future
    crate::pool::thread_pool::global().submit(move || {
        if let Some(rt) = coroutine_pool::global_runtime() {
            rt.block_on(counted_future);
        }
    });
} else {
    // async（推荐）与 coroutine（兼容别名）都走这里：tokio M:N 调度
    drop(coroutine_pool::global().spawn(counted_future));
}
```

## 性能特征对比

| 性能维度 | async（默认） | thread |
|------|--------------|--------|
| 高并发短任务吞吐 | 高（1024 并发，await 让出复用线程） | 受限于线程数（默认 = CPU 核数） |
| 长时 IO 等待（HTTP / DB） | 友好（等待时不占线程） | 占住一个线程直到返回 |
| CPU 计算 / 压缩 / 加解密 | 易打满少量 tokio 线程，拖累其他任务 | 每任务独占线程，OS 调度公平 |
| 内存占用（1000 并发） | 数 MB 量级 | 1000 × 2~8 MB ≈ 数 GB（不可行） |
| 严格并发上限（保护下游） | 需额外 `rateLimit` / `maxInstances` | 线程数即硬上限，天然限流 |

## 选用决策

用一个文字决策流程来选择：

1. **任务主要是 IO 等待**（HTTP 请求、shell 命令等待、数据库 / 缓存往返、webhook 回调）？
   - 是 → 选 `async`（默认）。`await` 让出让少量线程支撑高并发，单任务仅几 KB 内存。
2. **任务主要是 CPU 计算**（压缩、加解密、大文件处理、计算密集脚本）？
   - 是 → 选 `thread`。每任务独占线程由 OS 调度，避免长 CPU 任务在 tokio 线程上饿死其他任务。
3. **需要严格并发上限以保护下游系统**（限制对 DB / 外部 API 的并发连接数）？
   - 是 → 选 `thread`（线程数即硬上限），或 `async` + `rateLimit` / `maxInstances`。
4. **需要任务间强隔离**（避免某个任务拖累其他任务）？
   - 是 → 选 `thread`（独立线程，无抢占）。
5. **资源敏感环境**（容器、低配 VPS、与 PHP-FPM 共享内存）？
   - 倾向 `async`（内存占用低，默认 1024 并发仅数 MB）。

简记：**IO 密集 / 高并发短任务 → async；CPU 密集 / 强隔离 / 严格并发上限 → thread**。

## 切换代码演示

```php
<?php
use Xhjob\XhjobService;
use Xhjob\TaskBuilder;

// —— 场景一：async 模式（默认，IO 密集）——
$svc = new XhjobService('default', '/var/lib/xhjob');
$svc->ensureStopped();
putenv('XHJOB_POOL_MODE=async');
putenv('XHJOB_ASYNC_POOL_SIZE=2048');     // 高并发场景上调
$svc->start();

// 派发 1000 个 HTTP 任务，async 池可全部并发（受池上限与 FD 上限约束）
for ($i = 0; $i < 1000; $i++) {
    TaskBuilder::http('GET', "https://api.example.com/ping?id={$i}")
        ->timeout(10)
        ->dispatch('default', '/var/lib/xhjob');
}

// —— 场景二：切换到 thread 模式（CPU 密集）——
$svc->ensureStopped();                    // 必须先停旧 daemon
putenv('XHJOB_POOL_MODE=thread');
putenv('XHJOB_THREAD_POOL_SIZE=4');       // 4 个工作线程，保护下游 DB
$svc->start();                            // 重新拉起 daemon，新模式生效

// 此后派发的 CPU 密集任务在 4 个 OS 线程中分批执行
TaskBuilder::shell('php /app/jobs/compress.php ' . escapeshellarg($file))
    ->timeout(300)
    ->dispatch('default', '/var/lib/xhjob');
```

## 可复现基准对比

`releases/xhjob-thinkphp8-extend/test_xhjob_pool_diff.php` 提供了一套**可复现的差异验证法**，不依赖任何外部基准工具，仅用 Linux `/proc` 文件系统 + 时间戳分析证明两种模式的并发模型本质不同。

### 验证维度一：线程名

通过读取 daemon 进程的 `/proc/{daemon_pid}/task/{tid}/comm` 获取每个线程的名字：

```php
// 读取 daemon 进程的所有线程名
function readDaemonThreads(int $pid): array {
    $taskDir = "/proc/{$pid}/task";
    if (!is_dir($taskDir)) return [];
    $threads = [];
    foreach (array_diff(scandir($taskDir), ['.', '..']) as $tid) {
        $comm = @file_get_contents("{$taskDir}/{$tid}/comm");
        if ($comm !== false) $threads[$tid] = trim($comm);
    }
    return $threads;
}
```

- **async 模式**：存在 `xhjob-tokio` 线程，**不存在** `xhjob-worker-*` 线程。
- **thread 模式**：存在 `xhjob-worker-N` 线程，数量 = `XHJOB_THREAD_POOL_SIZE`（如 pool=2 则有 2 个），**不存在** `xhjob-tokio` 线程。

### 验证维度二：并发时间戳

派发 4 个 `sleep(3)` 任务，每个任务在开始时把纳秒时间戳写入标记文件，完成后读取 delta：

```php
// 每个任务开始时写时间戳，再 sleep
$cmd = "date +%s.%N > {$markerDir}/task-{$i}.start && sleep 3";
$mgr->create(TaskBuilder::shell($cmd)->timeout(30));
```

- **async 模式**（pool 1024）：4 个任务的开始时间戳几乎相同（delta < 1s），全部同时执行，总耗时 ≈ 1 × sleep = **3s**。`async/await` 在 `sleep` 时 yield，不占线程。
- **thread 模式**（pool=2）：4 个任务分 2 批——前 2 个立即开始，后 2 个延迟约 3s（等前 2 个释放线程），总耗时 ≈ 2 × sleep = **6s**。

### 对比总结表

测试脚本末尾输出对比（4 个 `sleep(3)` 任务）：

| 维度 | async 模式（async） | 线程模式（thread, pool=2） |
|------|---------------------|----------------------------|
| 执行线程名 | `xhjob-tokio` | `xhjob-worker-N` |
| 线程来源 | tokio async runtime | `std::thread` + `crossbeam` |
| 并发控制 | Semaphore(1024) | 线程数(2) |
| 并发模型 | 协作式（async/await yield） | 抢占式（每任务独占线程） |
| 4 任务 delta 分布 | 全部 < 1s | 前 2 个 < 1s，后 2 个 ≈ 3s |
| 总耗时 | ≈ 3s | ≈ 6s |
| 4 任务并发形态 | 全部同时 | 分 2 批（每批 2 个） |

### 运行测试

```bash
EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so
php -d extension=$EXT test_xhjob_pool_diff.php
```

测试使用独立 service（`pool-diff-test`）与独立 data_dir（`/tmp/xhjob-pool-diff`），不污染默认服务。每个阶段以 `[N] 描述 ... PASS/FAIL` 形式逐行输出，末尾汇总并打印对比表。

## 小结

两种模式功能行为完全一致（同一套任务状态机、触发器、重试、编排），只是**并发模型本质不同**：

- `async` = M:N 高并发，IO 密集型最优；
- `thread` = 真并行受限于线程数，CPU 密集型或需严格隔离时使用。

按场景选择即可，无需为"哪个更高级"纠结——它们是互补而非替代关系。
