# Xhjob 全量代码复审与生产韧性验证 Spec

## Why
xhjob（Rust + ext-php-rs 0.15 PHP 异步任务调度扩展）经过一轮修改后，需要**完全不参考原 spec** 重新独立审查全部代码，确保在「正常使用而非伪代码」场景下，正确率、代码质量与功能完善度、bug 率、错误率四个维度通过率均达到 **100%**。同时需要针对「任务假死」与「框架崩溃」两个生产事故场景，在 tp 项目里写代码模拟生产业务逻辑验证修复效果。所有提出的问题必须用可复现的代码证明其存在，再用代码证明其被修复；对不理解的问题必须先询问用户，不得瞎猜。

## What Changes
- **Rust 核心层全量复审**：src/ 下 37 个源文件（config / daemon / executor / ipc / outcome / pool / retry / scheduler / service / store / task / utils / lib.rs / daemon_main.rs / errors.rs），逐文件检查正确性、panic 安全（panic=abort 下 extern "C" panic=UB）、状态机一致性、资源泄漏、整数溢出、并发安全。
- **tp 业务层全量复审**：app/controller/XhjobTask.php、app/middleware/XhjobAuth.php、config/xhjob.php、route/app.php、releases/xhjob-thinkphp8-extend/ 下 Client.php / XhjobService.php / TaskBuilder.php / TaskManager.php / ServiceProvider.php / helper.php / facade、config/xhjob.php。
- **cargo clippy 零警告**：当前存在 12 个 clippy 错误（cron.rs double_parens 1 个 + retry/mod.rs field_reassign_with_default 11 个），必须全部修复至 `cargo clippy -- -D warnings` 零警告。
- **execution_lease bug 验证与修复**：worker_pid 当前只在任务完成后写入 store，导致崩溃恢复时无法检测孤儿进程、无法避免重复派发。需在 spawn 时写入 worker_pid，并验证 PID + starttime 双重校验防 PID 复用。
- **任务假死（watchdog）验证**：复现子进程卡在 IO 不触发 tokio timeout 的场景，验证 watchdog 扫描 Running 任务超时后标记 Interrupted 并取消。
- **框架崩溃（SIGKILL 恢复）验证**：复现 daemon 被 `kill -9` 异常退出后，重启时通过 worker_pid + starttime 检测孤儿进程存活，不重复派发（acks_late 任务除外）。
- **tp/repro/ 复现脚本**：所有问题用 PHP + Rust 混合脚本在 tp/repro/ 目录复现并验证修复，每脚本输出 `=== repro: N PASS / M FAIL / K SKIP ===` 汇总行供 runner.php 批量统计。
- **分维度 100% 通过门槛**：四个维度独立判定，任一维度未达 100% 即审查未完成。

## Impact
- Affected code:
  - Rust 核心：`src/scheduler/queue.rs`（worker_pid 写入时机）、`src/scheduler/watchdog.rs`（假死扫描）、`src/daemon/mod.rs`（PID starttime 校验 + 崩溃恢复）、`src/store/mod.rs`（TaskStore trait + EventType）、`src/store/sqlite.rs`（完整性检查 + 外键）、`src/store/in_memory.rs`（worker_pid 支持）、`src/retry/mod.rs`（clippy 修复）、`src/scheduler/cron.rs`（clippy 修复）、`src/executor/mod.rs`（dispatch 返回 worker_pid）、`src/executor/shell.rs` / `http.rs`（worker_pid 透传）。
  - tp 业务层：`tp/app/middleware/XhjobAuth.php`、`tp/config/xhjob.php`、`tp/route/app.php`、`tp/repro/*.php`。
- Affected tests：`tp/repro/runner.php` 批量执行框架、`tp/repro/_bootstrap.php` 公共启动逻辑、`tp/xhjob_server.php` 跨进程测试服务启动器。

## ADDED Requirements

### Requirement: Rust 核心层正确性（正确率 100%）
系统 SHALL 在「正常使用」场景下（PHP-FPM 调用 xhjob_* 函数 + daemon 后台执行 HTTP/Shell 任务）所有 API 行为符合 lib.rs 中文档化的契约：dispatch 返回 task_id 或 `error:` 前缀字符串；state 返回小写状态名（pending/running/success/failed/interrupted/cancelled/expired）；result 返回 stdout/stderr/exit_code 或 body/status_code；list/get/chain/group/chord 系列 JSON 字符串可被 json_decode 还原。

