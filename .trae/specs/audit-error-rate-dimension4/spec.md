# 维度 4：错误率审计报告

## 审计范围

### Rust 核心库（`/workspace/src/`，~14k LOC）

| 模块 | 文件 | 审计重点 |
|---|---|---|
| errors | `errors.rs` | XhjobError 枚举设计、#[source] 链、From 实现 |
| lib | `lib.rs` | ipc_request timeout、dispatch() error: 前缀契约 |
| ipc | `ipc/mod.rs`、`ipc/unix_socket.rs`、`ipc/named_pipe.rs` | 帧协议错误映射、8MB 上限、SO_PEERCRED |
| store | `store/sqlite.rs`、`store/in_memory.rs`、`store/crypto.rs`、`store/mod.rs` | Mutex unwrap、unwrap_or 吞错、WAL/busy_timeout |
| executor | `executor/http.rs`、`executor/shell.rs`、`executor/mod.rs` | HTTP 边界、Shell 边界、cancel 轮询 |
| scheduler | `scheduler/queue.rs`、`scheduler/cron.rs`、`scheduler/chain.rs`、`scheduler/chord.rs`、`scheduler/group.rs` | RAII、错误传播、scan_once warn |
| daemon | `daemon/mod.rs`、`daemon_main.rs` | send_terminate、连接超时、信号量、启动期 expect |
| pool | `pool/coroutine_pool.rs`、`pool/thread_pool.rs` | 启动期 expect、catch_unwind |

### PHP ThinkPHP 8 扩展（`/workspace/releases/xhjob-thinkphp8-extend/Xhjob/`，~6k LOC）

| 文件 | 审计重点 |
|---|---|
| `Exception/XhjobException.php` | 异常基类 |
| `Exception/InvalidTaskConfigException.php` | 配置异常 |
| `Exception/ServiceNotRunningException.php` | daemon 不可达异常 |
| `Exception/TaskNotFoundException.php` | 任务未找到异常 |
| `TaskManager.php` | state/result/get/list/chainState 等错误路径 |
| `Client.php` | callWithRetry 重试、fallback 兜底、@throws |
| `TaskBuilder.php` | dispatch/toJson/withHeaders 错误处理 |
| `XhjobService.php` | start/restart/ensureRunning 错误处理 |
| `facade/Xhjob.php` | @method 标注一致性 |
| `ServiceProvider.php`、`helper.php` | 容器注册、辅助函数 |

### 审计约束

- 只读审计，不修改任何代码
- 区分启动期 unwrap（合理）与生产路径 unwrap（需审查）
- fire-and-forget 的 `let _ = tx.send(...)` 不算缺陷
- 不重复维度 1/3 已发现问题（M-1 事务原子性、M-2 持锁 await）
- 每个 FAIL 必须给文件:行号 + 严重级别 + 修复建议
- 严重级别：CRITICAL / HIGH / MEDIUM / LOW

---

## 审计结果汇总

| 指标 | 数值 |
|---|---|
| 已检查项总数 (N) | 55 |
| PASS 项数 (P) | 44 |
| FAIL 项数 (F) | 11 |
| **通过率** | **44 / 55 = 80.0%** |
| 目标通过率 | ≥ 97% |
| **是否达标** | **否（未达标）** |

### 缺陷严重级别分布

| 级别 | 数量 |
|---|---|
| CRITICAL | 0 |
| HIGH | 1 |
| MEDIUM | 7 |
| LOW | 3 |

---

## 详细检查项

### Task 4.1: Rust 错误处理（5 类，35 项）

#### 类别 1: Result 正确传播（无 unwrap/expect/? 滥用）— 7 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 1 | errors.rs XhjobError 枚举使用 thiserror + #[source] 链 | PASS | `errors.rs:1-113` — Io/Ipc/Store/Exec/Config 变体均带 `#[source]` 或 `#[from]`，source 链完整 |
| 2 | errors.rs From<String>/From<io::Error> 支持 ? 传播 | PASS | `errors.rs` — `From<String>` 路由到 `ipc(s)`，`From<io::Error>` 通过 `#[from]` |
| 3 | errors.rs 类型化构造器 ipc/store/exec/config | PASS | `errors.rs` — 提供 `ipc()`/`ipc_with_source()`/`store()`/`exec()` 等构造器 |
| 4 | lib.rs ipc_request 用 timeout 包裹 + ? 传播 | PASS | `lib.rs:51-70` — `tokio::time::timeout(5s, ipc::request(...))` 防 PHP-FPM 死锁 |
| 5 | lib.rs dispatch() 错误返回 "error:" 前缀 | PASS | `lib.rs:1009-1022` — `Err(e) => format!("error: {}", e)` 保留错误上下文 |
| 6 | ipc/mod.rs write_frame/read_frame map_err 映射 | PASS | `ipc/mod.rs:118-149` — `XhjobError::ipc(format!("write len: {}", e))` |
| 7 | store/sqlite.rs/crypto.rs/cron.rs/http.rs/shell.rs 用 map_err + ? | PASS | sqlite.rs 查询用 `.map_err(|e| XhjobError::store(...))?`；crypto.rs `?`；cron.rs `XhjobError::CronParse` |

