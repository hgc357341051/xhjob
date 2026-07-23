# Checklist

## Task 4.1 类别 1: Result 正确传播（项 1-7）
- [x] 项 1 errors.rs XhjobError 枚举：thiserror + #[source] 链，Io/Ipc/Store/Exec/Config 变体完整
- [x] 项 2 errors.rs From<String> 路由到 ipc(s)，From<io::Error> 通过 #[from]
- [x] 项 3 errors.rs 提供 ipc()/ipc_with_source()/store()/exec() 等类型化构造器
- [x] 项 4 lib.rs:51-70 ipc_request 用 tokio::time::timeout(5s) 包裹防 PHP-FPM 死锁 + ? 传播
- [x] 项 5 lib.rs:1009-1022 dispatch() 错误返回 "error:" 前缀（P0-1 修复契约）
- [x] 项 6 ipc/mod.rs:118-149 write_frame/read_frame 用 XhjobError::ipc(format!("write len: {}", e)) 映射
- [x] 项 7 sqlite.rs/crypto.rs/cron.rs/http.rs/shell.rs 查询用 .map_err(|e| XhjobError::store(...))? + crypto ? + cron XhjobError::CronParse

## Task 4.1 类别 2: 错误信息可定位（项 8-12）
- [x] 项 8 errors.rs Ipc/Store/Exec/Config 变体含 context 字符串字段设计
- [x] 项 9 ipc/mod.rs:122-149 错误含 "write len"/"read len"/"serialize" op 上下文
- [x] 项 10 sqlite.rs:622 "cancel_task query: {}" / sqlite.rs:629 "cancel_task update: {}" 含方法名上下文
- [x] 项 11 lib.rs:65-68 "request timeout ({}s) for op={}" 含 op + timeout 上下文
- [x] 项 12 http.rs:51 "invalid proxy url (no scheme): {}" / "http send: {}" 含操作描述

## Task 4.1 类别 3: 无静默吞错（项 13-22）
- [x] 项 13 ipc fire-and-forget tx.send 合理（daemon_main.rs:1267-1268 关闭信号）
- [x] 项 14 daemon let _ = remove_file 最佳努力清理合理（daemon/mod.rs:81,98,105）
- [x] 项 15 shell let _ = child kill/wait 合理（shell.rs:64,100,108,124）
- [x] 项 16 queue let _ = shutdown_tx.send 合理（queue.rs:705 关闭信号）
- [x] 项 17 in_memory let _ = record_event 合理（in_memory.rs:180）
- [ ] 项 18 SQLite 事件 INSERT 静默吞错：**FAIL** D-2 (MEDIUM) — sqlite.rs:633 `let _ = conn.execute(...)` Cancelled 事件 INSERT 错误被丢弃，应改 `if let Err(e) = ... { tracing::warn!(...) }`
- [x] 项 19 sqlite.rs:252-255 maybe_decrypt 回退明文（文档化 backward compat with rows written before encryption）
- [ ] 项 20 payload/state unwrap_or 吞损坏：**FAIL** D-3 (MEDIUM) — sqlite.rs:256 `serde_json::from_str(...).unwrap_or(Null)` + sqlite.rs:259 `TaskState::from_str(...).unwrap_or(Pending)` 吞损坏，应加 tracing::warn!
- [ ] 项 21 tags/EventType unwrap_or：**FAIL** D-5 (LOW) — sqlite.rs:306 tags unwrap_or_default() 静默变空；sqlite.rs:926,944 EventType unwrap_or(Started) 未知类型静默变 Started
- [ ] 项 22 chain/group/chord list unwrap_or_default：**FAIL** D-4 (MEDIUM) — sqlite.rs:1013,1062,1114,1162,1215 任务列表 JSON 解析失败静默变空，掩盖损坏