#### Scenario: 正常 HTTP 任务执行
- **WHEN** PHP-FPM 调用 `xhjob_dispatch` 派发一个 HTTP GET 任务到可达 URL
- **THEN** 返回非 `error:` 前缀的 task_id
- **AND** 轮询 `xhjob_state` 最终返回 `success`
- **AND** `xhjob_result` 返回 status_code=200 与 body

#### Scenario: 正常 Shell 任务执行
- **WHEN** 派发 `echo hello` shell 任务
- **THEN** 最终 state=success，result.stdout=`hello\n`，exit_code=0

#### Scenario: 服务名非法
- **WHEN** 传入包含路径分隔符或空字符的 service name
- **THEN** dispatch 返回 `error: ...`，state 返回 state=UNKNOWN + error 字段

### Requirement: 代码质量与功能完善度 100%
系统 SHALL 满足：(1) `cargo clippy --all-targets -- -D warnings` 零警告；(2) `cargo build --release` 成功；(3) lib.rs 中所有 `#[php_function]` 与 `Xhjob` 类方法均有对应 tp 业务层封装或控制器路由；(4) panic=abort profile 下所有 extern "C" 边界（PHP 可调用函数）不得 panic，错误以返回值/`error:` 字符串表达。

#### Scenario: clippy 零警告
- **WHEN** 执行 `cargo clippy --all-targets -- -D warnings`
- **THEN** 退出码 0，无任何 warning/error 输出

#### Scenario: panic 边界安全
- **WHEN** PHP 传入非法参数（如超长 id、非 UTF-8 字符串、负数转 u32 溢出）
- **THEN** 函数返回 false / `error:` / None，不得触发进程 abort

### Requirement: bug 率 0%（已发现问题全部修复）
系统 SHALL 修复复审中发现的所有 bug，且每个 bug 必须有对应的 tp/repro/ 复现脚本证明修复前 FAIL、修复后 PASS。已知必须修复的 bug：
1. **execution_lease 失效**：worker_pid 只在任务完成后写入，spawn 时不写，导致崩溃恢复无法检测孤儿进程。
2. **clippy 12 错误**：cron.rs:939 double_parens、retry/mod.rs 多处 field_reassign_with_default。

#### Scenario: execution_lease 修复
- **WHEN** 派发一个 shell 任务并立即在 store 中查询该任务的 worker_pid
- **THEN** worker_pid 非 NULL（在 spawn 时即写入）
- **AND** daemon 被 kill -9 后重启，reset_running_to_pending 检测到 worker_pid 对应进程已死才重置为 Pending

#### Scenario: clippy 修复
- **WHEN** 执行 `cargo clippy --all-targets -- -D warnings`
- **THEN** 退出码 0

### Requirement: 错误率 0%（所有 repro 脚本 PASS）
系统 SHALL 通过 tp/repro/ 下所有复现脚本的验证。runner.php 批量执行所有 repro_*.php，汇总行 `=== repro: N PASS / M FAIL / K SKIP ===` 中 M 必须 = 0（FAIL 不允许，SKIP 需在脚本中注明合理原因并经用户确认）。

#### Scenario: 全量 repro 通过
- **WHEN** 执行 `php tp/repro/runner.php`
- **THEN** 所有 repro_*.php 汇总行 FAIL 计数为 0

### Requirement: 任务假死检测（watchdog）
系统 SHALL 在 daemon 运行期间周期性（默认每 5s，可配置）扫描所有 Running 状态任务，当 `now - started_at > timeout * factor`（factor 默认 2，可配置）时：调用 `signal_cancel` 终止子进程、将状态置为 Interrupted、记录 HungDetected 事件。timeout=0 的任务不扫描（无超时限制）。

