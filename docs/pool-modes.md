# 双线程池模式对比（Pool Modes）

> 进阶篇 · xhjob 提供两种线程池执行模式，通过 `XHJOB_POOL_MODE` 环境变量选择。本文严格对齐源码实现，给出差异表、性能对比、选型决策树、切换演示与可复现基准验证方法。

---

## 概述

xhjob daemon 在派发任务时（`process_one()` 调度路径），会读取环境变量 `XHJOB_POOL_MODE` 决定把任务提交到哪个全局池：

- **`async`（默认，推荐）** —— tokio M:N 调度，N 个 worker 线程复用跑 M 个 async task，适合 IO 密集 / 高并发短任务。
- **`thread`** —— `std::thread` 1:1 调度，每个任务独占一个 OS 线程，适合 CPU 密集 / 强隔离 / 严格并发上限场景。
- **`coroutine`** —— 旧别名，与 `async` 完全等价（路由到同一池），仅为向后兼容保留。

读取与路由逻辑位于 `/workspace/src/scheduler/queue.rs`：

```rust
// queue.rs:779
let pool_mode = std::env::var("XHJOB_POOL_MODE").unwrap_or_else(|_| "async".to_string());

// queue.rs:792-801
if pool_mode == "thread" {
    crate::pool::thread_pool::global().submit(move || {
        if let Some(rt) = coroutine_pool::global_runtime() {
            rt.block_on(counted_future);
        }
    });
} else {
    // `async`（推荐）与 `coroutine`（旧别名）都走这里
    drop(coroutine_pool::global().spawn(counted_future));
}
```

> ⚠️ **切换模式必须 stop + start**：env var 在 `process_one()` 每次调度时读取，但进程 env 不可变，且两个全局池都由 `OnceCell` 守护（首次使用即固定）。运行期改动 env 不会生效，必须 `xhjob_stop()` 后重新 `xhjob_start()`。

---

## 实现差异表

| 维度 | async（默认） | thread |
|---|---|---|
| 调度模型 | tokio M:N（多任务复用少量线程） | std::thread 1:1（每任务一线程） |
| 全局池 | `CoroutinePool::global()` | `ThreadPool::global()` |
| 守护方式 | `OnceCell`（首次使用初始化） | `OnceCell`（首次使用初始化） |
| 控制 env | `XHJOB_ASYNC_POOL_SIZE`（旧别名 `XHJOB_COROUTINE_POOL_SIZE`） | `XHJOB_THREAD_POOL_SIZE` |
| 默认并发上限 | 1024（信号量许可数） | `num_cpus::get()`（CPU 核数） |
| 底层 worker 线程数 | `num_cpus::get()` | 与并发上限相同（1:1） |
| worker 线程名 | `xhjob-tokio`（所有 worker 共用同一名字） | `xhjob-worker-{i}`（i = 0..size-1，每个唯一） |
| 任务执行方式 | `tokio::spawn` + 信号量 permit | `block_on(future)` 阻塞整个线程 |
| 代码位置 | `/workspace/src/pool/coroutine_pool.rs` | `/workspace/src/pool/thread_pool.rs` |

---

## 性能特征对比表

| 维度 | async（默认） | thread |
|---|---|---|
| 调度模型 | tokio M:N（多任务复用少量线程） | std::thread 1:1（每任务一线程） |
| 默认并发上限 | 1024（信号量） | num_cpus（CPU 核数） |
| 每任务内存开销 | KB 级（tokio task） | MB 级（OS 线程栈默认 2-8MB） |
| IO 密集表现 | 优秀（async I/O 复用） | 一般（线程阻塞浪费） |
| CPU 密集表现 | 一般（线程少，CPU 任务排队） | 优秀（线程多，CPU 并行） |
| 隔离性 | 弱（共享 runtime，一个 panic 影响其它） | 强（独立线程，panic 仅影响自身） |
| 严格并发上限 | 难（信号量软限） | 易（线程池硬限） |
| worker 名（`/proc/{pid}/task` 可见） | `xhjob-tokio` | `xhjob-worker-0..N` |

---

## 配置项

| 环境变量 | 作用 | 默认值 | 接受值 | 读取位置 |
|---|---|---|---|---|
| `XHJOB_POOL_MODE` | 选择线程池模式 | `async` | `async` / `coroutine` / `thread` | `src/scheduler/queue.rs:779` |
| `XHJOB_ASYNC_POOL_SIZE` | async 模式信号量许可数（最大并发） | `1024` | 正整数 | `src/pool/coroutine_pool.rs:64` |
| `XHJOB_COROUTINE_POOL_SIZE` | 同上的旧别名（向后兼容） | `1024` | 正整数 | `src/pool/coroutine_pool.rs:64` |
| `XHJOB_THREAD_POOL_SIZE` | thread 模式线程数 | `num_cpus::get()` | 正整数 | `src/pool/thread_pool.rs:141` |

> `XHJOB_ASYNC_POOL_SIZE` 与 `XHJOB_COROUTINE_POOL_SIZE` 同时设置时，优先读取 `XHJOB_ASYNC_POOL_SIZE`（推荐名），见 `coroutine_pool::configured_max()` 内的循环顺序。

---

## 选型决策树