#### 类别 2: 错误信息可定位（含上下文 task_id/op/cause）— 5 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 8 | errors.rs Ipc/Store/Exec/Config 变体含 context 字符串 | PASS | `errors.rs` — `Ipc { context, source }` 等变体设计 |
| 9 | ipc/mod.rs 错误含 op 上下文 | PASS | `ipc/mod.rs:122-149` — "write len"/"read len"/"serialize" |
| 10 | store/sqlite.rs map_err 含方法名上下文 | PASS | `sqlite.rs:622` — `"cancel_task query: {}"`, `sqlite.rs:629` — `"cancel_task update: {}"` |
| 11 | lib.rs ipc_request 错误含 op + timeout | PASS | `lib.rs:65-68` — `"request timeout ({}s) for op={}"` |
| 12 | executor/http.rs 错误含操作描述 | PASS | `http.rs:51` — `"invalid proxy url (no scheme): {}"`, `http.rs` — `"http send: {}"` |

#### 类别 3: 无静默吞错（let _ = / unwrap_or_default 掩盖真实错误）— 10 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 13 | ipc fire-and-forget tx.send（合理） | PASS | `daemon_main.rs:1267-1268` — `let _ = tx.send(15).await;` 关闭信号 fire-and-forget |
| 14 | daemon let _ = remove_file（合理清理） | PASS | `daemon/mod.rs:81,98,105` — 最佳努力清理 stale PID/socket |
| 15 | shell let _ = child kill/wait（合理） | PASS | `shell.rs:64,100,108,124` — 子进程 kill/wait fire-and-forget |
| 16 | queue let _ = shutdown_tx.send（合理） | PASS | `queue.rs:705` — 关闭信号 fire-and-forget |
| 17 | in_memory let _ = record_event（合理） | PASS | `in_memory.rs:180` — 事件记录 fire-and-forget |
| 18 | sqlite.rs:633 let _ = conn.execute 事件 INSERT | **FAIL** | `sqlite.rs:633` — Cancelled 事件 INSERT 错误被静默丢弃，应至少 `tracing::warn!` |
| 19 | sqlite.rs:255 maybe_decrypt 回退明文（文档化兼容） | PASS | `sqlite.rs:252-255` — 文档说明 "backward compat with rows written before encryption" |
| 20 | sqlite.rs:256,259 payload/state unwrap_or 掩盖损坏 | **FAIL** | `sqlite.rs:256` — `serde_json::from_str(...).unwrap_or(Null)` 吞 JSON 解析错误；`sqlite.rs:259` — `TaskState::from_str(...).unwrap_or(Pending)` 吞状态损坏 |
| 21 | sqlite.rs:306,926,944 tags/EventType unwrap_or | **FAIL** | `sqlite.rs:306` tags 静默变空；`sqlite.rs:926,944` 未知事件类型静默变 Started |
| 22 | sqlite.rs:1013+ chain/group/chord list unwrap_or_default | **FAIL** | `sqlite.rs:1013,1062,1114,1162,1215` — chain/group/chord 任务列表 JSON 解析失败静默变空 |

#### 类别 4: panic 风险（unwrap/数组下标/from_utf8/parse/整数转换）— 7 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 23 | sqlite.rs conn.lock().unwrap() x39 中毒 panic | **FAIL** | `sqlite.rs` 39 处 — Mutex 中毒时 `.lock().unwrap()` panic，daemon 崩溃 |
| 24 | ipc/mod.rs:121 json.len() as u32 截断 | **FAIL** | `ipc/mod.rs:121` — `json.len() as u32` 若 JSON > 4GB 静默截断（理论风险） |
| 25 | daemon_main.rs expect SIGTERM/SIGINT（启动期） | PASS | `daemon_main.rs:1264-1265` — 信号安装失败 panic 属启动期，合理 |
| 26 | pool expect runtime/spawn（启动期） | PASS | `coroutine_pool.rs:88`、`thread_pool.rs:77` — runtime/线程创建失败属启动期 |
| 27 | #[cfg(test)] expect/unwrap（测试代码） | PASS | `shell.rs:454`、`cron.rs:575`、`task/mod.rs:861` 等 — 全部在 `#[cfg(test)]` 块内 |
| 28 | http.rs String::from_utf8 检测（无 lossy） | PASS | `http.rs:217` — `String::from_utf8(body_buf)` 检测无效 UTF-8 → base64 编码 |
| 29 | shell.rs/cron.rs 安全 unwrap_or 回退 | PASS | `shell.rs:235` — `status.code().unwrap_or(-1)`；`cron.rs:415` — `now_ts().unwrap_or(0)` |

