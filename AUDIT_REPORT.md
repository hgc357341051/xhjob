# xhjob Rust 扩展全量代码审查报告

> **审查视角**：PHP 生产环境（PHP-FPM + 高并发 worker + 长跑 daemon + 运维友好性）
> **审查范围**：`/workspace/src` 下 27 个 Rust 源文件（~12000 行）
> **审查方式**：只读审查，未修改任何代码
> **审查日期**：2026-07-22
> **最后更新**：2026-07-23（追加修复状态追踪章节，见下方「修复状态追踪」）

---

## 修复状态追踪

> 本章节在原只读审查报告之后追加，用于回写各 finding 的修复状态，使报告与代码现状保持同步。
> 标记约定：✅ 已修复 / 🔄 部分修复 / ⬜ 待办 / ➖ 不适用。

### P0 级修复状态

| 维度 / Finding | 状态 | 修复说明 |
|---|---|---|
| P0-1 signal_cancel 返回值吞没 | ✅ | `let _ =` → `if !signaled { debug! }`（daemon_main.rs 多处） |
| P0-2 task id 字符集校验 + 错误链 | ✅ | `^[A-Za-z0-9_-]{1,64}$` 校验 + `thiserror` struct 变体 + `#[source]` |
| P0-3 IPC timeout 缺失 | ✅ | 移入 `lib.rs` `ipc_request` helper，统一 5s 超时 |
| P0-4 Unix socket fchmod + SO_PEERCRED | ✅ | `ipc/unix_socket.rs` 强制 0600 + 对端凭据校验 |
| P0-5 SQLite spawn_blocking | ✅ | `store/sqlite.rs` 全部 query 走 `spawn_blocking`，不阻塞 runtime |
| P0-6 payload AES-256-GCM 加密 | ✅ | `store/crypto.rs` + `OsRng` nonce + `XHJOB_ENCRYPTION_KEY` |
| P0-7 .env 风格配置解析 | ✅ | `config.rs` + `XHJOB_CONFIG_FILE` |
| P0-8 6 个 AtomicU64 计数器 | ✅ | `utils/metrics.rs` |
| P0-9 owner 多租户隔离列 | ✅ | `store/sqlite.rs` owner 列 + ownership_check |
| P0-10 maybe_encrypt 防泄漏 | ✅ | 落库前加密，读取时解密 |
| P0-13 配置文件加载 | ✅ | `XHJOB_CONFIG_FILE` |
| P0-17 ownership_check | ✅ | 默认拒绝，owner 字段匹配才放行 |

### 第三轮 5 维度并行审查修复状态

| 维度 | Finding | 状态 | 修复说明 |
|---|---|---|---|
| A | per-chain mutex（H5） | ✅ | `chain.rs` `CHAIN_LOCKS` per-id Mutex，+ 并发测试 `test_concurrent_advance_does_not_skip_steps` |
| A | per-chord mutex | ✅ | `chord.rs` `CHORD_LOCKS` per-id Mutex，+ 并发测试 `test_concurrent_refresh_does_not_double_dispatch_callback` |
| A | handle_dispatch TOCTOU | ✅ | `daemon_main.rs` `DISPATCH_LOCKS` per-task-id Mutex，序列化 check-then-insert |
| B | trace_id info_span! | ✅ | `daemon_main.rs` `info_span!("ipc", op, id, trace_id)` + `enter()` |
| C | kill_on_drop(true) | ✅ | `executor/shell.rs` Child handle drop 时 SIGKILL |
| C | process_group(0) | ✅ | `executor/shell.rs` 独立进程组，`kill(-pid, SIGTERM)` 整棵进程树 |
| C | env_clear + 最小 PATH | ✅ | `executor/shell.rs` 清除 daemon 环境变量，仅恢复 PATH/HOME |
| C | max_output_bytes | ✅ | `executor/shell.rs` `take(64MiB)` 限制 stdout/stderr 捕获量 |
| C | HTTP connect_timeout | ✅ | `executor/http.rs` `.connect_timeout(10s)`，`XHJOB_HTTP_CONNECT_TIMEOUT` |
| C | HTTP body size limit | ✅ | `executor/http.rs` Content-Length 预检 + 流式累加 64MiB 上限 |
| D | XhjobError source 链 | ✅ | `errors.rs` Ipc/Store/Exec/Config struct 变体 + `#[source] Option<BoxedSource>` |
| D | `let _ =` 全部清理 | ✅ | 3 处 → `if let Err(e) { warn! }` |
| E | XHJOB_MAX_PENDING 背压 | ✅ | `scheduler/queue.rs:118` 默认 10000 |
| E | XHJOB_MAX_CONNECTIONS | ✅ | `daemon_main.rs:296` Semaphore 256 |
| E | XHJOB_SHUTDOWN_DRAIN_SECS | ✅ | `daemon_main.rs:373` wait_for_idle 默认 30s |
| E | XHJOB_MAX_CRON_PER_TICK | ✅ | `scheduler/cron.rs:120` 默认 500 |
| E | XHJOB_IPC_TIMEOUT_SECS | ✅ | `ipc/mod.rs:233` 默认 5s |
| E | XHJOB_IPC_NO_PEERCRED | ✅ | `ipc/unix_socket.rs:76` 测试逃生开关 |

### 第四轮：BUG 修复 + 功能补全 + 测试补全

| 类别 | 项 | 状态 | 修复说明 |
|---|---|---|---|
| BUG | percent as u8 截断绕过 0-100 校验 | ✅ | `daemon_main.rs` u64 校验后再 cast，256→0/356→100 不再绕过 |
| BUG | handle_dispatch replace_existing TOCTOU | ✅ | per-task-id dispatch lock |
| BUG | persist=false 哑字段 | ✅ | 非周期任务进入终态后自动 `delete_task`（fire-and-forget-after-completion） |
| 功能 | persist 字段执行路径 | ✅ | `scheduler/queue.rs:609` 终态后自动删除 |
| 测试 | ownership_check / filter_*_by_owner | ✅ | 6 个单元测试覆盖空 owner / 匹配 / 不匹配 / 过滤 |
| 测试 | tags 过滤 + update_progress | ✅ | 2 个往返测试 |
| 测试 | per-chord / per-chain 并发 mutex | ✅ | 2 个 `tokio::spawn` + `join!` 并发测试 |

### 第四轮：MINOR 修复

| 项 | 状态 | 修复说明 |
|---|---|---|
| build() 校验空 URL/空 cmd | ✅ | `task/mod.rs::build()` HttpPayload.url/method + ShellPayload.cmd 非空校验，+ 9 个单元测试 |
| build() 校验 retry_delay>0 当 retry_max>0 | ✅ | `task/mod.rs::build()` 防 retry storm，+ 测试 |
| SQLite wal_checkpoint 定期执行 | ✅ | `store/sqlite.rs::cleanup_expired_results` 末尾 `PRAGMA wal_checkpoint(PASSIVE)`，每 ~60s 随 scan_once 执行 |

### 文档同步

| 项 | 状态 | 修复说明 |
|---|---|---|
| README 环境变量文档补全 | ✅ | `README.md` 环境变量表从 9 项补全到 20 项，新增 `XHJOB_MAX_PENDING` / `XHJOB_MAX_CRON_PER_TICK` / `XHJOB_MAX_CONNECTIONS` / `XHJOB_SHUTDOWN_DRAIN_SECS` / `XHJOB_IPC_TIMEOUT_SECS` / `XHJOB_IPC_NO_PEERCRED` / `XHJOB_HTTP_CONNECT_TIMEOUT` / `XHJOB_OWNER` / `XHJOB_SERVICE_NAME` / `XHJOB_CONFIG_FILE` / `XHJOB_ENCRYPTION_KEY` |
| AUDIT_REPORT 修复状态追踪 | ✅ | 本章节 |
| XHJOB_PERSIST 双分支语义说明 | ✅ | README 环境变量表标注 feature flag 影响取值集合 |

---

## 一、总体评价

### 成熟度评估

| 模块 | 成熟度 | 头号生产风险 |
|---|---|---|
| daemon | 中 | IPC 无超时导致 socket fd 泄漏、daemon 静默不可用（P0） |
| pool | 中下 | thread 模式 `block_on` 嵌套脆弱 + shutdown 孤儿子进程 + 无背压 OOM |
| scheduler | 中 | chain/chord 重启无恢复致业务流程丢失（P0）+ `max_instances>1` 不生效 |
| executor | 中 | Shell 命令注入（设计性 P0）+ stdout 管道死锁（P1） |
| store | 中上 | SQLite 单连接串行化是吞吐瓶颈（P0）+ 未设 busy_timeout |
| ipc | 中下 | Windows Named Pipe 默认 DACL + 远程可连（P0） |
| lib.rs | 中 | `xhjob_dispatch` 错误返回不带 `error:` 前缀（P0） |
| task / retry | 中 | `retry_delay=0` 重试风暴 + 429 不重试 |
| utils / service / outcome | 中上 | 内存限制单位字节易误设 + Interrupted 黑洞 |

### P0/P1/P2 数量汇总

| 优先级 | 数量 | 含义 |
|---|---|---|
| **P0** | 7 | 安全漏洞、数据丢失、静默不可用 |
| **P1** | 21 | 功能缺陷、资源泄漏、错误处理不当 |
| **P2** | 30+ | 代码异味、可读性、命名、文档 |

---

## 二、优点清单（值得保留的好功能与设计）

### 架构设计