| 场景 | 推荐模式 | 理由 |
|---|---|---|
| IO 密集 / 高并发短任务（HTTP、Redis、DB 查询） | **async（默认）** | tokio I/O 复用 + 1024 并发，单线程可承载大量等待中的 task，内存开销 KB 级 |
| CPU 密集（编解码、加解密、大计算） | **thread** | 1:1 真并行受 CPU 核数限制；async 模式 worker 线程少（=num_cpus），CPU 任务会排队且无法 yield |
| 强隔离需求（任务可能 panic / 段错误） | **thread** | 独立 OS 线程，panic 仅影响自身；async 共享 runtime，panic 影响其它 task |
| 严格并发上限（限流、配额） | **thread** | 线程池硬限（线程数即上限）；async 信号量是软限，配合不当易超发 |
| 混合场景（IO 为主 + 个别 CPU 密集） | **默认 async**，对个别 CPU 密集任务用独立 thread daemon 服务隔离 | 不混用同一池，避免 CPU 任务拖垮 IO 调度 |

**一句话决策**：

- 默认选 `async`，绝大多数业务（Web、API、消息消费）足够。
- 只有明确 CPU 密集或需要强隔离/硬并发上限时，才切 `thread`。
- 混合场景不要在同一 daemon 内混用，把 CPU 密集任务拆到独立 thread 模式 daemon。

---

## 切换代码演示

```php
<?php
// 切换到 thread 模式（必须 stop + start）
putenv("XHJOB_POOL_MODE=thread");
// 可选：覆盖线程数（默认 num_cpus）
putenv("XHJOB_THREAD_POOL_SIZE=8");

xhjob_start();  // 启动时初始化 ThreadPool::global()
// ... 派发任务 ...
xhjob_stop();   // 切换回 async 必须先 stop

// 切换回 async 模式
putenv("XHJOB_POOL_MODE=async");
// 可选：覆盖最大并发（默认 1024）
putenv("XHJOB_ASYNC_POOL_SIZE=2048");

xhjob_start();  // 重新初始化 CoroutinePool::global()
// ... 派发任务 ...
xhjob_stop();
```

### 切换注意事项

1. **env 不可变**：PHP 进程通过 `putenv()` 设置的 env 在进程生命周期内固定，运行期改动不会影响已读取的值。
2. **OnceCell 守护**：`CoroutinePool::global()` 与 `ThreadPool::global()` 都用 `OnceCell`，首次调用即初始化并固定，后续调用返回同一实例。
3. **必须 stop + start**：以上两点叠加 → 切换模式或调整池大小，必须先 `xhjob_stop()` 让 daemon 退出，再改 env 后 `xhjob_start()` 重启，新配置才会生效。
4. **不要混用**：同一 daemon 生命周期内任务都走同一池，不要试图让部分任务走 async、部分走 thread。

---

## 可复现基准对比说明

### 1. worker 名验证法（最快）

daemon 启动后，查看其 worker 线程名：

```bash
# {daemon_pid} 替换为实际 daemon 进程 PID
ls /proc/{daemon_pid}/task | xargs -I{} cat /proc/{daemon_pid}/task/{}/comm
```

- **async 模式**：可见多条 `xhjob-tokio`（所有 worker 共用同一名字，数量 = `num_cpus`）。
- **thread 模式**：可见 `xhjob-worker-0`、`xhjob-worker-1`、... `xhjob-worker-{N-1}`（每个 worker 唯一名字，数量 = `XHJOB_THREAD_POOL_SIZE` 或 `num_cpus`）。

### 2. 并发时间戳验证法

派发 N 个 `sleep 5` 的 shell 任务，记录完成时间分布：

- **async 模式**：1024 并发上限远大于 N，全部并行 → 全部约 5s 内完成。
- **thread 模式**：按 CPU 核数分批，例如 8 核则 8 个一批 → N=16 时两批共约 10s，N=24 时三批共约 15s。

示例（8 核机器，N=16）：

| 模式 | 第一批完成 | 第二批完成 | 总耗时 |
|---|---|---|---|
| async | ~5s | — | ~5s |
| thread | ~5s（8 个） | ~10s（剩 8 个） | ~10s |

### 3. 引用验证脚本

如存在 `releases/xhjob-thinkphp8-extend/test_xhjob_pool_diff.php`，可直接运行该脚本进行自动化对比验证。该脚本通常会：

- 分别以 `XHJOB_POOL_MODE=async` 与 `XHJOB_POOL_MODE=thread` 启动 daemon。
- 派发相同批次任务，记录耗时与 worker 线程名。
- 输出对比报告。

---

## 生产建议

1. **默认 async 足够**：绝大多数业务场景（Web、API、消息消费、定时任务）默认 `async` 即可，1024 并发上限对 IO 密集任务绰绰有余。
2. **CPU 密集型独立服务用 thread**：明确 CPU 密集（编解码、加解密、大计算）或需强隔离/硬并发上限时，部署独立 daemon 并设 `XHJOB_POOL_MODE=thread`。
3. **不要混用**：同一 daemon 内任务都用同一池。混合场景请拆分为多个 daemon，各自独立配置池模式，避免 CPU 任务拖垮 IO 调度。
4. **调整大小必须重启**：调整 `XHJOB_ASYNC_POOL_SIZE` / `XHJOB_THREAD_POOL_SIZE` 与切换模式一样，需要 stop + start 才能生效。
5. **worker 名是调试抓手**：线上排查时，`/proc/{pid}/task/*/comm` 可快速判断当前实际运行的模式与 worker 数量。