#### 类别 5: 外部边界错误处理（HTTP/SQLite/IPC/Shell）— 8 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 30 | HTTP 边界（timeout/redirect/body/binary/cancel/proxy） | PASS | `http.rs:119-141` connect_timeout 10s + redirect 5 + `http.rs:180` MAX_BODY_BYTES 64MiB + `http.rs:217` binary→base64 + `http.rs` cancel 200ms 轮询 + `http.rs:40-64` proxy 校验 |
| 31 | Shell 边界（soft_timeout/kill_on_drop/env_clear/output cap） | PASS | `shell.rs:29-277` soft_timeout→SIGTERM→grace→SIGKILL + `kill_on_drop(true)` + `process_group(0)` + `env_clear()` + `shell.rs:147` MAX_OUTPUT_BYTES 64MiB |
| 32 | SQLite 边界（WAL/busy_timeout/参数化） | PASS | `sqlite.rs:19-22` WAL + synchronous=NORMAL + busy_timeout=5000ms；全部查询用 `params![]` |
| 33 | IPC 边界（8MB cap/fchmod/SO_PEERCRED） | PASS | `ipc/mod.rs:141` 8MB 帧上限；`unix_socket.rs:13-49` fchmod（TOCTOU-safe）；`unix_socket.rs:91-115` SO_PEERCRED |
| 34 | lib.rs ipc_request timeout 5s + daemon_main 连接 timeout 15s | PASS | `lib.rs:51-70` timeout 5s 防 PHP-FPM 死锁；`daemon_main.rs:345-360` 每连接 15s timeout |
| 35 | daemon_main XHJOB_MAX_CONNECTIONS 信号量 + ownership_check | PASS | `daemon_main.rs:305-310` 连接数信号量；`daemon_main.rs:498-566` 默认拒绝多租户隔离 |

---

### Task 4.2: PHP 客户端错误处理（4 类，20 项）

#### 异常类型层级 — 3 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 36 | XhjobException 层级（基类 + 3 子类） | PASS | `XhjobException.php` extends `\Exception`；3 子类均 extends `XhjobException`；`catch XhjobException` 可统一捕获 |
| 37 | 异常类型语义正确 | PASS | InvalidTaskConfigException（配置/dispatch error）、ServiceNotRunningException（daemon 不可达）、TaskNotFoundException（任务不存在）语义清晰 |
| 38 | facade @method 标注一致 | PASS | `facade/Xhjob.php:27-47` — @method 标注与 TaskManager 方法签名一致 |

#### TaskManager 错误路径 — 8 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 39 | create/chain/group/chord dispatch 抛 InvalidTaskConfigException | PASS | `TaskManager.php:86-90,101-108,119-126,143-151` — parseResponse 检测 "error:" 前缀抛异常 |
| 40 | get/list/state/logs 抛 ServiceNotRunningException | PASS | `TaskManager.php:196-201` get 抛；`219-224` list 抛；`244-249` state 抛；`288-293` logs 抛 |
| 41 | result() 不抛异常（与 state() 不一致） | **FAIL** | `TaskManager.php:265-273` — result() 对 error 键不抛异常，与 state() 行为不一致；error 键可能是 daemon 不可达而非"结果不存在" |
| 42 | chainState/groupState/chordState 静默返回 null | **FAIL** | `TaskManager.php:394-402,411-419,432-440` — 未检测 "error:" 前缀，daemon 错误时 json_decode 返回 null → 方法返回 null，调用方无法区分"不存在"与"daemon 不可达" |
| 43 | pullEvents/inspect 静默返回 [] | **FAIL** | `TaskManager.php:471-479,488-496` — 检测到 "error:" 前缀但返回 [] 而非抛异常，与 get/list/logs 不一致 |
| 44 | remove() 抛 TaskNotFoundException | PASS | `TaskManager.php:359-368` — `!$ok` 时抛 TaskNotFoundException（注：IPC 失败也走此路径，语义有歧义但已有异常抛出） |
| 45 | waitForState/waitForResult 轮询容错 | PASS | `TaskManager.php:511-533,546-563` — catch `\Throwable` 继续轮询，适用于临时错误重试场景 |
| 46 | parseResponse 抛 InvalidTaskConfigException | PASS | `TaskManager.php:582-591` — 检测 "error:" 前缀抛异常含上下文 |