#### Scenario: 子进程卡在 IO 不退出
- **WHEN** 派发一个 `sleep 100` shell 任务，timeout=2，watchdog factor=2
- **AND** tokio timeout 因子进程在 sleep 系统调用中卡住而未及时触发（或 daemon 调度延迟）
- **THEN** watchdog 在 `2 * 2 = 4s` 后扫描到该任务超时
- **AND** 调用 signal_cancel 终止子进程
- **AND** 任务状态变为 interrupted
- **AND** 事件流中出现 HungDetected 事件

### Requirement: 框架崩溃恢复（SIGKILL 不重复派发）
系统 SHALL 在 daemon 被 SIGKILL/OOM 异常退出后，重启时通过执行租约（execution lease）机制避免重复派发：
1. spawn 子进程后立即将 worker_pid 写入 store。
2. 重启时扫描所有 Running 任务，对每个任务读取 worker_pid + 该 pid 的 starttime。
3. 若 pid 对应进程已死（kill(pid,0)!=0 或 starttime 不匹配）→ 视为孤儿，reset_running_to_pending（acks_late=true 才重置，否则保持 Running 等待人工干预或超时）。
4. 若 pid 对应进程仍存活且 starttime 匹配 → 记录 LeaseHeld 事件，不重复派发（避免双执行）。
5. PID 复用防护：仅校验 pid 不够，必须同时校验 /proc/<pid>/stat 的 starttime 与 spawn 时记录的一致。

#### Scenario: daemon 被 kill -9 后重启不重复派发
- **WHEN** 派发一个长时间 shell 任务（如 `sleep 30`），任务进入 Running
- **AND** daemon 进程被 `kill -9` 杀死（子进程被 init 收养仍存活）
- **AND** 重启 daemon
- **THEN** daemon 检测到 worker_pid 对应子进程仍存活
- **AND** 记录 LeaseHeld 事件
- **AND** 不重新派发该任务（避免双执行）

#### Scenario: PID 复用防护
- **WHEN** daemon 崩溃后，原 worker_pid 被系统复用给一个无关进程（如另一个 shell）
- **AND** 重启 daemon
- **THEN** starttime 校验发现不匹配
- **AND** 视为孤儿进程，reset_running_to_pending（仅 acks_late=true 时）
- **AND** 不杀无害进程

### Requirement: tp/repro/ 复现脚本规范
所有 repro 脚本 SHALL：
1. 位于 `tp/repro/repro_NN_<name>.php`，编号两位补零。
2. 顶部 `require __DIR__ . '/_bootstrap.php';` 复用扩展加载、daemon 启停、断言输出。
3. 使用 `repro_header($n, $title)` 输出分割线，`repro_step($name, $cond, $detail)` 断言，`repro_summary()` 输出汇总行。
4. 每个脚本独立启停 daemon（独立 service + data_dir），不互相污染。
5. 脚本结束时 `stop_daemon()` + `cleanup_data_dir()` 清理残留。
6. SKIP 必须在 detail 中注明原因，且需用户确认该 SKIP 合理（如某能力确属 Rust 内部无法在 PHP 层触发）。

#### Scenario: repro 脚本规范
- **WHEN** 检查 tp/repro/ 下所有 repro_*.php
- **THEN** 文件名匹配 `repro_NN_*.php`
- **AND** 顶部 require _bootstrap.php
- **AND** 调用 repro_header / repro_step / repro_summary
- **AND** 结束时调用 stop_daemon + cleanup_data_dir

## MODIFIED Requirements

### Requirement: TaskStore trait 扩展 worker_pid 写入
TaskStore trait SHALL 新增 `update_worker_pid(&self, task_id: &str, worker_pid: u32) -> Result<()>` 方法，在任务 spawn 后立即调用以写入 worker_pid。InMemoryStore 与 SqliteStore 均需实现。SQLite 表结构需新增 `worker_pid INTEGER` 列与 `worker_starttime INTEGER` 列（用于 PID 复用防护）。

#### Scenario: worker_pid 写入时机
- **WHEN** process_one 派发任务并拿到子进程 pid
- **THEN** 立即调用 store.update_worker_pid(task_id, pid)
- **AND** 此时任务状态为 Running
- **AND** 后续崩溃恢复可读取该 worker_pid

## REMOVED Requirements
无（本次为纯增补与修复，不删除既有能力）。
