# xhjob Rust 扩展全量代码审查报告

> **审查视角**：PHP 生产环境（PHP-FPM + 高并发 worker + 长跑 daemon + 运维友好性）
> **审查范围**：`/workspace/src` 下 27 个 Rust 源文件（~12000 行）
> **审查方式**：只读审查，未修改任何代码
> **审查日期**：2026-07-22

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

**审查未修改任何文件**。以上为纯只读审查结论。如需针对某条问题给出具体补丁方案，请告知。