#### Client 重试与 fallback — 5 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 47 | callWithRetry 业务错误透传 + setTimeout 软约束 | PASS | `Client.php:360-362` — InvalidTaskConfigException 不重试直接抛；`Client.php:367-372` — setTimeout 超时抛 ServiceNotRunningException |
| 48 | callWithRetry fallback 吞异常无日志 | **FAIL** | `Client.php:378-382` — 重试耗尽且有 fallback 时直接 `return $fallback`，原始异常被静默丢弃，无日志记录 |
| 49 | callWithRetry func_num_args() < 2 逻辑 bug | **FAIL** | `Client.php:379` — `func_num_args() < 2` 条件使得显式传 `null` 作为 fallback 时不抛异常而返回 null |
| 50 | dispatch() 无 fallback 重试耗尽抛出 | PASS | `Client.php:99-104` — dispatch 调用 callWithRetry 不传 fallback，重试耗尽抛出 |
| 51 | 各方法 fallback 值（容错客户端设计） | PASS | `Client.php:145-329` — state/result 返回 []、get 返回 null、control 返回 false 属容错客户端设计行为（Client 类文档明确为"鲁棒性增强"） |

#### @throws 标注一致性 — 4 项

| # | 检查项 | 结果 | 证据 |
|---|---|---|---|
| 52 | TaskManager @throws 与行为一致 | PASS | create @throws InvalidTaskConfigException ✓；get/list/state/logs @throws ServiceNotRunningException ✓；remove @throws TaskNotFoundException ✓；result 无 @throws（文档说明返回 error 数组）✓ |
| 53 | TaskBuilder @throws 与行为一致 | PASS | `TaskBuilder.php:792` dispatch @throws InvalidTaskConfigException ✓；toJson 抛 InvalidTaskConfigException（无 @throws 标注但行为正确） |
| 54 | XhjobService @throws 与行为一致 | PASS | `XhjobService.php:74` start @throws ServiceNotRunningException ✓；`113` restart @throws ✓；`206` ensureRunning @throws ✓ |
| 55 | Client @throws 与行为一致 | PASS | `Client.php:348-349` callWithRetry @throws InvalidTaskConfigException + ServiceNotRunningException（注：fallback 路径不抛，但 @throws 描述的是无 fallback 场景） |

---

## 发现的缺陷清单（仅 FAIL 项）

