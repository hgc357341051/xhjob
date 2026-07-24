---
title: 持久化与崩溃恢复
parent: 核心能力
nav_order: 34
---

# 持久化与崩溃恢复

Xhjob 面向生产场景设计了完整的崩溃恢复链路：SQLite 持久化、acksLate 重派、execution_lease 防重复执行、watchdog 假死检测、PID 复用防护，以及 daemon SIGKILL 后的自动恢复流程。相关实现分布于 `src/store/sqlite.rs`、`src/executor/shell.rs`、`src/scheduler/watchdog.rs` 与 `src/daemon_main.rs`。

---

## persist（SQLite + WAL 持久化）

`persist(bool $on)` 控制任务是否持久化到磁盘。

- `true`：使用 **SQLite + WAL** 模式存储任务、结果与事件，daemon 重启后可恢复。
- `false`（默认）：使用 `InMemoryStore`，所有数据驻留内存，**daemon 崩溃即全部丢失**。

### 编译要求

SQLite 持久化能力需要 **`--all-features`** 编译（启用 `sqlite` feature）。未启用该 feature 时，即便 `persist(true)` 也会 fallback 到 `InMemoryStore`。

> ⚠️ **警告**：未启用持久化时，daemon 进程一旦崩溃（OOM、SIGKILL、断电），所有 `Pending` / `Running` 任务与历史结果将 **全部丢失且无法恢复**。生产环境务必开启 `persist(true)` 并以 `--all-features` 编译。

### SQLite 配置

启用持久化时，连接初始化会依次设置（见 `src/store/sqlite.rs`）：

- `PRAGMA journal_mode=WAL`：写前日志，提升并发读写性能。
- `PRAGMA synchronous=NORMAL`：在 WAL 模式下兼顾安全与性能。
- `PRAGMA busy_timeout=5000`：5 秒忙等待，避免多 PHP-FPM worker 并发写时立即报 `SQLITE_BUSY`。
- `PRAGMA foreign_keys=ON`：开启外键约束（`results.task_id` → `tasks.id`）。
- `PRAGMA quick_check`：启动时完整性校验。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 生产环境：开启持久化，崩溃可恢复
TaskBuilder::shell('important-job.sh')
    ->cron('0 * * * *')
    ->persist(true)
    ->dispatch();
```

---

## acksLate（崩溃后重派）

`acksLate(bool $on)` 控制 daemon 在任务执行期间崩溃后是否重新派发该任务，参考 Celery `acks_late`。

- `false`（默认）：任务一旦被取出派发即 ack，daemon 崩溃时 Running 任务不会被重新派发（可能丢失）。
- `true`：任务在 **完成（成功/失败）后才 ack**。daemon 崩溃重启后，所有 `acks_late=true` 的 Running 任务会被 `reset_running_to_pending` 重置为 Pending 并立即重新派发。

> 适用场景：任务必须"至少执行一次"（at-least-once）。代价是崩溃恢复后可能 **重复执行** 一次，业务需保证幂等。

### 代码演示

```php
// 关键任务：acksLate 保证崩溃后重派（业务需幂等）
TaskBuilder::shell('send-email.sh')
    ->acksLate(true)
    ->persist(true)   // acksLate 需配合 persist 才能在崩溃后恢复
    ->dispatch();