## Task 4.1 类别 4: panic 风险（项 23-29）
- [ ] 项 23 SQLite Mutex unwrap 中毒 panic：**FAIL** D-1 (HIGH) — sqlite.rs 39 处 `conn.lock().unwrap()` 在 Mutex 中毒时 panic 导致 daemon 崩溃，应改 `unwrap_or_else(|e| e.into_inner())` 或 parking_lot::Mutex
- [ ] 项 24 IPC json.len() as u32 截断：**FAIL** D-6 (LOW) — ipc/mod.rs:121 `json.len() as u32` 若 JSON > 4GB 静默截断（理论风险），应加上限检查
- [x] 项 25 daemon_main.rs:1264-1265 expect SIGTERM/SIGINT 信号安装失败属启动期 panic，合理
- [x] 项 26 coroutine_pool.rs:88 / thread_pool.rs:77 expect runtime/spawn 创建失败属启动期，合理
- [x] 项 27 shell.rs:454 / cron.rs:575 / task/mod.rs:861 expect/unwrap 全部在 #[cfg(test)] 块内
- [x] 项 28 http.rs:217 String::from_utf8(body_buf) 检测无效 UTF-8 后走 base64 编码（非 from_utf8_lossy）
- [x] 项 29 shell.rs:235 status.code().unwrap_or(-1) / cron.rs:415 now_ts().unwrap_or(0) 安全回退

## Task 4.1 类别 5: 外部边界错误处理（项 30-35）
- [x] 项 30 HTTP 边界：http.rs:119-141 connect_timeout 10s + redirect 5 + http.rs:180 MAX_BODY_BYTES 64MiB + http.rs:217 binary→base64 + cancel 200ms 轮询 + http.rs:40-64 proxy 校验
- [x] 项 31 Shell 边界：shell.rs:29-277 soft_timeout→SIGTERM→grace→SIGKILL + kill_on_drop(true) + process_group(0) + env_clear() + shell.rs:147 MAX_OUTPUT_BYTES 64MiB
- [x] 项 32 SQLite 边界：sqlite.rs:19-22 WAL + synchronous=NORMAL + busy_timeout=5000ms；全部查询用 params![] 参数化
- [x] 项 33 IPC 边界：ipc/mod.rs:141 8MB 帧上限 + unix_socket.rs:13-49 fchmod（TOCTOU-safe）+ unix_socket.rs:91-115 SO_PEERCRED 认证
- [x] 项 34 timeout 双层防护：lib.rs:51-70 ipc_request timeout 5s 防 PHP-FPM 死锁 + daemon_main.rs:345-360 每连接 15s timeout
- [x] 项 35 daemon_main.rs:305-310 XHJOB_MAX_CONNECTIONS 信号量 + daemon_main.rs:498-566 ownership_check 默认拒绝多租户隔离

## Task 4.2 异常类型层级（项 36-38）
- [x] 项 36 XhjobException.php extends \Exception；3 子类均 extends XhjobException；catch XhjobException 可统一捕获
- [x] 项 37 异常类型语义正确：InvalidTaskConfigException（配置/dispatch）、ServiceNotRunningException（daemon 不可达）、TaskNotFoundException（任务不存在）
- [x] 项 38 facade/Xhjob.php:27-47 @method 标注与 TaskManager 方法签名一致

## Task 4.2 TaskManager 错误路径（项 39-46）
- [x] 项 39 TaskManager.php:86-90,101-108,119-126,143-151 create/chain/group/chord dispatch parseResponse 检测 "error:" 抛 InvalidTaskConfigException
- [x] 项 40 TaskManager.php:196-201 get / 219-224 list / 244-249 state / 288-293 logs 检测 "error:" 抛 ServiceNotRunningException
- [ ] 项 41 result() 不抛异常：**FAIL** D-7 (MEDIUM) — TaskManager.php:265-273 result() 对 error 键不抛异常，与 state() 行为不一致；error 键可能是 daemon 不可达
- [ ] 项 42 chainState/groupState/chordState 静默返回 null：**FAIL** D-8 (MEDIUM) — TaskManager.php:394-402,411-419,432-440 未检测 "error:" 前缀，daemon 错误时返回 null 无法区分
- [ ] 项 43 pullEvents/inspect 静默返回 []：**FAIL** D-9 (MEDIUM) — TaskManager.php:471-479,488-496 检测到 "error:" 但返回 [] 而非抛异常，与 get/list/logs 不一致
- [x] 项 44 TaskManager.php:359-368 remove() !$ok 时抛 TaskNotFoundException（IPC 失败也走此路径，语义有歧义但已有异常抛出）
- [x] 项 45 TaskManager.php:511-533,546-563 waitForState/waitForResult catch \Throwable 继续轮询，适用于临时错误重试
- [x] 项 46 TaskManager.php:582-591 parseResponse 检测 "error:" 前缀抛 InvalidTaskConfigException 含上下文