| 缺陷 ID | 严重级别 | 文件:行号 | 描述 | 修复建议 |
|---|---|---|---|---|
| D-1 | HIGH | `src/store/sqlite.rs` (39 处: 324,383,400,417,438,458,486,504,521,538,555,577,597,617,659,682,726,764,812,846,898,915,966,988,1007,1039,1056,1089,1108,1139,1156,1189,1209,1242,1271,1290,1309,1328,1347) | `conn.lock().unwrap()` 在 Mutex 中毒时 panic，daemon 崩溃。生产路径 39 处。 | 改用 `conn.lock().unwrap_or_else(|e| e.into_inner())` 获取中毒锁的数据（容错），或改用 `parking_lot::Mutex`（不中毒），或封装 `lock_conn()` helper 统一处理 |
| D-2 | MEDIUM | `src/store/sqlite.rs:633` | `let _ = conn.execute(...)` 静默丢弃 Cancelled 事件 INSERT 错误，审计日志可能缺失 | 改为 `if let Err(e) = conn.execute(...) { tracing::warn!("record cancelled event failed: {}", e); }` |
| D-3 | MEDIUM | `src/store/sqlite.rs:256,259` | `serde_json::from_str(...).unwrap_or(Null)` 吞 payload JSON 解析错误；`TaskState::from_str(...).unwrap_or(Pending)` 吞状态损坏（可能导致已完成任务被重新执行） | 添加 `tracing::warn!("payload decode failed for task {}: {}", id, e)` 后再 fallback；TaskState 损坏应记录 warn 并考虑标记为 Failed 而非 Pending |
| D-4 | MEDIUM | `src/store/sqlite.rs:1013,1062,1114,1162,1215` | chain/group/chord 任务列表 `serde_json::from_str(...).unwrap_or_default()` 静默变空，掩盖 JSON 损坏 | 添加 `tracing::warn!` 记录解析失败后再 fallback |
| D-5 | LOW | `src/store/sqlite.rs:306,926,944` | tags `unwrap_or_default()` 静默变空；EventType `unwrap_or(Started)` 未知事件类型静默变 Started | 添加 `tracing::debug!` 或 `tracing::warn!` 记录异常值 |
| D-6 | LOW | `src/ipc/mod.rs:121` | `json.len() as u32` 若 JSON 序列化后 > 4GB 则长度静默截断（理论风险） | 添加 `if json.len() > u32::MAX as usize { return Err(XhjobError::ipc("frame too large")); }` |
| D-7 | MEDIUM | `Xhjob/TaskManager.php:265-273` | result() 对 error 键不抛异常，与 state() 行为不一致；error 键可能表示 daemon 不可达而非"结果不存在" | 区分 error 类型：daemon 不可达时抛 ServiceNotRunningException，结果不存在时返回带 error 键数组（保持当前行为） |
| D-8 | MEDIUM | `Xhjob/TaskManager.php:394-402,411-419,432-440` | chainState/groupState/chordState 未检测 "error:" 前缀，daemon 错误时静默返回 null | 添加 `if (strncmp($json, 'error:', 6) === 0) { throw new ServiceNotRunningException(...); }` 与 get/list 一致 |
| D-9 | MEDIUM | `Xhjob/TaskManager.php:471-479,488-496` | pullEvents/inspect 检测到 "error:" 前缀但返回 [] 而非抛异常，与 get/list/logs 不一致 | 改为 `throw new ServiceNotRunningException(...)` 或至少添加 `trigger_error` 日志 |
| D-10 | MEDIUM | `Xhjob/Client.php:378-382` | callWithRetry 重试耗尽且有 fallback 时直接 `return $fallback`，原始异常被静默丢弃无日志 | 添加日志：`trigger_error('callWithRetry fallback after retries: ' . $lastException->getMessage(), E_USER_WARNING);` 后再 return fallback |
| D-11 | LOW | `Xhjob/Client.php:379` | `func_num_args() < 2` 条件使得显式传 `null` 作为 fallback 时不抛异常而返回 null | 移除 `func_num_args() < 2` 条件，仅靠 `$fallback === null` 判断；或用哨兵值区分"未传"与"传 null" |

---

## 最终通过率

| 指标 | 数值 |
|---|---|
| 已检查项总数 | 55 |
| PASS 项数 | 44 |
| FAIL 项数（缺陷数） | 11 |
| **通过率** | **44 / 55 = 80.0%** |
| 目标通过率 | ≥ 97% |
| **是否达标** | **否（未达标，差 17 个百分点）** |

### 缺陷分布

- **Rust 侧**：6 项缺陷（1 HIGH + 3 MEDIUM + 2 LOW），集中在 `store/sqlite.rs` 的 Mutex unwrap 与 unwrap_or 静默吞错
- **PHP 侧**：5 项缺陷（0 HIGH + 4 MEDIUM + 1 LOW），集中在 TaskManager 的错误路径不一致与 Client 的 fallback 静默吞异常

### 未达标根因分析

1. **SQLite Mutex unwrap（D-1，HIGH）**：39 处 `conn.lock().unwrap()` 是最大的 panic 风险源，任一持锁线程 panic 将导致 daemon 崩溃
2. **SQLite 反序列化静默吞错（D-2/D-3/D-4/D-5）**：`task_from_row` 及 chain/group/chord 状态查询中大量 `unwrap_or` / `unwrap_or_default` 掩盖数据损坏，无日志记录
3. **PHP API 错误路径不一致（D-7/D-8/D-9）**：state() 抛异常但 result() 不抛；get/list 检测 "error:" 但 chainState/groupState/chordState 不检测；pullEvents/inspect 检测到但返回 [] 而非抛异常
4. **PHP Client fallback 无日志（D-10）**：容错客户端的 fallback 设计本身合理，但吞异常无日志导致问题难以排查

### 达标路径建议

修复全部 11 项缺陷后通过率可达 55/55 = 100%。优先修复顺序：
1. D-1（HIGH）→ 改用 `parking_lot::Mutex` 或 `unwrap_or_else(|e| e.into_inner())`
2. D-2/D-3/D-4（MEDIUM）→ 添加 `tracing::warn!` 日志
3. D-7/D-8/D-9/D-10（MEDIUM）→ 统一 PHP 错误路径 + 添加日志
4. D-5/D-6/D-11（LOW）→ 防御性改进