```

---

## execution_lease（执行租约，防重复执行）

execution_lease 是 Xhjob 防止"daemon 崩溃导致孤儿子进程仍在运行、恢复后又重新派发造成重复执行"的核心机制。相关逻辑位于 `src/executor/shell.rs` 的 `execute_with_lease`。

### 工作原理

1. Shell 执行器在 `child.spawn()` 成功后，**立即** 将 `worker_pid` 与 `worker_starttime`（取自 `/proc/<pid>/stat` 的第 22 字段）写入 store。
2. 该写入是 **内联 await**（非 fire-and-forget 的 `tokio::spawn`），确保在进入 wait/timeout 路径前租约已落盘。
3. daemon 崩溃重启后，`reset_running_to_pending` 对每个 Running 任务调用 `is_process_alive_with_starttime(pid, starttime)`：
   - 若进程仍存活且 starttime 匹配 → **租约仍持有**，**不重派**，记录 `lease_held` 事件，让孤儿子进程自然完成后被回收。
   - 若进程已死或 starttime 不匹配（PID 被复用）→ 视为死孤儿，重置为 Pending 重新派发。

> ⚠️ **P0 修复背景**：早期版本用 fire-and-forget 的 `tokio::spawn` 写租约，JoinHandle 被丢弃。若 daemon 在 `child.spawn()` 与 SQLite UPDATE 完成之间被 SIGKILL/OOM，租约行未落盘，`reset_running_to_pending` 看到 `worker_pid=NULL` → 把仍在运行的孤儿重置为 Pending → **重复执行**。修复后改为内联 await，保证租约在崩溃窗口打开前已持久化。

### LeaseHeld 事件

当崩溃恢复时检测到某 Running 任务的 worker 进程仍存活，会记录 `EventType::LeaseHeld` 事件，便于观测哪些任务因租约持有而未被重派。

---

## watchdog（任务假死检测）

watchdog 周期性扫描所有 `Running` 任务，检测"假死"并强制回收。相关实现位于 `src/scheduler/watchdog.rs`。

### 检测逻辑

当一个 Running 任务的已运行时长超过 `timeout * factor` 仍未完成时，watchdog：

1. 通过队列发出 cancel 信号，终止孤儿子进程；
2. 将任务标记为 `Interrupted`；
3. 记录 `EventType::HungDetected` 事件。

### 覆盖的盲区

watchdog 专门覆盖 **IO hang 不触发 tokio timeout** 的盲区：当子进程卡在 fifo / NFS / DNS 等阻塞 IO 上时，`tokio::time::timeout` 的 Future 不会被触发（子进程未退出），任务会永远停留在 Running，只有 daemon 重启才能恢复。watchdog 以"运行时长"为判据，能捕获这类假死。

### 配置

通过环境变量配置（在 `daemon_main` 中读取）：

- `XHJOB_WATCHDOG_FACTOR`（默认 **2**）：容忍倍数。任务只有在超过 `timeout * factor` 秒后才被判定为假死，避免对略微超时（如正处于 SIGTERM 宽限期内）的任务误杀。
- `timeout == 0`（未设超时）的任务会被跳过——没有 timeout 基线就无法判断"假死"。

---

## PID 复用防护

`reset_running_to_pending` 在判断 worker 进程是否存活时，采用 **`pid` + `starttime` 双校验**，而非仅查 `pid`。

### 为什么只查 pid 不安全

操作系统会复用已退出进程的 PID。若仅凭 `pid` 判断存活：

1. 子进程 A（pid=12345）执行任务时 daemon 崩溃；
2. 子进程 A 退出，PID 12345 被回收；
3. 一个无关进程 B 恰好被分配到 PID 12345；
4. daemon 重启后只查 pid=12345 → 发现"存活" → 误判租约持有 → 不重派 → 任务实际丢失。

### 双校验方案

`is_process_alive_with_starttime(pid, starttime)` 同时比对进程的 starttime（启动时刻）。进程死亡后 PID 被复用，新进程的 starttime 必然不同，因此会被正确判定为"死孤儿"并重置为 Pending 重派。starttime 取自 `/proc/<pid>/stat` 的第 22 字段。

---

## SQLite 完整性检查

daemon 启动打开 SQLite 连接时执行两项保障（见 `src/store/sqlite.rs`）：

- **`PRAGMA quick_check`**：扫描每个 b-tree 页，报告结构性损坏。健康库返回单行 `ok`；损坏库返回一行或多行问题描述（或查询本身报错）。检测到损坏时 daemon **拒绝启动**，避免在损坏的库上继续写入造成不可恢复的破坏。
- **`PRAGMA foreign_keys=ON`**：SQLite 默认 **关闭** 外键约束。若不显式开启，`results` 表上的 `FOREIGN KEY (task_id) REFERENCES tasks(id)` 会沦为空操作，可能产生孤儿结果行。每次连接都显式开启以强制约束。

---

## daemon SIGKILL 崩溃恢复流程

当 daemon 被 SIGKILL（或 OOM、断电）后重启，恢复流程按以下顺序执行（见 `src/daemon_main.rs`）：

1. **stale pid 清理**：检查 pidfile 中的旧 pid；通过 `starttime` 校验该 pid 是否属于"已死的旧 daemon"。若 PID 已被其他进程复用（starttime 不同），不会误杀，仅清理 pidfile。
2. **DB 完整性检查**：执行 `PRAGMA quick_check`，确认库结构完好后才继续；损坏则拒绝启动。
3. **`reset_running_to_pending`**：将所有 `Running` 状态的任务重置为 `Pending`，使其在下一个调度周期可被重新派发。
4. **lease check（租约检查）**：对每个被重置的任务，检查其 `worker_pid` + `worker_starttime` 对应的进程是否仍存活：
   - **租约仍持有**（进程存活且 starttime 匹配）→ **跳过重派**，记录 `lease_held` 事件，等待孤儿子进程自然完成。
   - **租约已失效**（进程已死或 PID 被复用）→ 正常重派。

> 该流程必须 **在 cron 调度器与任务队列启动之前** 完成，确保重置后的任务对调度器可见、能被立即重新派发。

### 恢复流程示意

```
daemon SIGKILL 崩溃
        │
        ▼
  重启 daemon
        │
        ▼
① 清理 stale pid（校验 starttime，防误杀复用 PID）
        │
        ▼
② PRAGMA quick_check（库损坏则拒绝启动）
        │
        ▼
③ reset_running_to_pending（Running → Pending）
        │
        ▼
④ lease check
   ├─ 进程存活 + starttime 匹配 → 跳过重派，记 lease_held
   └─ 进程已死 / PID 复用 → 重派
        │
        ▼
  启动 cron 调度器 + 任务队列（重置任务对调度器可见）
```