## Task 4.2 Client 重试与 fallback（项 47-51）
- [x] 项 47 Client.php:360-362 InvalidTaskConfigException 不重试直接抛；Client.php:367-372 setTimeout 超时抛 ServiceNotRunningException
- [ ] 项 48 callWithRetry fallback 吞异常无日志：**FAIL** D-10 (MEDIUM) — Client.php:378-382 重试耗尽且有 fallback 时直接 return $fallback，原始异常静默丢弃无日志，应加 trigger_error
- [ ] 项 49 callWithRetry func_num_args bug：**FAIL** D-11 (LOW) — Client.php:379 `func_num_args() < 2` 使显式传 null 作为 fallback 时不抛异常而返回 null
- [x] 项 50 Client.php:99-104 dispatch 调用 callWithRetry 不传 fallback，重试耗尽抛出
- [x] 项 51 Client.php:145-329 state/result 返回 []、get 返回 null、control 返回 false 属容错客户端设计（Client 文档明确为"鲁棒性增强"）

## Task 4.2 @throws 标注一致性（项 52-55）
- [x] 项 52 TaskManager @throws 与行为一致：create @throws InvalidTaskConfigException ✓；get/list/state/logs @throws ServiceNotRunningException ✓；remove @throws TaskNotFoundException ✓；result 无 @throws（文档说明返回 error 数组）✓
- [x] 项 53 TaskBuilder.php:792 dispatch @throws InvalidTaskConfigException ✓；toJson 抛 InvalidTaskConfigException（无 @throws 标注但行为正确）
- [x] 项 54 XhjobService.php:74 start @throws ServiceNotRunningException ✓；:113 restart @throws ✓；:206 ensureRunning @throws ✓
- [x] 项 55 Client.php:348-349 callWithRetry @throws InvalidTaskConfigException + ServiceNotRunningException（注：fallback 路径不抛，但 @throws 描述的是无 fallback 场景）

## 通过率
- [x] PASS 项数 = 44
- [x] FAIL 项数 = 11（D-1 HIGH, D-2/D-3/D-4/D-7/D-8/D-9/D-10 MEDIUM, D-5/D-6/D-11 LOW）
- [x] 通过率 = 44/55 = 80.0% < 97% 目标 ✗（未达标，差 17 个百分点）

### FAIL 项与缺陷 ID 映射
- 项 18 → D-2 (MEDIUM) sqlite.rs:633 事件 INSERT 静默吞错
- 项 20 → D-3 (MEDIUM) sqlite.rs:256,259 payload/state unwrap_or 吞损坏
- 项 21 → D-5 (LOW) sqlite.rs:306,926,944 tags/EventType unwrap_or
- 项 22 → D-4 (MEDIUM) sqlite.rs:1013,1062,1114,1162,1215 chain/group/chord unwrap_or_default
- 项 23 → D-1 (HIGH) sqlite.rs 39 处 conn.lock().unwrap() Mutex 中毒 panic
- 项 24 → D-6 (LOW) ipc/mod.rs:121 json.len() as u32 截断
- 项 41 → D-7 (MEDIUM) TaskManager.php:265-273 result() 不抛异常
- 项 42 → D-8 (MEDIUM) TaskManager.php:394-440 chainState/groupState/chordState 静默返回 null
- 项 43 → D-9 (MEDIUM) TaskManager.php:471-496 pullEvents/inspect 返回 []
- 项 48 → D-10 (MEDIUM) Client.php:378-382 callWithRetry fallback 吞异常无日志
- 项 49 → D-11 (LOW) Client.php:379 func_num_args() < 2 逻辑 bug