1. **避开 in-process fork+setsid+fork 的 fork-safe 陷阱**（[unix.rs:1-7, 61-92](file:///workspace/src/daemon/unix.rs)）：选择 re-exec PHP 二进制 + `pre_exec(setsid)`，正确规避 tokio runtime 非 fork-safe 的核心风险。
2. **用 `-r` 命令行参数而非 env 传递 service_name / data_dir**（[unix.rs:41-59](file:///workspace/src/daemon/unix.rs)、[windows.rs:35-47](file:///workspace/src/daemon/windows.rs)）：兼容 phpenv 等 version-manager shim 清洗环境变量的问题。
3. **双重就绪检查（PID liveness + IPC socket ready）**（[mod.rs:245-268](file:///workspace/src/daemon/mod.rs)）：解决"PID 已写但 socket 未 bind"的竞态。
4. **SIGTERM → 10 秒轮询 → SIGKILL 兜底**（[mod.rs:165-188](file:///workspace/src/daemon/mod.rs)）：优雅退出 + 强制兜底。
5. **stale PID / socket 自动清理**（[mod.rs:73-84, 97-107](file:///workspace/src/daemon/mod.rs)）：避免崩溃后启动被占位文件阻塞。
6. **acks_late 崩溃恢复**（[daemon_main.rs:104-128](file:///workspace/src/daemon_main.rs)）：仅重置 `acks_late=true` 的 Running 任务，严格遵循 Celery 语义。
7. **多服务隔离**（[service/mod.rs](file:///workspace/src/service/mod.rs)）：`^[a-zA-Z][a-zA-Z0-9_-]{0,31}$` 校验，PID/sock/db/log 按服务名命名。

### 并发模型

8. **双模式可切换**（[queue.rs:477-488](file:///workspace/src/scheduler/queue.rs)）：async（M:N tokio）与 thread（1:1 OS 线程）按场景切换。
9. **coroutine_pool 用 Semaphore 控制并发上限**（[coroutine_pool.rs:44-56](file:///workspace/src/pool/coroutine_pool.rs)）：默认 1024，permit 是标准 RAII 自动释放。
10. **thread_pool 用 `catch_unwind` 隔离 panic**（[thread_pool.rs:60](file:///workspace/src/pool/thread_pool.rs)）：单个任务 panic 不杀死 worker。
11. **cancel_flag 用 AtomicBool 轮询而非 signal handler**（[queue.rs:46-69](file:///workspace/src/scheduler/queue.rs)）：避免 signal handler 中调用非 async-safe 函数。

### 调度与编排

12. **cron 5 字段自动补 "0" 秒**（[cron.rs:50-54](file:///workspace/src/scheduler/cron.rs)）：兼容传统 5 字段 cron。
13. **时区通过 chrono-tz 显式校验**（[cron.rs:28-32, 59-68](file:///workspace/src/scheduler/cron.rs)）：非法时区返回错误而非静默 fallback。
14. **coalesce + misfire_grace_time 语义完整**（[cron.rs:87-99](file:///workspace/src/scheduler/cron.rs)）：misfire 时记录 `Missed` 事件。
15. **周期任务成功后保持 Pending + 递增 execution_count**（[queue.rs:321-369](file:///workspace/src/scheduler/queue.rs)）：避免终态被 load_active_tasks 过滤导致 count 永远停 1。
16. **chord 终态幂等**（[chord.rs:79-94](file:///workspace/src/scheduler/chord.rs)）：已 success/partial_failed 的 chord 不重算。
17. **group 状态按需从 live task 重算**（[group.rs:37-86](file:///workspace/src/scheduler/group.rs)）：daemon 重启后可恢复。
18. **rate_limiter 滑动窗口实现正确**（[rate_limit.rs:42-56](file:///workspace/src/scheduler/rate_limit.rs)）：边界正确，有 `forget` 清理接口。

### 执行器

19. **soft_timeout 流程正确**（[shell.rs:63-90](file:///workspace/src/executor/shell.rs)）：SIGTERM → grace → SIGKILL + wait reap，避免僵尸进程。
20. **编码转换健壮**（[shell.rs:201-250](file:///workspace/src/executor/shell.rs)）：`encoding_rs` 处理 GBK/Big5/Shift_JIS，未知编码返回错误而非静默 lossy。
21. **TLS 默认安全**（[http.rs:117-126](file:///workspace/src/executor/http.rs)）：rustls 默认校验证书，未禁用。
22. **代理认证剥离日志**（[http.rs:84-93](file:///workspace/src/executor/http.rs)）：避免凭据出现在错误/日志中。

### 存储与 IPC

23. **SQLite 全部参数化查询**（[sqlite.rs](file:///workspace/src/store/sqlite.rs)）：`params![]` 宏，无 SQL 注入。
24. **WAL 模式 + synchronous=NORMAL**（[sqlite.rs:19-22](file:///workspace/src/store/sqlite.rs)）：读写并发与持久性的合理平衡。
25. **Schema 迁移**（[sqlite.rs:158-172](file:///workspace/src/store/sqlite.rs)）：`ensure_column` 动态补列兼容旧库。
26. **IPC 帧协议 64MB 上限**（[mod.rs:109-111](file:///workspace/src/ipc/mod.rs)）：防 DoS。

### API 与错误处理

27. **Task 字段 serde default 显式函数**（[task/mod.rs:31-46, 176-191](file:///workspace/src/task/mod.rs)）：修复"PHP 只传 task_type + payload 时 timeout=0"的真实 bug。
28. **`Some(0) → None` 防御性归一化**（[task/mod.rs:645-649](file:///workspace/src/task/mod.rs)）：对冲 PHP 客户端 Option 字段输出 0 的旧 bug。
29. **backoff_delay 溢出保护完备**（[retry/mod.rs:113-122](file:///workspace/src/retry/mod.rs)）：attempts≥63 时用 u64::MAX。
30. **should_retry 区分 HTTP 5xx/4xx/网络错误**（[retry/mod.rs:58-73](file:///workspace/src/retry/mod.rs)）：5xx 重试、4xx 不重试。
31. **max_memory_per_child fail-open**（[limits.rs:73-81](file:///workspace/src/utils/limits.rs)）：RSS 读不到时不触发关闭。

---

## 三、缺点清单（按 P0/P1/P2 优先级排序）

### P0：安全漏洞、数据丢失、静默不可用

| # | 模块 | 问题 | 位置 | 修复方向 |
|---|---|---|---|---|
| 1 | lib.rs | `xhjob_dispatch` 服务名校验失败时返回 `e`（无 `error:` 前缀），PHP 端把错误字符串当 task_id 用，非法服务名变成"假成功" | [lib.rs:129](file:///workspace/src/lib.rs) | 改 `return format!("error: {}", e)` |
| 2 | lib.rs / task | `Xhjob::id()` 不校验字符集，`withId("error: ...")` 破坏 PHP `dispatch()` 的 `error:` 前缀探测契约 | [lib.rs:896-899](file:///workspace/src/lib.rs) | 加 `^[A-Za-z0-9_-]{1,64}$` 校验 |
| 3 | daemon / ipc | `handle_connection` / `read_frame` / `write_frame` 无超时，PHP-FPM worker 被 kill -9/OOM 后 daemon 端 tokio task 永久挂起，socket fd 累积导致 daemon 静默不可用 | [daemon_main.rs:247-286](file:///workspace/src/daemon_main.rs) + [ipc/mod.rs:104-117](file:///workspace/src/ipc/mod.rs) | 包 `tokio::time::timeout`（5-10s） |
| 4 | scheduler | chain/chord daemon 重启后无恢复机制。chain 卡死在某步，chord callback 永不 dispatch，业务流程丢失 | [chain.rs:25-50](file:///workspace/src/scheduler/chain.rs) + [chord.rs:139-156](file:///workspace/src/scheduler/chord.rs) | 启动时扫描 state=running 的 chain/chord 重新推进 |
| 5 | ipc (Windows) | Named Pipe 未设安全描述符（DACL），默认允许 Everyone + 远程连接，任意用户 RCE | [named_pipe.rs:16-19, 37-39](file:///workspace/src/ipc/named_pipe.rs) | 加 `SecurityAttributes` + `reject_remote_clients(true)` |
| 6 | store (SQLite) | 未设 `PRAGMA busy_timeout`，并发写入时立即 SQLITE_BUSY | [sqlite.rs:19-22](file:///workspace/src/store/sqlite.rs) | `PRAGMA busy_timeout=5000` |
| 7 | store (SQLite) | 单 `Mutex<Connection>` 串行化所有 DB 操作，高并发 PHP-FPM worker 下严重吞吐瓶颈 | [sqlite.rs:9-11](file:///workspace/src/store/sqlite.rs) | 引入连接池或读写分离 |

### P1：功能缺陷、资源泄漏、错误处理不当

| # | 模块 | 问题 | 位置 |
|---|---|---|---|
| 8 | daemon | shutdown 不 drain 在途任务，`record_worker_limits` 500ms 后 `exit(0)` 强退，Running 任务静默丢失 | [daemon_main.rs:235-244](file:///workspace/src/daemon_main.rs) + [queue.rs:731-735](file:///workspace/src/scheduler/queue.rs) |
| 9 | daemon | `send_terminate` 用 `std::thread::sleep` 阻塞 PHP-FPM worker 最多 10 秒 | [mod.rs:176, 215](file:///workspace/src/daemon/mod.rs) |
| 10 | pool | thread 模式 `block_on(task_future)` 嵌套进入 tokio runtime，`task_future` 内 `tokio::spawn` 的延迟入队可能丢失 | [queue.rs:480-484](file:///workspace/src/scheduler/queue.rs) |
| 11 | pool | thread_pool `shutdown` 无法关闭 channel（sender 是 Arc 共享），正在执行 job 的 worker 不会被中断 | [thread_pool.rs:96-102](file:///workspace/src/pool/thread_pool.rs) |
| 12 | pool | thread_pool `Drop` 不 `join` workers，daemon exit 时 worker 被强杀，store 写入半完成 | [thread_pool.rs:105-109](file:///workspace/src/pool/thread_pool.rs) |
| 13 | scheduler | `pending` 队列用 `Vec` + 每次 enqueue `sort_by` O(n log n) + `drain_next` `remove(0)` O(n)，万级任务 CPU 开销显著 | [queue.rs:72-87](file:///workspace/src/scheduler/queue.rs) |
| 14 | scheduler | `OverlapController.running` HashMap 是"死缓存"，写了从不读 | [overlap.rs:17-21, 78-90](file:///workspace/src/scheduler/overlap.rs) |
| 15 | scheduler | `max_instances>1` 在 InMemoryStore/SqliteStore 上实际不生效（单行单 task 模型 count 永远 ≤1） | [in_memory.rs:88-97](file:///workspace/src/store/in_memory.rs) |
| 16 | scheduler | `scan_once` 单 task 失败会中断整批，后续 task 在该 tick 被跳过 | [cron.rs:112-275](file:///workspace/src/scheduler/cron.rs) |
| 17 | executor | Shell stdout/stderr 管道死锁：`read_to_end` 在 `wait` 之后，>64KB 输出导致 wait 永不返回，每次大输出任务硬超时 | [shell.rs:48-49, 146-167](file:///workspace/src/executor/shell.rs) |
| 18 | executor | stdout/stderr 无大小上限，`read_to_end` 读入全部输出，几百 MB 直接 OOM | [shell.rs:152, 158](file:///workspace/src/executor/shell.rs) |
| 19 | executor | `kill_on_drop(true)` 未设，daemon SIGKILL/panic 时孤儿子进程残留 | [shell.rs:41-45](file:///workspace/src/executor/shell.rs) |
| 20 | executor (HTTP) | 每次请求新建 `reqwest::Client`，无连接池复用，高并发耗尽 ephemeral port | [http.rs:117-126](file:///workspace/src/executor/http.rs) |
| 21 | executor (HTTP) | 响应 body 无大小限制，`resp.text().await` 读入内存，恶意上游 OOM | [http.rs:149-150](file:///workspace/src/executor/http.rs) |
| 22 | store (SQLite) | 多语句操作未包事务（delete_task / remove_task），进程崩溃数据不一致 | [sqlite.rs:413-416, 442-448](file:///workspace/src/store/sqlite.rs) |
| 23 | store (SQLite) | 未启用 `PRAGMA foreign_keys=ON`，外键声明不强制 | [sqlite.rs:75-83](file:///workspace/src/store/sqlite.rs) |
| 24 | ipc | `request` 无连接/读写超时，daemon 卡死时 PHP-FPM worker 永久挂住 | [mod.rs:166-181](file:///workspace/src/ipc/mod.rs) |
| 25 | ipc (Unix) | 无对端身份校验（无 `SO_PEERCRED`），同 uid/gid 进程可连 | [unix_socket.rs:30-38](file:///workspace/src/ipc/unix_socket.rs) |
| 26 | lib.rs | 28 个导出函数错误返回风格分裂 4 种（bool / Option / `error:` / KV），PHP 端难统一处理 | [lib.rs 全文](file:///workspace/src/lib.rs) |
| 27 | lib.rs | `xhjob_run_daemon` 作为普通 PHP 函数暴露，任何 PHP 进程调用即永久挂起 | [lib.rs:563-585, 1418](file:///workspace/src/lib.rs) |
| 28 | lib.rs | `with_retry` / `timeout` / `max_instances` 把负数静默转成 u32::MAX/u64::MAX，与 `max_executions`/`soft_timeout` 钳位风格不一致 | [lib.rs:709-712, 719-722, 734-737](file:///workspace/src/lib.rs) |
| 29 | task | `retry_delay=0` 无校验，配合 `retry_backoff=false` 触发 1Hz 重试风暴 | [task/mod.rs:337-341](file:///workspace/src/task/mod.rs) |
| 30 | retry | `is_retryable_shell_exit` 对 127（command not found）/126（permission denied）等永久错误也重试 | [retry/mod.rs:81-83](file:///workspace/src/retry/mod.rs) |
| 31 | retry | `acks_on_failure=false` 无总尝试次数上限，永久失败任务无限重试填满 events 表 | [retry/mod.rs:54-57](file:///workspace/src/retry/mod.rs) |
| 32 | retry | HTTP 429（Too Many Requests）不重试，违反 RFC 6585 | [retry/mod.rs:76-78](file:///workspace/src/retry/mod.rs) |
| 33 | utils | `XHJOB_MAX_MEMORY_PER_CHILD` 单位是字节，运维直觉是 MB，误设导致 daemon 启动后立即触发关闭 | [limits.rs:44-48](file:///workspace/src/utils/limits.rs) |
| 34 | outcome | `StateInfo::from_task` 手工 30 字段拷贝，`xhjob_state` 漏透 `replace_existing` | [outcome/mod.rs:83-116](file:///workspace/src/outcome/mod.rs) + [lib.rs:180-210](file:///workspace/src/lib.rs) |
| 35 | outcome | `Interrupted` 状态无自动恢复路径（acks_late 只覆盖 Running），daemon 崩溃后 Interrupted 任务永久卡死 | [store/mod.rs:89-91](file:///workspace/src/store/mod.rs) |

### P2：代码异味、可读性、命名、文档（节选）

| # | 问题 | 位置 |
|---|---|---|
| 36 | `coroutine_pool` 命名误导（Rust 无协程） | [coroutine_pool.rs:8-12](file:///workspace/src/pool/coroutine_pool.rs) |
| 37 | `spawn_via_double_fork` 实为单次 spawn + setsid，命名误导 | [unix.rs:78-92](file:///workspace/src/daemon/unix.rs) |
| 38 | 默认落 `/tmp`，受 systemd-tmpfiles 周期清理，daemon 长跑后 socket 可能被清掉 | [mod.rs:64](file:///workspace/src/daemon/mod.rs) + [ipc/mod.rs:50](file:///workspace/src/ipc/mod.rs) |
| 39 | `rand_jitter` 返回 `[0, secs)` 半开区间，文档写 `[0, secs]` 闭区间 | [task/mod.rs:197-203](file:///workspace/src/task/mod.rs) + [cron.rs:330-336](file:///workspace/src/scheduler/cron.rs) |
| 40 | `task_json` / `tasks_json` 等无大小限制，超大 JSON 直接 OOM | [lib.rs:126, 1140, 1226, 1316](file:///workspace/src/lib.rs) |
| 41 | `xhjob_state` 把 `None` 字段字符串化成 `"null"`，PHP 端需 `=== 'null'` 判断 | [lib.rs:190-210](file:///workspace/src/lib.rs) |
| 42 | `xhjob_state` 的 `tags` 字段返回 JSON 字符串而非嵌套数组 | [lib.rs:203](file:///workspace/src/lib.rs) |
| 43 | `reopen_std_streams_for_daemon` 裸 `dup2` 忽略返回值，项目已依赖 nix | [lib.rs:604-609](file:///workspace/src/lib.rs) |
| 44 | `RetryPolicy::delay_for_attempt` 与 `backoff_delay` 的 cap 不一致（1h 绝对 vs retry_delay×60 相对） | [retry/mod.rs:40-48 vs 109-123](file:///workspace/src/retry/mod.rs) |
| 45 | `WorkerLimits::default()` 在 Default impl 里读环境变量是隐藏副作用 | [limits.rs:37-55](file:///workspace/src/utils/limits.rs) |
| 46 | env var 解析失败静默回退到 0（unlimited） | [limits.rs:39-48](file:///workspace/src/utils/limits.rs) |
| 47 | `ServiceName` newtype 未端到端使用，类型安全未生效 | [service/mod.rs:39-58](file:///workspace/src/service/mod.rs) |
| 48 | `chain.rs::mark_failed` 用 `current_step=0`，丢失失败发生在哪一步的信息 | [chain.rs:58](file:///workspace/src/scheduler/chain.rs) |
| 49 | `notify_chain_and_group` 通过解析 task.meta JSON 字符串提取 chain_id/group_id，chord_id 用专门字段，两套机制不一致 | [queue.rs:547-553, 580-593](file:///workspace/src/scheduler/queue.rs) |
| 50 | `HttpPayload.headers: HashMap<String, String>` 不能表示重复头，且大小写敏感 | [task/mod.rs:298](file:///workspace/src/task/mod.rs) |

---

## 四、待完善功能清单

### 未实现 / stub

| # | 功能 | 现状 | 建议 |
|---|---|---|---|
| 1 | chain/chord 重启恢复 | daemon 启动只 reset Running task，不处理 chain/chord | 启动时扫描 state=running 的 chain/chord，检查 current_step 已完成则 advance，chord 检查 header 全完成则 dispatch callback |
| 2 | `max_instances>1` 真正并发 | InMemoryStore/SqliteStore 单行单 task，count 永远 ≤1 | 改造 store 支持 per-instance 行或独立 running 计数表 |
| 3 | 分布式支持 | 单 daemon，无跨节点协调 | 未来可考虑基于 SQLite WAL 的多读单写或引入 Raft |
| 4 | chord 持久化 | chord 状态仅在内存 events 表 | 持久化 chord_record 表 |
| 5 | HTTP 429 Retry-After | 不重试 429 | 读 Retry-After 头作为下次重试延迟 |
| 6 | shell 退出码 denylist | 所有非零都重试 | 加 `{126, 127}` denylist 或可配置 `retryable_exit_codes` |
| 7 | acks_on_failure=false 总上限 | u32::MAX 无限重试 | 加 `max_total_retry_seconds` 或 `max_retry_count` 兜底 |
| 8 | HTTP 连接池复用 | 每请求新建 Client | Client 单例化或 OnceCell |
| 9 | shell stdout/stderr 流式 drain | wait 后才 read，管道死锁 | `tokio::join!` 并发 wait + drain |
| 10 | shell 输出大小限制 | read_to_end 无上限 | `max_output_bytes` 截断 |
| 11 | SQLite 连接池 | 单 Mutex<Connection> 串行 | r2d2-sqlite / deadpool-sqlite |
| 12 | IPC keep-alive | 短连接模型 | 支持多请求复用连接 |
| 13 | IPC 连接数上限 | 无限制 | 加 Semaphore 限流 |
| 14 | 背压机制 | pending 队列 unbounded Vec | 加上限 + 拒绝入队 |
| 15 | `xhjob_run_daemon` 访问控制 | 普通 PHP 函数暴露 | 加 `php_sapi_name() === 'cli'` 守卫 |
| 16 | `withId` 字符集校验 | 接受任意字符串 | `^[A-Za-z0-9_-]{1,64}$` |
| 17 | payload schema 校验 | `serde_json::Value` 无约束 | enum + tag 强约束 |
| 18 | `Interrupted` 自动恢复 | 黑洞状态 | daemon 启动时 reset Interrupted → Pending |
| 19 | `xhjob_state` 补 `replace_existing` | 漏透 | 补字段 + 反射测试守卫 |
| 20 | Windows Named Pipe 安全描述符 | 默认 DACL | SecurityAttributes 限制 SID |

### 增强建议

- **监控**：暴露 Prometheus metrics（任务数、状态分布、重试次数、执行时长）
- **告警**：长期 running 的 chain/chord 告警
- **PHP SDK**：提供 `via_shell_safe()` 参数分离接口（不走 `bash -c`）
- **资源隔离**：shell 任务运行用户降权 + cgroup/namespace 隔离
- **rlimit**：spawn 前 setrlimit（CPU、内存、文件数）

---

## 五、生产环境特别关注点

### 运维风险 Top 5

1. **socket fd 泄漏 → daemon 静默不可用**（P0）：PHP-FPM worker 被 kill -9/OOM/max_execution_time 强杀制造半连接，daemon 端 tokio task 永久阻塞在 read_exact，fd 累积到达 ulimit -n 后无法 accept 新连接。**监控**：`ls /proc/<pid>/fd | wc -l` 设告警。

2. **chain/chord 重启丢失**（P0）：生产应避免在 chain/chord 执行期间重启 daemon。**监控**：告警长期 running 的 chain/chord。

3. **`xhjob_stop` 阻塞 FPM worker 10 秒**（P1）：高负载下可能耗尽 FPM worker 池。**建议**：禁止在 Web 请求路径调用 stop，改为 cron/supervisor 触发。

4. **`/tmp` 被 tmpfiles 清理**（P2）：生产应强制设置 `XHJOB_DATA_DIR` 指向持久目录。

5. **daemon 达到 max_tasks_per_child 后退出**（P2）：依赖 supervisor 重启。生产必须配 systemd unit `Restart=always`。

### 安全风险 Top 3

1. **Shell 命令注入**（设计性 P0）：`bash -c <cmd>` 直接执行 PHP 传入的 cmd，业务代码拼外部输入即 RCE。**缓解**：PHP SDK 文档强制警告；daemon 侧可选 cmd 模式（参数分离）；运行用户降权；cgroup 隔离。

2. **Windows Named Pipe 默认 DACL + 远程可连**（P0）：任意用户甚至远程机器可连接执行 shell 命令。**修复**：加 SecurityAttributes + `reject_remote_clients(true)`。

3. **Unix socket 无 peer cred 校验**（P1）：同 uid/gid 进程可连。**修复**：accept 后用 `getsockopt(SO_PEERCRED)` 校验 uid。

### 性能风险 Top 3

1. **SQLite 单 Mutex<Connection> 串行化**（P0）：PHP-FPM 几十个 worker 并发 xhjob_state 轮询会显著放大延迟。**建议**：读多写少场景用 read-only 连接池。

2. **pending 队列 Vec O(n) 操作**（P1）：万级任务时 CPU 开销显著且持锁时间长。**建议**：换 BinaryHeap。

3. **scan_once 每秒全表读**（P1）：任务数到万级时与 process_one 的 store 写争锁，tick 延迟 > 1s，cron 触发不准时。

---

## 六、改进建议优先级汇总

### 立即修复（P0，影响生产稳定/安全）

1. [lib.rs:129](file:///workspace/src/lib.rs) `return e` → `return format!("error: {}", e)`
2. [lib.rs:896-899](file:///workspace/src/lib.rs) `Xhjob::id()` 加字符集校验
3. [daemon_main.rs:247-286](file:///workspace/src/daemon_main.rs) + [ipc/mod.rs](file:///workspace/src/ipc/mod.rs) IPC 加读写超时
4. [chain.rs](file:///workspace/src/scheduler/chain.rs) + [chord.rs](file:///workspace/src/scheduler/chord.rs) 启动时恢复推进逻辑
5. [named_pipe.rs:16-19](file:///workspace/src/ipc/named_pipe.rs) Windows 加安全描述符 + reject_remote_clients
6. [sqlite.rs:19-22](file:///workspace/src/store/sqlite.rs) `PRAGMA busy_timeout=5000`
7. [sqlite.rs:9-11](file:///workspace/src/store/sqlite.rs) 引入连接池或读写分离

### 近期修复（P1，影响功能/资源）

8. shutdown 时通知并 reap 所有在途 shell 子进程
9. `pending` 队列换 BinaryHeap + 加背压上限
10. `scan_once` per-task 错误隔离
11. Shell 执行器并发 drain stdout/stderr（tokio::join!）
12. Shell 执行器 stdout/stderr 加 max_output_bytes 截断
13. HTTP 执行器 Client 单例化 + 响应 body 大小限制
14. SQLite 多语句操作包事务
15. `with_retry` 加 `delay = delay.max(1)` 钳位
16. `is_retryable_shell_exit` 加 `{126, 127}` denylist
17. HTTP 429 加入重试集合
18. `XHJOB_MAX_MEMORY_PER_CHILD` 支持 M/G 后缀
19. `xhjob_state` 补 `replace_existing` 字段
20. `Interrupted` 状态自动恢复

### 后续优化（P2，代码质量）

21. 统一错误返回风格（全用 `error:` 字符串或全用异常）
22. 超大 JSON 大小限制
23. `xhjob_state` 的 `null` 字符串改省略键
24. `reopen_std_streams_for_daemon` 用 nix::unistd::dup2
25. `coroutine_pool` 重命名为 `async_pool`（保持兼容别名）
26. 默认数据目录从 `/tmp` 改为 `/var/lib/xhjob`
27. `ServiceName` newtype 端到端使用
28. payload schema 校验（enum + tag）

---

## 七、第二轮深挖：新发现的问题

### cron.rs 深度审查新增

#### P0（数据丢失）

| # | 问题 | 位置 |
|---|---|---|
| P0-8 | cron 无未来匹配时 roll-forward 失败，next_fire 不更新，任务每 tick 重复 enqueue | [cron.rs:259-272](file:///workspace/src/scheduler/cron.rs) |
| P0-9 | `run_at` 任务在 enqueue 后立即标记 `Success`，process_one 跳过未执行任务 | [cron.rs:178-191](file:///workspace/src/scheduler/cron.rs) |
| P0-10 | `max_executions` 与 Running 任务竞态，高频 cron 下实际执行次数超出上限 | [cron.rs:118-124](file:///workspace/src/scheduler/cron.rs) |

#### P1（功能缺陷）

| # | 问题 | 位置 |
|---|---|---|
| P1-36 | scan_once 不检查 `state == Running`，长任务运行期间被重复 enqueue | [cron.rs:216-274](file:///workspace/src/scheduler/cron.rs) |
| P1-37 | 不可能日期（2月30日/4月31日）解析成功但 next_fire 永远返回 None，任务成僵尸 | [cron.rs:56-57](file:///workspace/src/scheduler/cron.rs) |
| P1-38 | DST 切换时触发可能重复或丢失，`timestamp_opt().single()` 对 Ambiguous/NonExistent 不健壮，全文件无 DST 测试 | [cron.rs:63-64, 70-71](file:///workspace/src/scheduler/cron.rs) |
| P1-39 | roll-forward 从 `now + 1` 而非 `next` 重算，lag 较大时跳过中间 occurrence | [cron.rs:259](file:///workspace/src/scheduler/cron.rs) |
| P1-40 | 全文 9 处 `let _ =` 静默吞掉 store 写入错误，update_next_fire 失败导致重复 enqueue | [cron.rs:121/135/142/168/183/211/250/265/280](file:///workspace/src/scheduler/cron.rs) |
| P1-41 | scan_once 无 retry 任务分支，无 cron/interval/run_at 的任务被静默忽略成孤儿 | [cron.rs:178-274](file:///workspace/src/scheduler/cron.rs) |

#### P2（代码异味）

| # | 问题 | 位置 |
|---|---|---|
| P2-51 | `LAST_CLEANUP_TS` 是全局 static，多实例互相干扰 | [cron.rs:15](file:///workspace/src/scheduler/cron.rs) |
| P2-52 | 多任务同时 due 无 priority / created_at 排序 | [cron.rs:114-115](file:///workspace/src/scheduler/cron.rs) |
| P2-53 | tick 间隔 1s 不对齐墙钟秒边界，秒级 cron 触发抖动最大 ~1s | [cron.rs:296](file:///workspace/src/scheduler/cron.rs) |
| P2-54 | `now_ts()` 在 scan_once 内被多次独立调用，时间快照不一致 | [cron.rs:113/121/139/187/255](file:///workspace/src/scheduler/cron.rs) |
| P2-55 | `max_executions` / `end_date` 转 `Success` 不记录事件，与 `Expired` 路径不一致 | [cron.rs:118-124, 165-172](file:///workspace/src/scheduler/cron.rs) |
| P2-56 | `Missed` 事件时间戳用 `now`（检测时刻）而非 `next`（应触发时刻） | [cron.rs:254](file:///workspace/src/scheduler/cron.rs) |
| P2-57 | `start_date` 之后首次 next_fire 从 `now` 计算，跳过 start_date 与 now 之间的 occurrence | [cron.rs:158-162, 221-235](file:///workspace/src/scheduler/cron.rs) |
| P2-58 | `jitter` 可能大于 cron 间隔，导致下一次 occurrence 被跳过 | [cron.rs:262-264](file:///workspace/src/scheduler/cron.rs) |
| P2-59 | `load_active_tasks` 每 tick 全表扫描，无 `next_fire <= now` 过滤 | [cron.rs:114](file:///workspace/src/scheduler/cron.rs) |
| P2-60 | scan_once 自身无并发互斥，可被外部并发调用造成重复 enqueue | [cron.rs:112](file:///workspace/src/scheduler/cron.rs) |
| P2-61 | L/W/# 扩展语法不支持且无明确报错 | [cron.rs:56](file:///workspace/src/scheduler/cron.rs) |

### sqlite.rs / store 深度审查新增

#### P0（数据丢失/安全）

| # | 问题 | 位置 |
|---|---|---|
| P0-11 | chord `refresh_state` 严重 N+1：N 个 header = 最多 2N 次 store 调用 | [chord.rs:99-126](file:///workspace/src/scheduler/chord.rs) |
| P0-12 | group `summarize` 同样 N+1：10k 任务 = 10k 次串行加锁查询 | [group.rs:39-56](file:///workspace/src/scheduler/group.rs) |
| P0-13 | `chain::advance` 非幂等且 TOCTOU，可重复 dispatch 或跳步 | [chain.rs:31-49](file:///workspace/src/scheduler/chain.rs) |
| P0-14 | 默认 DB 目录 `/tmp` 是 tmpfs（重启即丢）+ 0644 权限（任何用户可读 payload）+ 符号链接攻击 | [store/mod.rs:728-735](file:///workspace/src/store/mod.rs) |
| P0-15 | `save_result` 与 `update_state` 之间崩溃 → 无结果的成功态或孤儿 Running | [queue.rs:234, 291, 374](file:///workspace/src/scheduler/queue.rs) |

#### P1（功能缺陷/资源泄漏）

| # | 问题 | 位置 |
|---|---|---|
| P1-42 | 缺 `tasks(chord_id)` / `chains(state)` / `groups(state)` / `events(task_id,ts)` 索引 | [sqlite.rs:73, 93-116, 91-92](file:///workspace/src/store/sqlite.rs) |
| P1-43 | `load_active_tasks` 无 LIMIT，重启时全量加载 10 万 pending 瞬时占内存 | [sqlite.rs:318-333](file:////workspace/src/store/sqlite.rs) |
| P1-44 | `list_tasks` 的 tag 过滤在 Rust 侧做，无法下推 SQL，全表扫描后丢弃 | [sqlite.rs:529-531, 543-545](file:///workspace/src/store/sqlite.rs) |
| P1-45 | `list_tasks` 返回 `TaskSummary` 却先构造完整 `Task`（解码 47 列含大 JSON 后丢弃） | [sqlite.rs:525-533](file:///workspace/src/store/sqlite.rs) |
| P1-46 | `cleanup_expired_events` 单条大 DELETE 无批量，阻塞所有 store 操作 | [sqlite.rs:722-725](file:///workspace/src/store/sqlite.rs) |
| P1-47 | `list_events` 无 LIMIT，可能瞬时返回数万行 | [sqlite.rs:693-710](file:///workspace/src/store/sqlite.rs) |
| P1-48 | `delete_task` 与 `remove_task` 删除顺序相反且都不清 events，留下孤儿事件 | [sqlite.rs:413-416 vs 442-448](file:///workspace/src/store/sqlite.rs) |
| P1-49 | `insert_task` 用 `INSERT OR REPLACE` 静默覆盖 Running 任务 | [sqlite.rs:252](file:///workspace/src/store/sqlite.rs) |
| P1-50 | `reset_running_to_pending` 不重置 `last_error` / `attempts` / `cancel_requested` | [sqlite.rs:638-646](file:///workspace/src/store/sqlite.rs) |
| P1-51 | 多 daemon 误连同一 DB 文件无隔离，无文件锁 | [store/mod.rs:711-725](file:///workspace/src/store/mod.rs) |
| P1-52 | `Connection::open` 不设文件权限，DB 文件常 0644，payload 含敏感数据 | [sqlite.rs:16](file:///workspace/src/store/sqlite.rs) |
| P1-53 | `count_running_instances` 语义无效：`id` 是 PK，count ≤ 1，max_instances>1 根本无法实现 | [sqlite.rs:376-380](file:///workspace/src/store/sqlite.rs) |
| P1-54 | open 时无 `PRAGMA quick_check`，DB 损坏首次查询才报错 | [sqlite.rs:15-23](file:///workspace/src/store/sqlite.rs) |
| P1-55 | 无备份/恢复 API，必须停 daemon 才能 `cp` DB 文件 | TaskStore trait |
| P1-56 | `update_state` 不校验状态机合法性，可绕过终态校验 | [sqlite.rs:297-303](file:///workspace/src/store/sqlite.rs) |
| P1-57 | 受影响行数检查不一致，多数 update 方法不检查 affected==0 | [sqlite.rs](file:///workspace/src/store/sqlite.rs) 多处 |
| P1-58 | `record_event` 在热路径串行化进全局 mutex，与 `update_state` 抢锁 | [sqlite.rs:658-661](file:///workspace/src/store/sqlite.rs) |
| P1-59 | 终态任务行永不清理，tasks 表无限增长 | [sqlite.rs](file:///workspace/src/store/sqlite.rs) 全文 |
| P1-60 | `Mutex<Connection>` 全局串行，WAL 读并发优势完全浪费 | [sqlite.rs:9-11](file:///workspace/src/store/sqlite.rs) |

#### P2（代码异味）

| # | 问题 | 位置 |
|---|---|---|
| P2-62 | `list_tasks` 的 state_filter 分支只查小写，漏大写旧数据 | [sqlite.rs:522-526](file:///workspace/src/store/sqlite.rs) |
| P2-63 | `task_from_row` 多处静默吞错（type 未知→Shell，state 未知→Pending resurrect） | [sqlite.rs:178-180, 182, 185, 230-233](file:///workspace/src/store/sqlite.rs) |
| P2-64 | 历史大写 state 值未迁移，查询永远带双 case | [sqlite.rs:323, 377, 644, 984](file:///workspace/src/store/sqlite.rs) |
| P2-65 | `events.id` 用 AUTOINCREMENT 有额外开销 | [sqlite.rs:85](file:///workspace/src/store/sqlite.rs) |
| P2-66 | prepare 未缓存，热路径每秒数百次 prepare | [sqlite.rs](file:///workspace/src/store/sqlite.rs) 全文 |
| P2-67 | WAL checkpoint 时机不受控，daemon 退出无 TRUNCATE | [sqlite.rs:19-22](file:///workspace/src/store/sqlite.rs) |
| P2-68 | `chains.tasks` / `groups.tasks` 存大 JSON blob，每次全量解析 | [sqlite.rs:95, 103, 111](file:///workspace/src/store/sqlite.rs) |

### PHP 客户端契约不一致

#### P0（核心功能失效）

| # | 问题 | 位置 |
|---|---|---|
| P0-16 | **`withProxy()` 完全失效**：PHP 写入 `payload.proxy`，Rust 读 `task.proxy`（顶层），代理设置被静默丢弃 | [TaskBuilder.php:128-134, 416-423](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) + [task/mod.rs:54-56](file:///workspace/src/task/mod.rs) |
| P0-17 | **HTTP 空 headers 反序列化失败**：PHP `json_encode([])` 产生 `[]`（数组），Rust `HashMap` 期望 `{}`（对象），所有默认空 headers 的 HTTP 任务执行时失败 | [TaskBuilder.php:131](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) + [store/mod.rs:102-103](file:///workspace/src/store/mod.rs) |
| P0-18 | **`encoding` 字段 PHP 端无法设置**：Rust 有 `withEncoding()` 但 PHP TaskBuilder 无此方法 | [lib.rs:688-692](file:///workspace/src/lib.rs) + [TaskBuilder.php](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) |

#### P1（功能缺陷）

| # | 问题 | 位置 |
|---|---|---|
| P1-61 | `state()` / `result()` 不检查 `error` 键，daemon 错误被静默吞掉 | [TaskManager.php:238-243, 252-256](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php) |
| P1-62 | `get()` 的 `error:` 检查是死代码，Rust `xhjob_get` 返回 Option 从不返回 `error:` 字符串 | [TaskManager.php:196-201](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php) + [lib.rs:504-539](file:///workspace/src/lib.rs) |
| P1-63 | `status()` 忽略 `error` 键，无效服务名与 daemon 未运行不可区分 | [XhjobService.php:144-155](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/XhjobService.php) |
| P1-64 | `Client::callWithRetry()` 对 daemon 错误无效，`InvalidTaskConfigException` 不重试 | [Client.php:356-358](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/Client.php) |
| P1-65 | `events()` 传空字符串作 task_id 过滤，应传 null 不过滤 | [Client.php:203-208](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/Client.php) |
| P1-66 | `max_instances: 0` 未经 PHP 端归一化，任务可能永远不被调度 | [TaskBuilder.php:299-303](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) |

#### PHP 客户端 Bug

| # | 问题 | 位置 |
|---|---|---|
| Bug-1 | `fromJson()` JSON 解码失败时静默清空配置 | [TaskBuilder.php:199-205](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) |
| Bug-2 | `update()` 修改传入的 TaskBuilder（副作用） | [TaskManager.php:163-174](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php) |
| Bug-3 | `callWithRetry()` 对 `TaskNotFoundException` 无意义重试 | [Client.php:359-365](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/Client.php) |
| Bug-4 | `start()` 忽略 `wait()` 返回值 | [XhjobService.php:85](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/XhjobService.php) |
| Bug-5 | `xhjob_task()` 在 `$service=null` 时丢弃 `$dataDir` | [helper.php:46-54](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/helper.php) |
| Bug-6 | PHP 端无符号整数字段无负值校验 | [TaskBuilder.php](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) 多处 |
| Bug-7 | `withId('')` 传空字符串而非 null | [TaskBuilder.php:575-579](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) |
| Bug-8 | `parseResponse()` 将 daemon 错误误分类为配置错误 | [TaskManager.php:565-574](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php) |

---

## 八、可增加的优秀功能（对标业界方案）

### 第一优先级：可靠性闭环（改动小，价值极高）

| # | 功能 | 对标 | 实现思路 |
|---|---|---|---|
| F1 | **死信队列（DLQ）+ 失败任务归档表** | Sidekiq DLQ / Symfony failure_transport / Laravel failed_jobs | 新增 `failed_jobs` 表，重试耗尽时原子迁移；提供 `failedList/retryFailed/purgeFailed` API |
| F2 | **心跳保活（Heartbeat）+ 长任务防误杀** | Temporal Heartbeating / Laravel retry_after | 复用进度上报通道作心跳，调度器据此动态延长硬超时窗口 |
| F3 | **调度器身份锁 + Cron 防重复执行** | APScheduler acquire_schedules / Hangfire 分布式锁 | SQLite `scheduler_lock` 表，拉取 cron 前 INSERT OR IGNORE 抢锁 |
| F4 | **重试策略增强（按异常决策 + 耗尽回调 + retry_for）** | Sidekiq sidekiq_retry_in / Temporal Retry Policy | 重试策略支持按异常类决定 retry/kill/discard + onExhausted 回调 + retry_for 时间窗口 |

### 第二优先级：可观测性与可扩展性（中等改动）

| # | 功能 | 对标 | 实现思路 |
|---|---|---|---|
| F5 | **内嵌 Web Dashboard（零依赖 HTTP 监控）** | Sidekiq Web UI / Hangfire Dashboard | Rust 侧用 hyper/tiny_http 起可选 HTTP server，JSON API + 单文件 HTML |
| F6 | **多队列 + 任务路由 + 队列优先级/权重** | Celery task_routes / Sidekiq -q / Hangfire 多队列 | tasks 表加 queue 字段，调度器按权重加权轮询，路由规则按任务名 glob |
| F7 | **任务过滤器管道（Filter Pipeline / Middleware）** | Hangfire IServerFilter / Symfony Messenger middleware | PHP 端 Filter 接口（onDispatch/onExecuting/onExecuted/onError），内置 DisableConcurrentExecution |

### 第三优先级：编排能力跃升（中长期）

| # | 功能 | 对标 | 实现思路 |
|---|---|---|---|
| F8 | **信号/外部输入等待（Signal-like）** | Temporal Signal / Hangfire Continuations | 任务 `awaitSignal($name, $timeout)` 置 WAITING 状态，外部 `signal($jobId, $name, $payload)` 唤醒续跑 |
| F9 | **任务延续（Continuations / 动态依赖）** | Hangfire ContinueWith / Temporal Child Workflow | `continueWith($parentId, fn($result) => Job::...)`，父完成事件触发回调动态派发子任务 |
| F10 | **ContinueAsNew / 长任务分片（防历史膨胀）** | Temporal ContinueAsNew | 任务声明 `continueAsNew()`，完成后以新 job_id 派发下一轮，旧记录按 retention 归档 |

### 不适合引入的功能

| 功能 | 原因 |
|---|---|
| Redis/MongoDB Jobstore | 违背零依赖定位 |
| Event Broker 多节点协作 | 需外部 broker，违背零依赖 |
| Temporal Durable Execution + Replay | 架构大改，PHP 非确定性约束不友好 |
| Celery 远程控制 broadcast | 依赖 broker，可用 Dashboard + inspect 替代 |
| eventlet/gevent 协程 | 已有 tokio async M:N 池 |
| 可视化工作流设计器（BPMN） | 超出"代码驱动队列"定位 |

---

## 九、第二轮改进建议优先级汇总

### 立即修复（P0，影响核心功能）

1. [TaskBuilder.php:128-134](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) `withProxy()` 位置错配 → 写入顶层 `proxy` 而非 `payload.proxy`
2. [TaskBuilder.php:131](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) 空 headers 用 `(object)[]` 或 `new \stdClass()` 产生 `{}`
3. [TaskBuilder.php](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php) 新增 `withEncoding()` 方法
4. [cron.rs:259-272](file:///workspace/src/scheduler/cron.rs) cron 无未来匹配时标记终态并记录事件
5. [cron.rs:178-191](file:///workspace/src/scheduler/cron.rs) run_at 任务不在 enqueue 时标记 Success，由 process_one 完成
6. [cron.rs:118-124](file:///workspace/src/scheduler/cron.rs) max_executions 竞态，enqueue 前检查 inflight
7. [chord.rs:99-126](file:///workspace/src/scheduler/chord.rs) + [group.rs:39-56](file:///workspace/src/scheduler/group.rs) N+1 改批量 SQL
8. [chain.rs:31-49](file:///workspace/src/scheduler/chain.rs) advance 加乐观锁 `WHERE current_step=?`
9. [store/mod.rs:728-735](file:///workspace/src/store/mod.rs) 默认 DB 目录移出 `/tmp`，文件权限 0600
10. [queue.rs:234, 291, 374](file:///workspace/src/scheduler/queue.rs) 提供 `complete_task(id, state, result)` 原子接口

### 近期修复（P1，功能/性能/安全）

11. [cron.rs:216-274](file:///workspace/src/scheduler/cron.rs) scan_once 检查 `state == Pending` 守卫
12. [cron.rs:56-57](file:///workspace/src/scheduler/cron.rs) 不可能日期创建时预演校验
13. [cron.rs:63-64](file:///workspace/src/scheduler/cron.rs) DST 显式处理 + 回归测试
14. [cron.rs:178-274](file:///workspace/src/scheduler/cron.rs) 增加 retry 任务显式分支
15. [sqlite.rs](file:///workspace/src/store/sqlite.rs) 添加 chord_id/chains state/groups state/events 复合索引
16. [sqlite.rs:722-725](file:///workspace/src/store/sqlite.rs) cleanup_expired_events 分批 DELETE
17. [sqlite.rs:252](file:///workspace/src/store/sqlite.rs) insert_task 改 INSERT 显式冲突
18. [sqlite.rs:638-646](file:///workspace/src/store/sqlite.rs) reset_running_to_pending 重置 last_error/attempts/cancel_requested
19. [sqlite.rs:9-11](file:///workspace/src/store/sqlite.rs) 引入连接池释放 WAL 读并发
20. [TaskManager.php:238-243](file:///workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php) state()/result() 检查 error 键

### 后续优化（P2 + 新功能）

21. 一次性迁移 `UPDATE tasks SET state=lower(state)` 后清除双 case
22. `task_from_row` 静默吞错改 tracing::warn!
23. prepare 缓存
24. 终态任务 TTL 清理 / 归档表
25. WAL checkpoint TRUNCATE on shutdown
26. 实现 F1-F4 可靠性闭环功能
27. 实现 F5-F7 可观测性功能
28. 实现 F8-F10 编排能力

---

## 十、第三轮深挖：5 维度并行审查

> 第三轮启动 5 个并行子代理分别从「并发原语 / ext-php-rs FFI / HTTP-Shell 执行器 / 错误处理与可观测性 / 安全与 DoS」五个全新维度深挖，发现新问题共 **95 项**（去重后）。下面按维度分组，并标注与第一/二轮的交叉点。

### 维度 A：并发原语正确性（14 项）

#### A.1 【P0】chord `refresh_state` check-then-act 竞态，callback 可重复派发

- **位置**：[chord.rs:66-164](file:///workspace/src/scheduler/chord.rs) + [queue.rs:601-633](file:///workspace/src/scheduler/queue.rs)
- **问题**：`refresh_state` 的"读取 chord 记录 → 遍历 header 状态 → 判定全成功 → 派发 callback → 标记 success"序列**不是原子的**。多个 header task 并发完成时会并发调用 `refresh_state`，两个并发调用可能同时读到 chord 仍为 `pending`/`running`，各自 `callback.build()` 生成**不同 callback_task_id**并 `insert_task`，最终 callback 被执行**两次**。
- **修复方向**：用 SQLite 条件 UPDATE 做 CAS（`UPDATE chords SET state='success' WHERE id=? AND state!='success'`，affected_rows>0 才派发）；InMemory 用 `tokio::sync::Mutex` 按 chord_id 分片锁。

#### A.2 【P0】SqliteStore 在 tokio worker 线程上执行同步阻塞 I/O

- **位置**：[sqlite.rs:10,246+](file:///workspace/src/store/sqlite.rs)
- **问题**：`SqliteStore` 用 `tokio::sync::Mutex<Connection>`，每方法 `lock().await` 后执行**同步阻塞**的 rusqlite 调用（`execute`/`prepare`/`query_map`）。多线程 runtime 下若 `num_cpus` 个并发请求同时访问 store，**所有 tokio worker 线程被阻塞**，IPC accept / cron tick / queue loop 全部饿死（不是死锁，但严重可用性风险）。
- **修复方向**：(a) `tokio::task::spawn_blocking` 包裹所有 rusqlite 操作；(b) 或改 `deadpool-sqlite` / `r2d2`；(c) 或专用线程 + channel。

#### A.3 【P0】`std::process::exit` 跳过 Drop，SQLite 连接未正常关闭

- **位置**：[queue.rs:734](file:///workspace/src/scheduler/queue.rs) + [daemon_main.rs:42](file:///workspace/src/daemon_main.rs)
- **问题**：`record_worker_limits` 在 `max_tasks_per_child`/`max_memory_per_child` 触达时 `tokio::spawn` 一个 500ms 后 `exit(0)` 的任务。`exit()` 不运行 Drop，`Arc<Mutex<Connection>>` 不会 drop，SQLite 连接不正常关闭（WAL 不 checkpoint），所有 in-flight `task_future` 被强制中断，任务状态停留在 `Running`。
- **修复方向**：改为通过 `shutdown_tx` 通知主循环优雅退出，主循环 break 后 drop store + listener。

#### A.4 【P0】Rust panic 穿越 `extern "C"` handler = UB

- **位置**：[coroutine_pool.rs:88](file:///workspace/src/pool/coroutine_pool.rs) `.expect("failed to build tokio runtime")`、[thread_pool.rs:42/69](file:///workspace/src/pool/thread_pool.rs) `assert!/.expect` + [Cargo.toml:43-45](file:///workspace/Cargo.toml) `[profile.release]` **未设** `panic = "abort"`
- **问题**：ext-php-rs 0.15 的 `try_catch` 对 Rust panic 是 `resume_unwind`（重抛）而非吞掉，`#[php_function]` 生成的 handler 是 `extern "C" ABI`，panic unwinding through `extern "C"` 是**未定义行为**。FPM worker 首次调用 `xhjob_dispatch` 触发 runtime 构建失败时，`expect` panic → UB，常见表现为 segfault 或直接 abort，PHP 端看到 502 而非可处理异常。
- **修复方向**：(a) `Cargo.toml` 加 `panic = "abort"`（最简单稳妥）；(b) 或在 `#[php_function]` 体最外层包 `catch_unwind` 拦截 panic；(c) 把所有 `expect` 改为返回 `Result`。

#### A.5 【P1】FPM worker 被 `block_on(ipc::request)` 永久阻塞

- **位置**：[lib.rs:137/173/233/269/298/...](file:///workspace/src/lib.rs) 共 23 处 `rt.block_on(...)` + [ipc/mod.rs:166-181/104-117](file:///workspace/src/ipc/mod.rs) `request` / `read_frame` 无超时
- **问题**：`ipc::request` 的 `connect` → `write_frame` → `read_frame` 三个 async 操作**都没有 `tokio::time::timeout` 包裹**。daemon 接受连接后卡死不回包时，`read_exact` 永久挂起，`rt.block_on` 阻塞 FPM worker 线程。PHP `max_execution_time` 只在 executor tick 生效，**不打断 C 层阻塞调用**。daemon 异常时 FPM worker 池被逐个耗尽，站点 502/504 且无法自愈。
- **修复方向**：`ipc::request` 外层 `tokio::time::timeout(Duration::from_secs(5-10), async {...})`。

#### A.6 【P1】InMemoryStore 跨 await 持有 RwLock 写锁（多锁嵌套）

- **位置**：[in_memory.rs:122-131/144-154/192-217](file:///workspace/src/store/in_memory.rs)
- **问题**：`delete_task` 持 `tasks` 写锁的同时 `.await` 取 `results` 写锁；`cleanup_expired_results` 持 `tasks` **读锁**遍历 results 写操作，期间所有 tasks 写操作（insert/update/delete）被阻塞。锁序一致（tasks → results，不死锁），但写锁持有时间被 await 拉长。
- **修复方向**：先 `tasks.read()` 快照待删 ID 列表，drop guard 后再 `results.write()` 批删；或合并到同一 `RwLock<StoreData>` 单锁。

#### A.7 【P1】ThreadPool Drop 不 join worker 线程

- **位置**：[thread_pool.rs:96-109](file:///workspace/src/pool/thread_pool.rs)
- **问题**：`shutdown` 仅发信号，从不 join `workers: Vec<JoinHandle<()>>`。Drop 后 worker 线程仍存活（detached），持有的 `Arc<Receiver>` 阻止 channel 关闭，worker 永远阻塞在 `recv`。worker 可能 outlive pool 引用 Arc 已释放的资源。
- **修复方向**：`shutdown` 或 `Drop` 中 `for h in workers.drain(..) { let _ = h.join(); }`。

#### A.8 【P2】OverlapController in-memory `running` map 是"只写不读"死状态

- **位置**：[overlap.rs:20,39-75](file:///workspace/src/scheduler/overlap.rs)
- **问题**：`should_fire` 通过 `store.count_running_instances()` 查 store 判定并发数，**完全不读** `running` map。`on_start/on_finish` 更新 map 但无人消费。in-memory count 与 store count 可能不一致（crash 后 map 丢失）。store 层存在 TOCTOU 窗口（当前因 process_one 串行调用，TOCTOU 不触发，但隐含约束未被代码强制）。
- **修复方向**：让 `should_fire` 优先读 map 快路径，store 兜底；或删除无用 map 简化设计；或文档标注不变量。

#### A.9 【P2】`enqueue` 每次全排序 + `drain_next` `remove(0)`

- **位置**：[queue.rs:72-87](file:///workspace/src/scheduler/queue.rs)
- **问题**：`pending: Mutex<Vec<(i32, String)>>` 每次 `enqueue` 后 `sort_by` O(n log n)，`drain_next` `remove(0)` O(n) 移位。万级任务时显著 CPU 开销。
- **修复方向**：改 `BinaryHeap<(Reverse<i32>, String)>`，enqueue O(log n) + drain O(log n)。

#### A.10 【P2】`AssertUnwindSafe` 闭包 panic 后共享状态可能不一致

- **位置**：[thread_pool.rs:60](file:///workspace/src/pool/thread_pool.rs) `catch_unwind(AssertUnwindSafe(j))`
- **问题**：若 job 闭包持有 `Arc<Mutex<...>>` 并在持锁期间 panic，tokio 锁正常释放（不 poison），但若 job 已修改共享数据结构后 panic，状态可能不一致。`AssertUnwindSafe` 强制断言"安全"，实际仅靠人工约束。
- **修复方向**：保持现状但禁止 job 内部持锁修改共享结构；或缩小 `AssertUnwindSafe` 范围。

#### A.11 【P2】daemon 关闭仅 sleep 200ms，无 in-flight task join

- **位置**：[daemon_main.rs:236-238](file:///workspace/src/daemon_main.rs)
- **问题**：`shutdown_tx.send(true)` + `sleep(200ms)` 即进入 cleanup。已 `spawn` 的 in-flight `task_future` **不检查 shutdown 信号**，200ms 后 daemon 退出时这些 future 被丢弃，任务状态停留 `Running`。
- **修复方向**：维护 `AtomicU64` in-flight 计数，shutdown 后 wait 直到归零或超时；或 task_future 加 `tokio::select!` with shutdown_rx 抢占。

#### A.12-P2~P2-14】 cancel flag `SeqCst` 过度保守、RateLimiter 内部 Arc 冗余嵌套、`LAST_CLEANUP_TS` Relaxed 多 worker 重复清理

详见第二轮报告的 P2 区，第三轮交叉验证一致，无新增。

---

### 维度 B：ext-php-rs FFI 边界（7 项）

#### B.1 【P0】Rust panic 穿越 `extern "C"` handler = UB（同 A.4）

见 A.4，FFI 维度独立确认。

#### B.2 【P1】`Vec<(String, String)>` 在 PHP 端是关联数组，入参错传数字索引数组失败

- **位置**：[lib.rs:105/158/219/658](file:///workspace/src/lib.rs) `xhjob_status/state/result` + `with_headers`
- **问题**：ext-php-rs 0.15 把 `Vec<(K, V)>` 转 `ZendHashTable.insert(k, v)`，PHP 端拿到的是**关联数组**。若 PHP 端 `with_headers` 误传数字索引数组 `[['Content-Type','application/json']]`，`TryFrom<ArrayKey> for String` 因 `ArrayKey::Long` 报错，整个 `from_zval` 返回 `None`，PHP 端抛晦涩 `Exception`。
- **修复方向**：PHP stub / README 明确 `withHeaders(array $headers)` 必须是关联数组；或 `from_zval` 失败时给出明确错误消息。

#### B.3 【P1】`Option<String>` 入参对非字符串静默退化为 None

- **位置**：[lib.rs:126](file:///workspace/src/lib.rs) `xhjob_dispatch(name: Option<String>)`、[lib.rs:376](file:///workspace/src/lib.rs) `xhjob_list` 等所有 `name/data_dir: Option<String>`
- **问题**：derive 宏对 nullable 参数 accessor 是 `.val()` 返回 `Option<Option<String>>` 折叠为 `Option<String>`。PHP 传整数/数组/对象时 `Zval::string()` 返回 `None`，`Option<String>` 得 `None`，被当作"未传参"走默认值。
- **影响**：`xhjob_dispatch('{"..."}', 123, '/tmp')`（第二参误传 int）不会报错，而是把 `name` 当作 `None` 路由到 `default` 服务，行为静默偏离预期。
- **修复方向**：关键参数显式校验类型；或 PHP stub 标注 `?string` 依赖 phpstan/psalm。

#### B.4 【P1】`write_frame` 帧长 `as u32` 截断

- **位置**：[ipc/mod.rs:93](file:///workspace/src/ipc/mod.rs) `let len = json.len() as u32;`
- **问题**：若 JSON 序列化后 > 4GiB，`as u32` 静默截断，写入错误长度前缀破坏帧协议。对端 `read_frame` 的 64MiB 上限会拒绝，但发送端自身截断属于协议层隐患。
- **修复方向**：`u32::try_from(json.len()).map_err(|_| XhjobError::Ipc("frame too large".into()))?`。

#### B.5 【P1】`i64 as u32/u64/i32` 整数入参静默回绕

- **位置**：[lib.rs:709/719/724/734/758/765/799/870](file:///workspace/src/lib.rs) `with_retry/timeout/priority/max_instances/start_at/end_at/run_at/soft_timeout`
- **问题**：PHP `int`（64 位下 `zend_long = i64`）直接 `as u32/u64`，负数与大数静默回绕。`withRetry(-1, -1)` → `max = u32::MAX`（约 42 亿次重试）、`delay = u64::MAX`（约 5849 亿年延迟）；`timeout(-1)` → `u64::MAX`（永不超时）；`priority(PHP_INT_MAX)` 经 `as i32` 高位截断后可能变负。第一轮 P1-28 已提"静默转 u32::MAX"，但第三轮给出完整方法清单和 PHP 端精确影响。
- **修复方向**：统一用 `u32::try_from`/`u64::try_from` 拒绝越界值并抛 `PhpException`，或归一化到 `clamp(0, 上限)`。

#### B.6 【P2】错误经 `"error:"` 字符串前缀返回，契约脆弱

- **位置**：[lib.rs:126/376/963/1094/1139/1225/1315](file:///workspace/src/lib.rs) 共 8 个函数
- **问题**：宏从 Rust 签名自动生成 PHP arginfo，参数/返回类型契约一致，但 xhjob 的错误处理约定是"返回字符串前缀 `error:`"，PHP 端必须 `str_starts_with($r, 'error:')` 字符串匹配。若用户传入的显式 id（`task/mod.rs::build`）以 `"error:"` 开头，会被误判为错误。
- **修复方向**：长期建议改为 `PhpResult<String>`（ext-php-rs 支持 `Result<T, PhpException>` 自动 throw），PHP 端用 try/catch；短期文档明确约定 + 在 `build` 校验禁止 `"error:"` 前缀的 id。

#### B.7 【P2】无 RSHUTDOWN 钩子做每请求清理

- **位置**：[lib.rs:1399-1429](file:///workspace/src/lib.rs) `get_module` 未注册 `request_shutdown_function`
- **问题**：当前每请求不分配 Rust 侧 per-request 状态，不泄漏；但若未来接入写文件的 tracing subscriber，文件句柄会跨请求常驻。
- **修复方向**：保持现状；未来引入 per-request 资源时需注册 `request_shutdown_function` 清理。

---

### 维度 C：HTTP/Shell 执行器深挖（25 项）

#### C.1 【P0】POST/PUT/DELETE/PATCH 在 5xx 与网络错误时被无条件重试，重复副作用

- **位置**：[http.rs:108-160](file:///workspace/src/executor/http.rs) + [retry/mod.rs:58-73](file:///workspace/src/retry/mod.rs) + [queue.rs:277-303](file:///workspace/src/scheduler/queue.rs)
- **问题**：`should_retry` 仅按 HTTP 状态码判定（500–599 或 `status_code=None`），**完全不区分方法**。queue.rs 在 5xx 时调 `schedule_retry` 重新发完整 HTTP 请求（含 body）。`acks_on_failure=false` 时甚至 `u32::MAX` 无限重试。
- **攻击场景/生产影响**：转账、扣库存、发短信、调用第三方支付 API 这类 POST 任务，后端偶发 502/503 即被重复执行 N 次（甚至无限次），造成**重复扣款/重复发货**。这是分布式系统里最经典的"非幂等重试"事故源。
- **修复方向**：`should_retry` 按方法区分——GET/HEAD 默认可重试；POST/PUT/PATCH/DELETE 仅在 Task 标记 `idempotent=true` 或带 `Idempotency-Key` 头时才重试；网络错误对非幂等方法默认不重试或仅一次。

#### C.2 【P0】stdout/stderr 管道死锁（第一轮 P1-17 已提，第三轮给出更精确因果）

- **位置**：[shell.rs:48-49/146-162](file:///workspace/src/executor/shell.rs)
- **问题**：`stdout.take()` 后 `read_to_end` 在 `wait_fut` 返回之后才执行。Linux pipe 缓冲区 64KB，子进程写入超 64KB 时 pipe 满、write 阻塞、`child.wait()` 永不返回。即使外层 timeout 兜底 SIGKILL，wait 走 Err 分支直接 return，**stdout/stderr 从头到尾没被 read**，调用方拿不到任何输出。
- **修复方向**：必须**并发**读 + wait，`tokio::try_join!(stdout_fut, stderr_fut, wait_fut)`。

#### C.3 【P1】重定向策略未配置，存在 https→http 协议降级风险

- **位置**：[http.rs:117-126](file:///workspace/src/executor/http.rs)
- **问题**：reqwest 默认 `Policy::default()` 跟随最多 10 次重定向，**允许 https→http 跨协议降级**。MITM 第一跳注入 302 → `http://同域名同路径`，后续请求降级明文，若 `payload.headers` 含 `Authorization` 或 cookie 即被嗅探。
- **修复方向**：`.redirect(reqwest::redirect::Policy::custom(|attempt| { if attempt.url().scheme() != "https" { attempt.error("https-only") } else if attempt.previous().len() >= 5 { attempt.stop() } else { attempt.follow() } }))`。

#### C.4 【P1】响应 body 大小无上限，OOM 风险

- **位置**：[http.rs:149-150](file:///workspace/src/executor/http.rs) `resp.text().await`
- **问题**：直接 `resp.text().await` 读进 `String`，无 `content_length` 预检，无流式上限。恶意/失配服务器返回 10GB 响应，daemon 内存撑爆，tokio runtime OOM panic 全体任务死。即使有 timeout，1Gbps 链路 30s 内也能塞 ~3GB。
- **修复方向**：读前检查 `resp.content_length()`；或 `resp.bytes_stream()` + 计数器，超 `max_body_bytes` 立即 abort。Task 增加 `max_response_bytes` 字段。

#### C.5 【P1】代理认证凭据泄漏到 reqwest URL / 错误消息

- **位置**：[http.rs:82-83, 96-103](file:///workspace/src/executor/http.rs)
- **问题**：对 `http://user:pass@proxy:8080`，代码把**完整 `proxy_str`（含 user:pass）**直接传给 `reqwest::Proxy::http`，又调 `.basic_auth`——凭据被设置两次。reqwest 内部保留带凭据的 URL，会出现在 error Display / tracing span。`format!("parse proxy {}: {}", proxy_str, e)`（96 行）直接把 `proxy_str` 写入 `last_error` 持久化到 sqlite。SOCKS5 分支（84-93 行）已做剥离，**行为不一致**。
- **修复方向**：http/https 分支也按 SOCKS5 方式重建无凭据 URL；96 行错误消息改为只打印 host_port。

#### C.6 【P1】无 URL 协议白名单 + 无 SSRF 防护

- **位置**：[http.rs:138](file:///workspace/src/executor/http.rs) `client.request(method, &payload.url)`
- **问题**：无显式 `Url::parse` + scheme 白名单。reqwest 0.12 不支持 `file://`/`ftp://` 是**隐式**保护，未来若开启 feature 即失守。**完全没有 SSRF 防护**：任务可请求 `http://127.0.0.1:6379/`（Redis）、`http://169.254.169.254/latest/meta-data/`（云元数据，可拿临时凭证）、`http://localhost:9200/_search`（ES 内网）。
- **修复方向**：显式 `Url::parse` + `scheme == "http" || "https"`；解析 host 拒绝私网/环回/链路本地 IP；可选 Task 标记 `allow_internal_url=true` 才放行。

#### C.7 【P1】`max_output_bytes` 完全未实现

- **位置**：[shell.rs:151-152, 157-158](file:///workspace/src/executor/shell.rs) + [store/mod.rs](file:///workspace/src/store/mod.rs) Task 字段无 `max_output_bytes`
- **问题**：`read_to_end` 读全部 stdout/stderr 进内存，无任何上限。`yes` / `cat /dev/urandom` / `find /` / `dd if=/dev/zero of=/dev/stdout` 会撑爆内存 OOM。
- **修复方向**：Task 增加 `max_output_bytes: Option<u64>`（默认 16MB）；用 `tokio::io::AsyncReadExt::take(max)` 截断。

#### C.8 【P1】kill 仅杀直接子进程（bash），不杀进程组 → 孤儿子进程

- **位置**：[shell.rs:74/82/98/128/131/135](file:///workspace/src/executor/shell.rs) + `build_command` (253-266)
- **问题**：`nix::kill(Pid::from_raw(child.id()), SIGTERM)` 只发给 bash PID。`build_command` 没有 `process_group(0)` / `setsid`。bash 启动的子进程（`sleep 1000 &`、`python script.py`、`find | grep` 管道）**不收信号**，bash 死后被 reparent 到 init 继续运行。
- **攻击场景**：长任务被 cancel/timeout 后，子进程孤儿长期驻留，逐步吃满 fd/内存/CPU；外部观察者以为任务已停，实际后台还在跑。
- **修复方向**：spawn 时 `Command::process_group(0)`（Rust 1.64+ std 内建）或 `pre_exec` 里 `setpgid(0,0)`；kill 时 `kill(-pgid, SIGTERM)` 杀整个进程组。

#### C.9 【P1】无 `kill_on_drop` + 无 `PR_SET_PDEATHSIG`，daemon 死后子进程孤儿

- **位置**：[shell.rs:38-45](file:///workspace/src/executor/shell.rs) + [unix.rs:78-82](file:///workspace/src/daemon/unix.rs)
- **问题**：(a) `tokio::process::Command` 默认 `kill_on_drop(false)`，daemon 收 SIGTERM 时 tokio runtime shutdown，in-flight 的 `child.wait()` future 被 drop，但 child **不会**被 kill；(b) 没有 `prctl(PR_SET_PDEATHSIG, SIGKILL)` 兜底；(c) daemon 自身通过 `pre_exec setsid` 成为新会话首，executor 没有对应隔离。
- **修复方向**：(a) `Command::new("bash").kill_on_drop(true)`；(b) Linux `pre_exec` 调 `libc::prctl(PR_SET_PDEATHSIG, SIGKILL)`；(c) daemon 注册 SIGTERM handler，向所有 `cancel_flags` set true + 等 grace 后再退出。

#### C.10 【P1】ExitStatus 信号信息丢失，137/143 不分，崩溃任务被无限重试

- **位置**：[shell.rs:161](file:///workspace/src/executor/shell.rs) `let code = status.code().unwrap_or(-1);` + [retry/mod.rs:67-71](file:///workspace/src/retry/mod.rs)
- **问题**：Unix 上 `ExitStatus::code()` 被信号杀死时返回 `None`，统一映射成 `-1`。无法区分 137（SIGKILL）/143（SIGTERM）/139（SIGSEGV）/134（SIGABRT）。`retry::is_retryable_shell_exit` 把所有非 0 退出码（含 -1）视为可重试，**SIGSEGV/SIGABRT 这种确定性崩溃也会被无限重试**。
- **修复方向**：`#[cfg(unix)] status.signal()` 编码为 `128 + signo`；retry 对 SIGSEGV/SIGABRT/SIGILL 不重试。

#### C.11 【P1】环境变量未清空，子进程继承 daemon 全部 env（含 PATH/密钥）

- **位置**：[shell.rs:38-45](file:///workspace/src/executor/shell.rs) + `build_command` (253-266)
- **问题**：`build_command` 没有 `.env_clear()`、没有重置 `PATH`。子进程继承 daemon 的全部环境变量。任意 shell 任务执行 `env` 即可 dump `DATABASE_URL`、`AWS_SECRET_ACCESS_KEY`、`REDIS_PASSWORD` 等启动脚本注入的密钥。`LD_PRELOAD`/`LD_LIBRARY_PATH`/`BASH_ENV` 等危险变量未过滤。
- **修复方向**：`Command::env_clear()` + 显式注入最小 `PATH`（`/usr/local/bin:/usr/bin:/bin`）+ unset `LD_PRELOAD`/`LD_LIBRARY_PATH`/`BASH_ENV`/`ENV`；ShellPayload 增加 `env: HashMap<String,String>` 白名单。

#### C.12 【P1】工作目录未隔离，子进程在 daemon CWD 运行

- **位置**：[shell.rs:38-45](file:///workspace/src/executor/shell.rs)
- **问题**：子进程继承 daemon 的 CWD（通常是 PHP 项目根 / daemon 启动目录）。shell 任务可 `cd ..` 或直接读写该目录下的文件——配置、sqlite 数据库、PID、log，进而篡改任务定义、注入恶意任务、删除 daemon 锁文件让 daemon 多开。
- **修复方向**：ShellPayload 增加 `cwd: Option<String>`；默认 `current_dir` 设为临时目录或 `/var/lib/xhjob/sandbox/<task_id>`；可选 chroot/pivot_root/mount namespace 隔离。

#### C.13~C.18 【P2】HTTP 侧：Client 不复用 / HTTP 不支持 cancel / URL 含 token 入日志 / 无 connect_timeout / TLS 未显式锁定 / in-flight 未 abort / Authorization 缺显式保护注释

详见对应位置：[http.rs:113-126/117-118/147/117/113-160/139-141](file:///workspace/src/executor/http.rs)。修复方向见第二轮报告同维度。

#### C.19~C.25 【P2】Shell 侧：fd 继承 / 无 uid/gid drop / read 阶段无 timeout / cancel 轮询 200ms 延迟 / grace 可小到 1s / 无 exec 模式（cmd 是 shell 字符串）/ Windows cmd /C 转义复杂

详见 [shell.rs:41-44/38-45/146-162/111-118/57-64/253-266/261-265](file:///workspace/src/executor/shell.rs)。修复方向：ShellPayload 增加 `args: Option<Vec<String>>` + `use_shell: bool`，当 `use_shell=false` 时 `Command::new(cmd).args(args)` 不经 bash。

---

### 维度 D：错误处理一致性 + 可观测性 + 配置管理（30 项）

#### D.1 【P0】tracing subscriber 全程未初始化，77 处 `tracing!` 宏全部 NO-OP

- **位置**：整个 `/workspace/src`
- **问题**：`Cargo.toml` 引入了 `tracing-subscriber`，但 `src/` 全目录搜不到任何 `tracing_subscriber::fmt()` / `EnvFilter` / `try_init` / `set_global_default` 调用。tracing 0.1 默认无 subscriber 时所有 `tracing::info!`/`error!`/`warn!`/`debug!` 宏都是**空操作**。
- **影响**：daemon 内 77 处日志调用全部静默——`daemon_main.rs:56` "daemon starting" banner、`:36` "daemon exited with error"、`queue.rs:244` "task execution failed" 全部不写入任何文件。`lib.rs:581` 调 `reopen_std_streams_for_daemon()` 把 stdout/stderr 重定向到日志文件，但没有 subscriber 输出到 stdout/stderr，**日志文件是空的**。运维对 daemon 状态完全失明。
- **修复方向**：`xhjob_run_daemon` 进入 `daemon_main()` 前调 `tracing_subscriber::fmt().with_writer(io::stderr).with_env_filter(EnvFilter::from_default_env()).try_init()`；支持 `RUST_LOG` 控制级别。

#### D.2 【P0】`let _ = expr` 82 处吞错（含状态机迁移失败）

- **位置**：[queue.rs](file:///workspace/src/scheduler/queue.rs)（30+ 处）、[cron.rs](file:///workspace/src/scheduler/cron.rs)（11 处）、[daemon_main.rs](file:///workspace/src/daemon_main.rs)（10 处）等
- **问题**：`let _ = expr` 共 82 处，绝大多数在丢弃 `Result`。最危险：
  - `queue.rs:114/121/157/166/193/234/258/265/291/297/303/340/347/354/362/374/380/388/427/433/438/447/450` — `update_state`/`record_event`/`save_result`/`set_attempts_and_error`/`schedule_retry`/`enqueue` 的 Result **全部静默丢弃**
  - `cron.rs:121/135/142/168/183/211/250/265/280` — `update_state(Success/Expired)`、`update_next_fire`、`record_event`、`cleanup_expired_results` 全部静默
  - `daemon_main.rs:188` — cron 回调里 `let _ = queue.enqueue(&task_id, priority).await;` 静默丢失**任务入队失败，会丢触发**
  - `daemon_main.rs:314` — `replace_existing` 路径 `let _ = store.delete_task(...)` 静默，删除失败后续 `insert_task` 仍可能成功造成脏数据
  - `daemon_main.rs:412` — `let _ = queue.signal_cancel(task_id).await;` 取消信号丢失
  - `lib.rs:581/595` — `reopen_std_streams_for_daemon` 与 `create_dir_all` 失败静默，daemon 启动后日志写入黑洞
  - `service/mod.rs:99/110` — `OnceLock::set` 失败静默（第二次 set_current 无声丢弃服务名）
- **影响**：状态机迁移失败无任何告警 → 任务卡在 `Running` 假死 → 重试调度失败永久丢失 → cron next_fire 推进失败导致重复触发或永不触发 → chain/group/chord 编排中断无人知晓。
- **修复方向**：所有 `let _ = store.<op>` 改为 `if let Err(e) = store.<op>.await { tracing::warn!(error=%e, task_id=%..., "op failed"); }`；关键路径升级 `error!` + metric。引入 Clippy `clippy::let_underscore_drop` + `clippy::let_underscore_must_use`。

#### D.3 【P0】`XhjobError` 没有 `source()` 实现，错误链断裂

- **位置**：[errors.rs:35](file:///workspace/src/errors.rs) `impl std::error::Error for XhjobError {}`
- **问题**：`XhjobError::Io(io::Error)` 持有原始 `io::Error`，但 `impl Error` 空实现没有 `fn source()`。所有 `Io` 变体的源错误链丢失。`Ipc(String)`/`Store(String)`/`Exec(String)` 用 `String` 持有错误信息，本身无法承载 source。
- **影响**：日志只能看到 `io: Connection reset by peer`，看不到哪一行/哪个 socket/哪个 op 触发；调试 SQLite 错误时无法区分 `SQLITE_BUSY` vs `SQLITE_CORRUPT` vs `SQLITE_READONLY`（全部被 `format!("{}", e)` 拍平）。
- **修复方向**：`Ipc(String)` 等改为 `Ipc { context: String, source: Box<dyn Error + Send + Sync> }` 结构体变体；实现 `source()` 返回内层错误；引入 `thiserror` 派生。

#### D.4 【P0】无任何 metrics（任务时长 / 队列深度 / 错误率）

- **位置**：整个项目
- **问题**：唯一可观测的计数器是 `utils/limits.rs::WorkerLimits::tasks_executed`（仅总数），通过 `stats` IPC op 暴露。没有：
  - 任务执行时长分布（histogram）
  - 当前 pending queue 深度（gauge）
  - 失败率 / 重试率 / 取消率
  - 各 service_name 维度的分桶
  - cron 触发延迟（misfire gap）分布
- **影响**：无法回答"daemon 现在健康吗"、无法 SLI/SLO 告警、无法容量规划。
- **修复方向**：引入 `metrics` + `metrics-exporter-prometheus`；`xhjob_inspect` 增加 `mode=metrics` 返回 Prometheus 文本格式；或独立 HTTP `:9101/metrics` 端口。`queue.rs::process_one` 包 `histogram!("xhjob.task.duration")`，enqueue/dequeue 更新 `gauge!("xhjob.queue.depth")`。

#### D.5 【P0】无分布式追踪，trace_id 不跨 PHP → daemon → worker

- **位置**：[ipc/mod.rs:166-181](file:///workspace/src/ipc/mod.rs) `request()` 构造 `Request { id: rand_id(), ... }`，`id` 仅纳秒时间戳不是 trace_id
- **问题**：PHP 端 `xhjob_dispatch` 不传 trace_id，daemon `handle_dispatch` 不提取 trace_id，executor 也不注入。全项目搜不到 `span!`/`#[instrument]`/`info_span`/`trace_id`。单个 PHP 请求触发 5 个 dispatch → 5 个独立 daemon 日志条目无法串联。
- **修复方向**：`Request` 增加 `trace_id: Option<String>` 字段，PHP 端 `xhjob_dispatch($task, $trace_id = null)` 注入；daemon `handle_connection` 用 `tracing::info_span!("ipc", trace_id, op)` 包裹；executor 在 `dispatch` 内嵌 `info_span!("task", task_id, trace_id)`。可对接 OpenTelemetry SDK。

#### D.6 【P0】无配置文件支持，全部走环境变量

- **位置**：所有 `std::env::var("XHJOB_*")` 调用（61 处）
- **问题**：项目 16 个 `XHJOB_` 配置项全部 env vars。无 toml/yaml/json 配置文件。PHP-FPM 场景下 env vars 通常由 pool config 的 `env[XHJOB_*]` 指令设置，但 PHP-FPM 默认不传递未在 `clear_env = no` 下显式声明的环境变量，PHP SAPI 在 CLI 模式下 env 行为不同。
- **影响**：配置变更需重启 PHP-FPM pool；配置漂移无法审计；多环境需写大量 env 模板。
- **修复方向**：支持 `XHJOB_CONFIG=/etc/xhjob/config.toml` 指向 toml 配置文件，优先级 env > config file > default。可用 `serde` + `config` crate。

#### D.7 【P0】无运行时重载，所有配置启动期冻结

- **位置**：[daemon_main.rs:872-886](file:///workspace/src/daemon_main.rs) `install_unix_signal_handler` 仅注册 SIGTERM/SIGINT
- **问题**：搜不到 SIGHUP/SIGUSR1/SIGUSR2 任何处理。`XHJOB_MAX_TASKS_PER_CHILD`/`XHJOB_MAX_MEMORY_PER_CHILD`/`XHJOB_POOL_MODE`/`XHJOB_ASYNC_POOL_SIZE` 启动期读取后冻结。`WorkerLimits::default()` 在 `init_worker_limits` 时一次性快照。
- **影响**：调整限流参数需 kill daemon 后由 PHP 重新 `xhjob_start`，期间任务丢失（除非 persist=true）。
- **修复方向**：注册 SIGHUP handler 触发 `ArcSwap<Config>` 重载；watch 信号 channel 在 `tokio::select!` 内合并到主循环。注意：`pool_mode`/`async_pool_size` 改动需重建池子，可标记"下次重启生效"。

#### D.8 【P0】日志文件无轮转，长跑 daemon 必撑爆磁盘

- **位置**：[lib.rs:588-614](file:///workspace/src/lib.rs) `reopen_std_streams_for_daemon`
- **问题**：`OpenOptions::new().create(true).append(true).open(&log)` 永远追加写。无大小检查、无按日轮转、无 truncate。`cron.rs` 每秒 scan + `queue.rs` 每 100ms 循环，DEBUG 级日志量极大。
- **影响**：daemon 跑几周后单文件 GB 级，磁盘满后 daemon 写日志失败但状态机继续运行（因 `let _ =`），最终 SQLite 也写不动 → 整体崩溃。
- **修复方向**：引入 `tracing-appender` 的 `RollingFileAppender`（按日/小时轮转），或 `logrotate` + SIGUSR1 重开 fd。建议默认按日轮转 + 保留 7 天。

#### D.9 【P1】28 个导出函数错误返回风格 4 套分裂（第一轮 P1-26 已提，第三轮给出完整 4 套分类）

- **位置**：[lib.rs](file:///workspace/src/lib.rs) 全部 26 个 `#[php_function]` + `Xhjob::dispatch`
- **问题**：4 套互不兼容契约：
  1. `bool`（11 个）：`xhjob_start/stop/restart/remove/pause/resume/cancel/requeue/reschedule/run_daemon/report_progress` — 失败原因仅 `tracing::error!`，PHP 拿不到任何错误字符串
  2. `Vec<(String, String)>`（3 个）：`xhjob_status/state/result` — 失败塞入 `("error", msg)` 或 `("state", "UNKNOWN")` 键
  3. `String` 前缀 `"error: ..."`（8 个）：`xhjob_dispatch/list/events/chain/group/chord/pull_events/inspect` — 成功也返回 String，靠前缀判错
  4. `Option<String>`（4 个）：`xhjob_get/chain_state/group_state/chord_state` — `None` 同时表示"未找到""daemon 不可达""IPC 错误""service name 非法"4 种语义
- **修复方向**：统一改为 `Result<T, XhjobError>`，PHP 端通过 ext-php-rs 抛 `Exception`（或 `XhjobException` 带 `code` + `category` 字段）。

#### D.10 【P1】`unwrap/expect/panic` 在 PHP-FPM 长跑进程内的爆炸半径

- **位置**：[coroutine_pool.rs:88](file:///workspace/src/pool/coroutine_pool.rs) `.expect("failed to build tokio runtime")`、[thread_pool.rs:69](file:///workspace/src/pool/thread_pool.rs) `.expect("failed to spawn worker thread")`、[daemon_main.rs:876/877](file:///workspace/src/daemon_main.rs) `.expect("install SIGTERM")` 等
- **问题**：daemon 是 PHP-FPM/CLI 通过 `xhjob_start` fork 出的长跑进程。任何 panic 在没有 catch_unwind 保护下都会让 daemon 整体退出，PHP 端 `xhjob_start` 返回 true 后不再监控。tokio runtime 构造失败、signal handler 安装失败让 daemon 启动期直接 abort，但 PID 文件未写，PHP 端下次 `xhjob_status` 才能发现。
- **修复方向**：daemon 启动期 `expect` 全改 `Result`，由 `daemon_main` 统一 banner + 错误后 `exit(1)`；运行期所有 unwrap/expect 替换为 `?` 或 `match`；引入 `panic::set_hook` 把 panic 写日志。

#### D.11 【P1】panic 无 catch_unwind + 记录，async task panic 静默

- **位置**：[thread_pool.rs:60](file:///workspace/src/pool/thread_pool.rs) `let _ = std::panic::catch_unwind(...)`；[coroutine_pool.rs:48-56](file:///workspace/src/pool/coroutine_pool.rs) `tokio::spawn` 返回 `JoinHandle` 被丢弃；[queue.rs:487](file:///workspace/src/scheduler/queue.rs)
- **问题**：线程池 `catch_unwind` 捕获 panic 后 `let _ =` 丢弃 `thread::Result`，**连一行日志都不打**，worker 静默吞错继续运行。协程池 `tokio::spawn` 返回的 `JoinHandle` 被 `let _ =` 丢弃，tokio 默认 panic 后 task 终止但 worker 继续，panic 信息走 `tracing` 默认 handler（但本项目没装 subscriber，**也是 NO-OP**）。daemon_main 没有 `panic::set_hook`，async 任务里任何 panic 不会被记录。
- **影响**：任务执行 panic 后状态机不推进（task 永远卡 Running），但 daemon 看起来一切正常。
- **修复方向**：daemon_main 启动时安装 `panic::set_hook` 把 panic + task_id（thread-local）写入 tracing error；thread_pool 改 `match catch_unwind(...) { Ok(()) => {}, Err(p) => tracing::error!(panic=?p, "worker panic") }`；协程池包装 `spawn` 让 future 内部 `AssertUnwindSafe(future).catch_unwind().await` 并记录。

#### D.12 【P1】proxy URL 凭据泄漏到错误消息

- **位置**：[http.rs:96](file:///workspace/src/executor/http.rs) `format!("parse proxy {}: {}", proxy_str, e)`、[http.rs:40](file:///workspace/src/executor/http.rs) `format!("invalid proxy url (no scheme): {}", url)`
- **问题**：`build_proxy` 接受原始 `proxy_str`（可能含 `user:pass@`），错误时直接 `format!` 把整个 URL 写入错误消息。socks5 分支已主动剥离（87/91），错误路径仍泄漏。
- **修复方向**：错误消息统一用 `parsed.host_port` 而非原始 `proxy_str`；实现 `Display` 时 `user:pass@` 段做 `***` 脱敏。

#### D.13 【P1】默认 `/tmp` + 日志文件 `0o644` 跨用户可读

- **位置**：[daemon/mod.rs:34-37](file:///workspace/src/daemon/mod.rs) `log_file_path` 默认 `/tmp`；[lib.rs:598](file:///workspace/src/lib.rs) `.mode(0o644)` 创建日志文件
- **问题**：`/tmp` 是世界可读目录。日志文件 mode 0o644 表示其他用户可读。日志内含 task 配置（shell 命令、HTTP headers、proxy URL 残留凭据）、错误堆栈、task_id。socket 文件 `0o660` 是合理的，但日志和 SQLite DB 权限管理缺位。
- **修复方向**：默认日志/DB 目录改 `/var/lib/xhjob` 或 `/var/log/xhjob`，mode 改 `0o640`；目录权限 `0o750`。

#### D.14 【P1】bool 配置解析不统一

- **位置**：[daemon_main.rs:71-78](file:///workspace/src/daemon_main.rs)
- **问题**：`XHJOB_PERSIST` 在两个 feature 分支下接受不同取值集合：feature on 时 `"off"`/`"no"`/`""` 都被视作 true；feature off 时只接受 `"1"`/`"true"`。两套都不支持 `on/off`/`yes/no`。无统一 `parse_bool` 工具函数。
- **影响**：用户设 `XHJOB_PERSIST=off` 在 feature 启用时反而打开 persist，行为反直觉。
- **修复方向**：`utils/mod.rs` 增 `pub fn parse_bool(s: &str) -> Option<bool>` 接受 `true/false/1/0/on/off/yes/no`（大小写不敏感）。

#### D.15 【P1】duration 配置只接受原始秒数，不支持 `1h`/`5s`

- **位置**：[shell.rs:269-276](file:///workspace/src/executor/shell.rs) `XHJOB_SHELL_TIMEOUT` 解析为 u64 秒；[limits.rs:39-48](file:///workspace/src/utils/limits.rs) `XHJOB_MAX_MEMORY_PER_CHILD`（字节）
- **问题**：用户必须写 `XHJOB_SHELL_TIMEOUT=3600` 表示 1 小时，不能写 `1h`/`60m`/`3600s`。`XHJOB_MAX_MEMORY_PER_CHILD` 必须写 `5368709120` 表示 5GB，不能写 `5g`/`512m`。
- **修复方向**：引入 `humantime`/`humansize` crate 解析。

#### D.16~D.18 【P1】配置项命名遗留别名不一致 / `XHJOB_DATA_DIR` 与 service 参数优先级隐式且未在 daemon 端验证 / 默认值 `/tmp`/1024/300s 偏激进

详见 [service/mod.rs:122-130/144-151](file:///workspace/src/service/mod.rs)、[daemon/mod.rs:46-70](file:///workspace/src/daemon/mod.rs)、[coroutine_pool.rs:71](file:///workspace/src/pool/coroutine_pool.rs)、[shell.rs:275](file:///workspace/src/executor/shell.rs)。修复方向：默认 data_dir 改 `/var/lib/xhjob`；async_pool_size 默认 `num_cpus::get() * 64`；shell_timeout 默认 60s。

#### D.19~D.21 【P1】日志上下文字段不一致 / 关键决策点缺日志 / 启动退出 banner 不全

详见 [daemon_main.rs:214/222](file:///workspace/src/daemon_main.rs)、[queue.rs:503/508](file:///workspace/src/scheduler/queue.rs)、[lib.rs](file:///workspace/src/lib.rs)。修复方向：daemon 入口 `info_span!("ipc", op, req_id)` 包裹所有 handler；PHP 端 lib.rs 错误日志加 `service = %service_name, op = "xhjob_xxx"` 字段；启动 banner 打印 `version=..., build=..., config={...}`。

#### D.22~D.25 【P2】`XHJOB_PERSIST` 与 cargo feature 耦合 / `XHJOB_*_DIR` 仅 Unix 部分生效 / `xhjob_state/result` 静默吞掉 daemon 错误 / 无配置漂移检测

详见对应位置。修复方向见第二轮同维度报告。

#### D.26 【P1】`reopen_std_streams_for_daemon` 使用 unsafe `dup2` 而非 `Stdio`

- **位置**：[lib.rs:587-614](file:///workspace/src/lib.rs)
- **问题**：用 `extern "C" { fn dup2(...) }` 手写 FFI 而非 `std::os::unix::io::AsRawFd`。`std::mem::forget(f)` 持有 fd 直到进程退出，无显式 close。
- **修复方向**：用 `tracing-appender` 的 `NonBlocking` writer 直接接管，跳过 std 重定向。

---

### 维度 E：安全与 DoS 防护（24 项）

#### E.1 【P0】daemon IPC 完全没有 peer 认证

- **位置**：[daemon_main.rs:247-286](file:///workspace/src/daemon_main.rs) `handle_connection`；[unix_socket.rs:30-38](file:///workspace/src/ipc/unix_socket.rs) `accept`；[named_pipe.rs:27-46](file:///workspace/src/ipc/named_pipe.rs)
- **问题**：`handle_connection` 接收连接后立即 `read_frame` 并按 `req.op` 分发，全程**没有**：Unix `SO_PEERCRED`/`getpeereid` 校验、token/shared secret、mTLS、任何握手。代码搜 `peer_cred|getpeer|SO_PEERCRED|uid|gid` 在 daemon/ipc 模块零命中。`unix_socket.rs:33` 的 `_addr` 被显式丢弃。
- **攻击场景**：任何能连接到 socket 文件的进程（同 uid、同组、root）都能执行全部 op，包括 `dispatch` 任意 shell 命令、`cancel` 他人任务、`remove` 任务、`reschedule` cron、`chain/group/chord` 派发。**谁能连 socket，谁就能以 daemon 身份执行任意 shell 命令**（通过 dispatch 一个 `TaskType::Shell` 任务）。
- **修复方向**：(a) Unix：accept 后立即 `getsockopt(SOL_SOCKET, SO_PEERCRED)` 取 `ucred`，校验 `uid == getuid() && gid == getgid()`，不匹配直接 close；(b) 引入 token：daemon 启动生成随机 token 写入仅 owner 可读的文件，client 首帧携带 token；(c) 管理 op（shutdown/cancel/remove/reschedule）要求额外权限位或独立 admin token。

#### E.2 【P0】PHP-FPM worker 横向越权（单 socket 共享，无 per-worker 隔离）

- **位置**：[daemon_main.rs:208-233](file:///workspace/src/daemon_main.rs) accept loop 只有一个 listener；[ipc/mod.rs:23-45](file:///workspace/src/ipc/mod.rs) socket 路径仅由 service_name 决定
- **问题**：同 service_name 的所有 PHP worker 共用同一 daemon socket。`handle_dispatch`/`handle_cancel_op`/`handle_remove_op`/`handle_reschedule_op` 等操作**仅凭 payload 中的 `task_id` 即可操作任意任务**，无"任务归属者"字段或所有权校验。
- **攻击场景**：站点 A 的 PHP 代码可以 `cancel`/`remove`/`state`/`result`/`reschedule` 站点 B 的任务；恶意租户可枚举 task_id（UUID v4，但 `replace_existing=true` 时 task_id 可由用户指定）；`load_result` 可读取他人 shell 任务的 stdout/stderr（可能含敏感输出）。
- **修复方向**：(a) Task 结构增加 `owner`/`tenant` 字段，dispatch 时绑定调用者身份，state/cancel/remove 校验所有权；(b) 或为每个租户/worker-pool 启动独立 daemon 实例（不同 service_name + 不同 socket + 不同 uid）。

#### E.3 【P0】同机其他用户可连接 daemon socket（默认 /tmp + 权限竞态 TOCTOU）

- **位置**：[unix_socket.rs:21-25](file:///workspace/src/ipc/unix_socket.rs)；[ipc/mod.rs:23-51](file:///workspace/src/ipc/mod.rs) `fallback_sock_dir = /tmp`；[daemon/mod.rs:46-70](file:///workspace/src/daemon/mod.rs)
- **问题**：(1) socket 默认 `/tmp/xhjob.<name>.sock`，`/tmp` world-writable（sticky）；(2) `UnixListener::bind(&path)` 先创建 socket 文件，**之后**才 `set_permissions(0o660)`。两步间存在 **TOCTOU 窗口**：bind 时 socket 模式由 umask 决定（PHP-FPM umask=022 → 文件 0o644），其他用户在 chmod 完成前可 connect；(3) 即使 chmod 成功，0o660 仍允许同组用户连接（daemon 以 www-data 运行时同组所有用户可连）；(4) bind 前 `remove_file(&path)` 会跟随符号链接，攻击者预先放 symlink 可删除任意可写路径下的文件（有限，因 sticky）。
- **修复方向**：(a) bind 前用 `tempfile` 在私有目录创建 socket，或 `socket_pair` + 抽象命名空间；(b) bind 后立即 `fchmod` fd 而非 path（避免 TOCTOU），设 `0o600`；(c) 默认目录从 `/tmp` 改为 `/run/xhjob` 或 `/var/run/xhjob`；(d) bind 前 `remove_file` 改 `lstat` 检查不是 symlink 再删，或 `O_NOFOLLOW`。

#### E.4 【P0】单连接 IPC 帧 64MB 上限过大 + 无并发连接数上限

- **位置**：[ipc/mod.rs:109](file:///workspace/src/ipc/mod.rs) `if len > 64 * 1024 * 1024`；[daemon_main.rs:208-225](file:///workspace/src/daemon_main.rs) accept loop 无限 `tokio::spawn`，无计数、无 Semaphore
- **问题**：(1) 单帧上限 64MB，`read_frame` `vec![0u8; len]` 一次分配 64MB，后续 `serde_json::from_slice` 再分配 2-3 倍。单个请求即可吃掉 ~200MB；(2) accept loop 对每个连接 `tokio::spawn`，**无并发上限、无连接速率限制、无 IP/uid 限流**。攻击者打开 10 万连接即耗尽 fd/内存；(3) `handle_connection` 每连接只处理一请求即返回，但连接期间无读超时，slowloris 风格可挂起大量半连接。
- **修复方向**：单帧降到 1-4MB；引入 `Semaphore`（参考 `coroutine_pool.rs:15` 已有用法）限并发连接（如 256）；每连接 `tokio::time::timeout` 设读超时。

#### E.5 【P0】内存 pending 队列 unbounded 增长

- **位置**：[queue.rs:22](file:///workspace/src/scheduler/queue.rs) `pending: Mutex<Vec<(i32, String)>>`；[queue.rs:72-78](file:///workspace/src/scheduler/queue.rs) `enqueue` 直接 `push` 无 cap
- **问题**：`TaskQueue::enqueue` 无限 push，`Vec` 无容量上限。cron 风暴、PHP 快速 dispatch、retry 风暴让 Vec 无限增长直到 OOM。第一轮 P1-14 已提"背压缺失"，第三轮独立确认 OOM 路径。
- **修复方向**：改用有界 channel（`tokio::sync::mpsc`）或加 `max_pending` 阈值，超限拒绝并返回 `XHJOB_QUEUE_FULL`。

#### E.6 【P0】Cron 风暴（同秒 1000 个 cron 触发，无批处理上限）

- **位置**：[cron.rs:112-284](file:///workspace/src/scheduler/cron.rs) `scan_once` 每秒 `load_active_tasks` 全表扫描；[daemon_main.rs:176-192](file:///workspace/src/daemon_main.rs) `cron.run` 回调对每个 task_id `tokio::spawn` enqueue
- **问题**：(1) `scan_once` 每秒执行，`load_active_tasks`（cron.rs:114）拉取全部 active 任务到内存（无 LIMIT）；(2) 1000 个 cron 任务同秒到期全部 `due.push`，回调里 `tokio::spawn` 对每个 id enqueue，无批大小限制；(3) `process_one` 每 100ms 才处理一个，队列积压无上限；(4) `coalesce=true`（默认）折叠错过的触发为一次 fire，但这是单任务的折叠，1000 个不同任务同秒触发仍全部 fire。
- **影响**：调度抖动后恢复时积压 cron 一次性涌入，daemon 卡死或 OOM。
- **修复方向**：`scan_once` 加 `LIMIT N` 分批；回调内对 due 列表分批 enqueue；全局 cron 触发速率限制。

#### E.7 【P0】任务 payload 明文存储（含密码/token/PII）

- **位置**：[sqlite.rs:26-29/247-248](file:///workspace/src/store/sqlite.rs) `payload TEXT NOT NULL`，`payload_str = serde_json::to_string(&task.payload)` 直接存
- **问题**：HTTP 任务的 `headers` 可能含 `Authorization: Bearer xxx`、`body` 可能含密码/PII；Shell 任务的 `cmd` 可能含内联密钥（`curl -u user:pass`）。全部以明文 JSON 写入 SQLite `tasks.payload` 列，**无加密、无字段级脱敏、无掩码**。
- **影响**：DB 文件被读取即泄露所有任务凭证；备份/快照同样泄露。
- **修复方向**：(a) 引入 `xhjob_encrypt_payload` 选项，对 payload 应用层加密（libsodium / AES-GCM），密钥放环境变量或 KMS；(b) 至少对已知敏感字段（headers.Authorization、body）做掩码存储；(c) 提供 `payload_secret_ref`（引用外部 secret store）替代内联。

#### E.8 【P0】数据库文件权限未显式设置（默认随 umask，可能 0o644 world-readable）

- **位置**：[sqlite.rs:15-17](file:///workspace/src/store/sqlite.rs) `Connection::open(path)` 未设文件权限；对比 [unix_socket.rs:23-25](file:///workspace/src/ipc/unix_socket.rs) socket 显式设 0o660
- **问题**：`Connection::open` 创建 `.db` 文件时按进程 umask（PHP-FPM 常为 022 → 文件 0o644）。**`.db` 文件 world-readable**，结合 E.7（明文 payload）= 任何本地用户可 `sqlite3 /tmp/xhjob.default.db` 读取所有任务凭证。WAL 文件 `.db-wal` 同理。socket 文件被显式 chmod 0o660，但 db 文件被遗漏，**严重不一致**。
- **修复方向**：`open` 后立即 `std::fs::set_permissions(path, 0o600)`；data_dir 默认从 `/tmp` 改 `/var/lib/xhjob`（0o700）。

#### E.9~E.12 【P1】`service_name` 校验绕过路径 / `data_dir` 路径穿越无防护 / 管理操作无更高权限校验 / PID 文件非原子写入

详见 [service/mod.rs:122-130/144-151](file:///workspace/src/service/mod.rs)、[daemon/mod.rs:46-70/87-94/165/292-303](file:///workspace/src/daemon/mod.rs)。修复方向：`ipc_path`/`pid_file_path`/`db_path_for` 内统一 `service::validate`；data_dir `canonicalize` + 白名单根目录；管理 op 引入 admin token；PID file 用 `flock` + `O_CREAT|O_EXCL`。

#### E.13~E.17 【P1】单 task payload/tags 无大小限制 / events 表无上限自动增长（`cleanup_expired_events` 是死代码，从未被自动调用）/ failed_jobs/results 表无 TTL 默认清理（`result_ttl` 默认 0 = 永久保留）/ `list()`/`inspect()` 无分页无大小限制 / SQLite 数据库文件无大小上限无 VACUUM 策略

详见 [daemon_main.rs:288-340/418-450/820-851](file:///workspace/src/daemon_main.rs)、[sqlite.rs:15-22/498-513/84-92/717-728](file:///workspace/src/store/sqlite.rs)、[store/mod.rs:171-174](file:///workspace/src/store/mod.rs)。修复方向：`build()` 内校验 payload ≤ 256KB、tags ≤ 32、单 tag ≤ 64 字符；`scan_once` 周期调 `cleanup_expired_events`；`result_ttl` 默认 7 天；list 加 `limit`/`offset` 参数；`PRAGMA max_page_count` 设上限。

#### E.18~E.21 【P1】日志文件无大小上限无轮转 / fork bomb 任务派生任务无递归深度限制 / 同 task_id 无限派发（replace_existing + requeue 循环）/ 巨型 JSON payload OOM（无流式解析）

详见 [daemon/mod.rs:34-37](file:///workspace/src/daemon/mod.rs)、[queue.rs:208-488](file:///workspace/src/scheduler/queue.rs)、[daemon_main.rs:540-740/313-320](file:///workspace/src/daemon_main.rs)、[ipc/mod.rs:112-116](file:///workspace/src/ipc/mod.rs)。修复方向：集成 `tracing-appender` `Rotation::DAILY` + max_files；chain/group/chord 限制 `tasks.len() ≤ 100` + 任务 meta 记录 `dispatch_depth`；requeue 加冷却时间；JSON 降到 4MB + `serde_json::Deserializer::from_reader` + 深度限制。

#### E.22~E.25 【P1】result 字段可能被日志记录 / SQL 注入残留风险点（`ensure_column` 用 `format!`，目前内部硬编码安全但需防御） / 命令注入新点（data_dir 未走 validate 进 PHP `-r` code 字符串） / 日志脱敏缺失 / 数据库无 at-rest 加密

详见 [queue.rs:441-453/244](file:///workspace/src/scheduler/queue.rs)、[sqlite.rs:158-172](file:///workspace/src/store/sqlite.rs)、[unix.rs:48-58](file:///workspace/src/daemon/unix.rs)、[daemon_main.rs:88/253](file:///workspace/src/daemon_main.rs)、[sqlite.rs:15-22](file:///workspace/src/store/sqlite.rs)。修复方向：dispatch 错误脱敏后再 log；`ensure_column` 文档标注"仅限内部硬编码"或 allowlist 校验；data_dir 走 `validate` 风格字符白名单 + canonicalize；引入 `tracing` field filter 对 `payload`/`body`/`headers`/`cmd` 自动掩码；集成 SQLCipher 或应用层加密。

#### E.26 【P2】无备份/快照机制 + 无 GDPR right-to-be-forgotten

- **位置**：[sqlite.rs](file:///workspace/src/store/sqlite.rs) 全文（无 backup/export 接口）；[sqlite.rs:409-419](file:///workspace/src/store/sqlite.rs) `delete_task` 删 task + result，但 **events 表无 ON DELETE CASCADE**，`events.task_id` 是普通列
- **问题**：(1) 无内置备份/快照功能（依赖外部 `sqlite3 .backup`）；(2) `delete_task` 删除 task 和 result，但 **events 表中该 task_id 的事件记录永久残留**（`sqlite.rs:84-90` events 表无外键约束、无级联删除）。GDPR right-to-be-forgotten 场景下，用户数据删除不彻底。
- **修复方向**：events 表加 `FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE`；或 `delete_task` 同时 `DELETE FROM events WHERE task_id=?`；提供 `purge_task` op 彻底清除。

---

## 十一、第三轮改进建议优先级汇总（去重后）

> 第一轮 7 P0 + 21 P1 + 30+ P2；第二轮新增 10 P0 + 25 P1 + 18 P2 + 8 Bug + 10 新功能；第三轮新增 **30 P0 + 35 P1 + 25 P2**（与第一/二轮部分交叉的合并去重）。三轮累计 **47 P0 + 81 P1 + 73+ P2 + 8 Bug + 10 新功能**。

### 立即修复（第三轮 P0，影响核心功能/安全/可用性）

| # | 位置 | 问题 | 修复要点 |
|---|---|---|---|
| 1 | [chord.rs:66-164](file:///workspace/src/scheduler/chord.rs) | chord `refresh_state` check-then-act 竞态，callback 可重复派发 | SQLite 条件 UPDATE CAS 或 `tokio::sync::Mutex` 按 chord_id 分片 |
| 2 | [sqlite.rs:10,246+](file:///workspace/src/store/sqlite.rs) | SqliteStore 在 tokio worker 线程上执行同步阻塞 I/O | `tokio::task::spawn_blocking` 或 `deadpool-sqlite` |
| 3 | [queue.rs:734](file:///workspace/src/scheduler/queue.rs) + [daemon_main.rs:42](file:///workspace/src/daemon_main.rs) | `std::process::exit` 跳过 Drop，SQLite 连接未正常关闭 | 通过 `shutdown_tx` 通知主循环优雅退出 |
| 4 | [coroutine_pool.rs:88](file:///workspace/src/pool/coroutine_pool.rs) + [Cargo.toml:43-45](file:///workspace/Cargo.toml) | Rust panic 穿越 `extern "C"` = UB | `Cargo.toml` 加 `panic = "abort"` |
| 5 | [lib.rs:137](file:///workspace/src/lib.rs) 等 23 处 + [ipc/mod.rs:166-181/104-117](file:///workspace/src/ipc/mod.rs) | FPM worker 被 `block_on(ipc::request)` 永久阻塞 | `tokio::time::timeout(5-10s)` 包裹 `ipc::request` |
| 6 | [http.rs:108-160](file:///workspace/src/executor/http.rs) + [retry/mod.rs:58-73](file:///workspace/src/retry/mod.rs) | POST/PUT/DELETE 在 5xx/网络错误时无条件重试，重复副作用 | 按方法区分重试 + `idempotent` 标记 |
| 7 | [shell.rs:48-49/146-162](file:///workspace/src/executor/shell.rs) | stdout/stderr 管道死锁，高输出任务全部 timeout 失败无输出 | `tokio::try_join!(stdout, stderr, wait)` 并发 |
| 8 | 全 `/workspace/src` | tracing subscriber 全程未初始化，77 处 `tracing!` 全部 NO-OP | `tracing_subscriber::fmt()...try_init()` |
| 9 | [queue.rs](file:///workspace/src/scheduler/queue.rs) 等 82 处 | `let _ = expr` 系统性吞错，含状态机迁移失败 | 改 `if let Err(e) = ... { tracing::warn! }` + Clippy lint |
| 10 | [errors.rs:35](file:///workspace/src/errors.rs) | `XhjobError` 无 `source()` 实现，错误链断裂 | 引入 `thiserror`，结构化错误变体 |
| 11 | 全项目 | 无任何 metrics（任务时长/队列深度/错误率） | `metrics` + `metrics-exporter-prometheus` |
| 12 | [ipc/mod.rs:166-181](file:///workspace/src/ipc/mod.rs) | 无分布式追踪，trace_id 不跨 PHP→daemon→worker | `Request` 加 `trace_id` 字段 + `info_span!` |
| 13 | 全部 `std::env::var("XHJOB_*")`（61 处） | 无配置文件支持，全部走 env vars | `XHJOB_CONFIG=/etc/xhjob/config.toml` + `serde` + `config` crate |
| 14 | [daemon_main.rs:872-886](file:///workspace/src/daemon_main.rs) | 无 SIGHUP / 运行时重载 | 注册 SIGHUP handler + `ArcSwap<Config>` |
| 15 | [lib.rs:588-614](file:///workspace/src/lib.rs) | 日志文件无轮转，必撑爆磁盘 | `tracing-appender` `RollingFileAppender` 按日轮转 |
| 16 | [daemon_main.rs:247-286](file:///workspace/src/daemon_main.rs) + [unix_socket.rs:30-38](file:///workspace/src/ipc/unix_socket.rs) | daemon IPC 完全没有 peer 认证 | Unix `SO_PEERCRED` uid/gid 校验 + token |
| 17 | [daemon_main.rs:208-233](file:///workspace/src/daemon_main.rs) | PHP-FPM worker 横向越权 | Task 加 `owner`/`tenant` 字段 + 所有权校验 |
| 18 | [unix_socket.rs:21-25](file:///workspace/src/ipc/unix_socket.rs) | socket 默认 `/tmp` + 权限竞态 TOCTOU | `fchmod` fd 而非 path + 默认 `/run/xhjob` |
| 19 | [ipc/mod.rs:109](file:///workspace/src/ipc/mod.rs) + [daemon_main.rs:208-225](file:///workspace/src/daemon_main.rs) | 单连接 64MB 帧 + 无并发连接数上限 | 单帧降到 1-4MB + `Semaphore(256)` |
| 20 | [queue.rs:22/72-78](file:///workspace/src/scheduler/queue.rs) | 内存 pending 队列 unbounded 增长 | 有界 channel 或 `max_pending` 阈值 |
| 21 | [cron.rs:112-284](file:///workspace/src/scheduler/cron.rs) + [daemon_main.rs:176-192](file:///workspace/src/daemon_main.rs) | Cron 风暴同秒 1000 触发 | `LIMIT N` 分批 + 全局 cron 速率限制 |
| 22 | [sqlite.rs:26-29/247-248](file:///workspace/src/store/sqlite.rs) | 任务 payload 明文存储（含密码/token/PII） | 应用层加密（libsodium / AES-GCM） |
| 23 | [sqlite.rs:15-17](file:///workspace/src/store/sqlite.rs) | 数据库文件权限未显式设置（默认 0o644） | `set_permissions(0o600)` + 默认 `/var/lib/xhjob` |

### 近期修复（第三轮 P1）

24. [http.rs:117-126](file:///workspace/src/executor/http.rs) 重定向 https→http 协议降级
25. [http.rs:149-150](file:///workspace/src/executor/http.rs) 响应 body 无上限 → OOM
26. [http.rs:82-83/96-103](file:///workspace/src/executor/http.rs) 代理认证凭据泄漏
27. [http.rs:138](file:///workspace/src/executor/http.rs) 无 URL 协议白名单 + 无 SSRF 防护
28. [shell.rs:151-152/157-158](file:///workspace/src/executor/shell.rs) `max_output_bytes` 未实现
29. [shell.rs:74/82/98/128/131/135](file:///workspace/src/executor/shell.rs) kill 不杀进程组 → 孤儿子进程
30. [shell.rs:38-45](file:///workspace/src/executor/shell.rs) 无 `kill_on_drop` + 无 `PR_SET_PDEATHSIG`
31. [shell.rs:161](file:///workspace/src/executor/shell.rs) ExitStatus 信号信息丢失，崩溃任务被无限重试
32. [shell.rs:38-45/253-266](file:///workspace/src/executor/shell.rs) env 未清空 → 子进程可 dump daemon 密钥
33. [shell.rs:38-45](file:///workspace/src/executor/shell.rs) 工作目录未隔离
34. [lib.rs:105/158/219/658](file:///workspace/src/lib.rs) `Vec<(String,String)>` PHP 端是关联数组，入参错传失败
35. [lib.rs:126/376](file:///workspace/src/lib.rs) `Option<String>` 入参对非字符串静默退化为 None
36. [ipc/mod.rs:93](file:///workspace/src/ipc/mod.rs) `write_frame` 帧长 `as u32` 截断
37. [lib.rs:709/719/724/734/758/765/799/870](file:///workspace/src/lib.rs) `i64 as u32/u64/i32` 整数入参静默回绕
38. [lib.rs](file:///workspace/src/lib.rs) 28 个导出函数错误返回风格 4 套分裂
39. [coroutine_pool.rs:88](file:///workspace/src/pool/coroutine_pool.rs) + [thread_pool.rs:42/69](file:///workspace/src/pool/thread_pool.rs) + [daemon_main.rs:876/877](file:///workspace/src/daemon_main.rs) `unwrap/expect/panic` 在 PHP-FPM 长跑进程
40. [thread_pool.rs:60](file:///workspace/src/pool/thread_pool.rs) + [coroutine_pool.rs:48-56](file:///workspace/src/pool/coroutine_pool.rs) panic 无 catch_unwind + 记录，async panic 静默
41. [http.rs:96/40](file:///workspace/src/executor/http.rs) proxy URL 凭据泄漏到错误消息
42. [daemon/mod.rs:34-37](file:///workspace/src/daemon/mod.rs) + [lib.rs:598](file:///workspace/src/lib.rs) 默认 `/tmp` + 日志 `0o644` 跨用户可读
43. [daemon_main.rs:71-78](file:///workspace/src/daemon_main.rs) bool 配置解析不统一
44. [shell.rs:269-276](file:///workspace/src/executor/shell.rs) + [limits.rs:39-48](file:///workspace/src/utils/limits.rs) duration 配置只接受秒数，不支持 `1h`/`5s`
45. [lib.rs:587-614](file:///workspace/src/lib.rs) `reopen_std_streams_for_daemon` 使用 unsafe `dup2`
46. [service/mod.rs:122-130/144-151](file:///workspace/src/service/mod.rs) + [daemon/mod.rs:46-70/87-94/165/292-303](file:///workspace/src/daemon/mod.rs) service_name 绕过 validate + data_dir 路径穿越 + 管理操作无权限校验 + PID 文件非原子
47. [daemon_main.rs:288-340/418-450/820-851](file:///workspace/src/daemon_main.rs) 单 task payload/tags 无大小限制
48. [sqlite.rs:84-92/717-728](file:///workspace/src/store/sqlite.rs) events 表 `cleanup_expired_events` 死代码，从未被自动调用
49. [store/mod.rs:171-174](file:///workspace/src/store/mod.rs) `result_ttl` 默认 0 = 永久保留
50. [daemon_main.rs:418-450/820-851](file:///workspace/src/daemon_main.rs) `list()`/`inspect()` 无分页
51. [sqlite.rs:15-22](file:///workspace/src/store/sqlite.rs) SQLite 数据库文件无 `max_page_count` 上限
52. [daemon/mod.rs:34-37](file:///workspace/src/daemon/mod.rs) 日志文件无大小上限无轮转
53. [queue.rs:208-488](file:///workspace/src/scheduler/queue.rs) + [daemon_main.rs:540-740](file:///workspace/src/daemon_main.rs) fork bomb 任务派生任务无递归深度限制
54. [daemon_main.rs:313-320](file:///workspace/src/daemon_main.rs) + [queue.rs:456-465](file:///workspace/src/scheduler/queue.rs) 同 task_id 无限派发（replace_existing + requeue 循环）
55. [ipc/mod.rs:112-116](file:///workspace/src/ipc/mod.rs) 巨型 JSON payload OOM 无流式解析
56. [queue.rs:441-453/244](file:///workspace/src/scheduler/queue.rs) result 字段可能被日志记录
57. [sqlite.rs:158-172](file:///workspace/src/store/sqlite.rs) SQL 注入残留风险点（`ensure_column` 用 `format!`）
58. [unix.rs:48-58](file:///workspace/src/daemon/unix.rs) 命令注入新点（data_dir 未走 validate 进 PHP `-r` code）
59. [daemon_main.rs:88/253](file:///workspace/src/daemon_main.rs) + [sqlite.rs:15-22](file:///workspace/src/store/sqlite.rs) 日志脱敏缺失 + 数据库无 at-rest 加密

### 后续优化（第三轮 P2）

60. [lib.rs:126/376/963/1094/1139/1225/1315](file:///workspace/src/lib.rs) 错误经 `"error:"` 字符串前缀返回，契约脆弱 → 改 `PhpResult<String>`
61. [lib.rs:1399-1429](file:///workspace/src/lib.rs) 无 RSHUTDOWN 钩子做每请求清理
62. [http.rs:113-126](file:///workspace/src/executor/http.rs) Client 不复用（连接池无）
63. [http.rs](file:///workspace/src/executor/http.rs) HTTP 任务不支持 cancel（trait 默认实现忽略 cancel_flag）
64. [http.rs:147](file:///workspace/src/executor/http.rs) URL 可能含 token 入日志
65. [http.rs:117-118](file:///workspace/src/executor/http.rs) 无 `connect_timeout`，慢连接吃满总预算
66. [http.rs:117](file:///workspace/src/executor/http.rs) TLS 配置依赖 feature flag 隐式选择，未显式锁定
67. [http.rs:113-160](file:///workspace/src/executor/http.rs) in-flight 请求未显式 abort
68. [http.rs:139-141](file:///workspace/src/executor/http.rs) Authorization header 缺显式保护注释
69. [shell.rs:41-44](file:///workspace/src/executor/shell.rs) 子进程 fd 继承（依赖 Rust 默认 CLOEXEC，未显式 `close_fds`）
70. [shell.rs:38-45](file:///workspace/src/executor/shell.rs) uid/gid 未切换 + capabilities 未 drop
71. [shell.rs:146-162](file:///workspace/src/executor/shell.rs) timeout 仅覆盖 wait，不覆盖后续 stdout/stderr 读取
72. [shell.rs:111-118](file:///workspace/src/executor/shell.rs) cancel_flag 轮询 200ms 粒度，存在最长 200ms 取消延迟
73. [shell.rs:57/64](file:///workspace/src/executor/shell.rs) soft_timeout grace 可小到 1 秒
74. [shell.rs:253-266](file:///workspace/src/executor/shell.rs) 无 exec 模式（cmd 即 shell 字符串）
75. [shell.rs:261-265](file:///workspace/src/executor/shell.rs) Windows `cmd /C` 转义复杂
76. [overlap.rs:20/39-75](file:///workspace/src/scheduler/overlap.rs) in-memory `running` map 死缓存
77. [queue.rs:72-87](file:///workspace/src/scheduler/queue.rs) `enqueue` 全排序 + `drain_next` `remove(0)`
78. [thread_pool.rs:60](file:///workspace/src/pool/thread_pool.rs) `AssertUnwindSafe` 闭包 panic 安全性
79. [rate_limit.rs:29](file:///workspace/src/scheduler/rate_limit.rs) `Arc<Mutex<>>` 内嵌于 `Arc<RateLimiter>`，冗余
80. [daemon_main.rs:236-238](file:///workspace/src/daemon_main.rs) 关闭仅 sleep 200ms，无 in-flight task join
81. [in_memory.rs:192-217](file:///workspace/src/store/in_memory.rs) `cleanup_expired_results` 持 tasks 读锁阻塞所有写
82. [cron.rs:278-281](file:///workspace/src/scheduler/cron.rs) `LAST_CLEANUP_TS` `Relaxed` + 非原子 check-then-set，可能重复清理
83. [thread_pool.rs:96-109](file:///workspace/src/pool/thread_pool.rs) Drop 不 join worker
84. [daemon_main.rs:214/222](file:///workspace/src/daemon_main.rs) + [queue.rs:503/508](file:///workspace/src/scheduler/queue.rs) 日志上下文字段不一致，缺 task_id/op
85. [daemon_main.rs:56-60/243](file:///workspace/src/daemon_main.rs) 启动/退出 banner 信息不全
86. [lib.rs](file:///workspace/src/lib.rs) 关键决策点缺日志（expires/rate-limit/overlap-skip）
87. [daemon_main.rs:71-78](file:///workspace/src/daemon_main.rs) `XHJOB_PERSIST` 与 cargo feature 耦合
88. [daemon/mod.rs:16-17](file:///workspace/src/daemon/mod.rs) `XHJOB_*_DIR` 仅 Unix 部分生效
89. [lib.rs:174-178/233-238](file:///workspace/src/lib.rs) `xhjob_state/result` 静默吞掉 daemon 错误
90. [sqlite.rs:84-90/409-419](file:///workspace/src/store/sqlite.rs) events 表无 ON DELETE CASCADE，无 GDPR right-to-be-forgotten

---

## 十二、三轮审查总结

### 累计发现统计

| 维度 | P0 | P1 | P2 | Bug | 新功能 |
|---|---|---|---|---|---|
| 第一轮（架构/并发/调度/执行器/存储/API） | 7 | 21 | 30+ | - | - |
| 第二轮（cron/sqlite/PHP 客户端/业界对标深挖） | 10 | 25 | 18 | 8 | 10 |
| 第三轮 A：并发原语 | 4 | 3 | 7 | - | - |
| 第三轮 B：ext-php-rs FFI 边界 | 1 | 4 | 2 | - | - |
| 第三轮 C：HTTP/Shell 执行器 | 2 | 11 | 14 | - | - |
| 第三轮 D：错误处理/可观测性/配置 | 11 | 9 | 10 | - | - |
| 第三轮 E：安全与 DoS 防护 | 8 | 17 | 4 | - | - |
| **三轮累计（去重）** | **47** | **81** | **73+** | **8** | **10** |

### 最关键的"必修五件套"（任一不修都属生产事故源）

1. **A.4 / B.1 / D.10**：`Cargo.toml` 加 `panic = "abort"` —— 一行配置消除 UB
2. **A.5**：`ipc::request` 加 `tokio::time::timeout(5s)` —— 一行包裹消除 FPM worker 永久阻塞
3. **D.1**：`tracing_subscriber::fmt()...try_init()` —— 让 77 处日志真正输出
4. **E.1 / E.8**：daemon 加 `SO_PEERCRED` + SQLite 文件 `set_permissions(0o600)` —— 阻断本地任意用户提权
5. **C.1**：HTTP 重试按方法区分 + `idempotent` 标记 —— 防止非幂等 POST 被重复执行

### 与第二轮新功能（F1-F10）的协同关系

第三轮发现的问题，恰恰是第二轮 F1-F10 新功能能否落地的前置条件：

- **F1 死信队列** 需先修 D.2（`let _ = expr` 82 处吞错）才能保证迁移可靠
- **F2 心跳保活** 需先修 D.4（无 metrics）才能可视
- **F3 调度器身份锁** 需先修 E.1（IPC 无认证）才有意义
- **F4 重试策略增强** 需先修 C.1（非幂等重试）+ C.10（ExitStatus 信号丢失）
- **F5 Web Dashboard** 需先修 E.1/E.2（认证授权）否则变 RCE
- **F6 多队列路由** 需先修 E.5（pending 队列 unbounded）
- **F7 Filter Pipeline** 需先修 D.9（28 函数错误返回风格 4 套）统一异常体系

---

**三轮审查均为只读，未修改任何文件**。如需针对某条问题给出具体补丁方案或开始实现某个新功能，请告知。
