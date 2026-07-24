# Tasks — production-resilience-audit-v2

> 分阶段执行，每阶段交付可验证产物。Phase 1-2 可并行，Phase 3 依赖 1-2，Phase 4-5 可并行，Phase 6 依赖全部，Phase 7 收尾。

## Phase 1: Rust 核心容灾新增功能

- [x] Task 1: 新增 Running 任务 watchdog 机制
  - [x] SubTask 1.1: 在 `src/scheduler/` 新增 `watchdog.rs`，实现 `Watchdog::start(store, queue, interval, factor)` 周期扫描 Running 任务，`now - started_at > timeout * factor` 时 `signal_cancel` + `update_state(Interrupted)` + `record_event(HungDetected)`
  - [x] SubTask 1.2: `Task` 结构（`src/store/mod.rs`）无新字段（复用 started_at + timeout），但需确保 `load_active_tasks` 能返回 Running 任务（当前已支持）
  - [x] SubTask 1.3: `daemon_main.rs` 启动 watchdog（读 `XHJOB_WATCHDOG_INTERVAL` 默认 5s、`XHJOB_WATCHDOG_FACTOR` 默认 2，interval=0 不启动），shutdown 时停止
  - [x] SubTask 1.4: 新增 `EventType::HungDetected` 变体（`src/store/mod.rs`）
  - [x] SubTask 1.5: 单测 `test_watchdog_interrupts_hung_task`（mock store + sleep 任务）、`test_watchdog_skips_normal_task`、`test_watchdog_disabled_when_interval_zero`

- [x] Task 2: 新增 execution_lease 防重复执行
  - [x] SubTask 2.1: `Task` 结构（`src/store/mod.rs`）新增 `worker_pid: Option<u32>` 字段（serde default None）
  - [x] SubTask 2.2: SQLite schema 加列 `worker_pid INTEGER`（`src/store/sqlite.rs`，含 `ensure_column` 迁移）；`task_from_row`/`insert_task`/`update_state` 读写该字段
  - [x] SubTask 2.3: `scheduler/queue.rs` dispatch 时在 `overlap.on_start` 后写入 `worker_pid`（Shell 任务取 child pid，HTTP 任务取 None）
  - [x] SubTask 2.4: `reset_running_to_pending`（sqlite.rs + in_memory.rs）重置前检查 `worker_pid` 存活（调用增强版 `is_process_alive_with_starttime`），存活则跳过 + 记录 `EventType::LeaseHeld`
  - [x] SubTask 2.5: 新增 `EventType::LeaseHeld` 变体
  - [x] SubTask 2.6: 单测 `test_reset_skips_task_with_alive_worker`、`test_reset_resets_task_with_dead_worker`、`test_reset_resets_task_without_worker_pid`

- [ ] Task 3: 修复 PID 复用风险（starttime 校验）
  - [ ] SubTask 3.1: `src/daemon/mod.rs` 新增 `fn process_starttime(pid: u32) -> Option<u64>`（Linux 读 `/proc/<pid>/stat` 第 22 字段，非 Linux 返回 None）
  - [ ] SubTask 3.2: 新增 `fn is_process_alive_with_starttime(pid, expected_starttime: Option<u64>) -> bool`，pid 存活且（starttime 匹配 或 expected 为 None）才返回 true；`is_process_alive` 改为调用此函数传 None
  - [ ] SubTask 3.3: `write_pid` 同时写 `pid\nstarttime`（`daemon/unix.rs` `daemon_started`）；`read_pid` 解析两行返回 `(pid, Option<u64>)`，向后兼容旧单行格式（starttime=None）
  - [ ] SubTask 3.4: `send_terminate`/`status`/`start` 用 `is_process_alive_with_starttime` 校验身份
  - [ ] SubTask 3.5: 单测 `test_pid_reuse_rejected_by_starttime`（mock starttime 不匹配）、`test_legacy_pid_file_backward_compat`（单行格式）

- [x] Task 4: SQLite 启动完整性检查 + 外键约束
  - [x] SubTask 4.1: `src/store/sqlite.rs` `open` 在 schema 创建后执行 `PRAGMA quick_check`，失败返回 `XhjobError::store("integrity check failed: ...")`
  - [x] SubTask 4.2: `open` 增加 `PRAGMA foreign_keys=ON`
  - [x] SubTask 4.3: 单测 `test_integrity_check_fails_on_corrupt_db`（写坏 DB 文件验证报错）、`test_foreign_keys_enforced`（删 task 验证 result 约束）

- [ ] Task 5: send_terminate 超时对齐 drain
  - [ ] SubTask 5.1: `src/daemon/mod.rs` `send_terminate` 的 SIGKILL 等待从固定 10s 改为 `max(10, XHJOB_SHUTDOWN_DRAIN_SECS + 5)`（读 env，默认 35）
  - [ ] SubTask 5.2: 单测 `test_send_terminate_waits_for_drain_window`

## Phase 2: Rust 测试补全（无依赖，可与 Phase 1 并行）

- [ ] Task 6: 补全 executor/scheduler 单测缺口
  - [ ] SubTask 6.1: `src/executor/http.rs` 新增 `test_execute_with_cancel_aborts_request`（派发长 HTTP + cancel，验证返回 "cancelled during http request"）
  - [ ] SubTask 6.2: `src/executor/shell.rs` 新增 `test_hard_timeout_sigkill`（sleep 30 + timeout=2 无 soft_timeout，验证 Err "timeout after 2s" + 子进程被 reap）
  - [x] SubTask 6.3: `src/scheduler/queue.rs` 新增 `test_inflight_guard_balanced_on_abort`（spawn counted_future + abort JoinHandle，验证 in_flight_count 归 0）
  - [x] SubTask 6.4: `src/store/sqlite.rs` 新增 `SqliteStore` 崩溃恢复集成测试：写 Running 任务 + worker_pid → drop store → 重新 open → 验证 reset 行为（含 lease check）

- [ ] Task 7: soft_timeout HTTP 语义文档化与测试锁定
  - [ ] SubTask 7.1: `src/task/mod.rs` TaskBuilder 对 HTTP 任务保留 soft_timeout 字段但 inspect 输出标注 `soft_timeout_unsupported: true`（在 Task 加 `soft_timeout_unsupported: bool` 计算字段 或 在 inspect 时判断 task_type）
  - [ ] SubTask 7.2: 单测 `test_http_soft_timeout_marked_unsupported`、`test_shell_soft_timeout_supported`

## Phase 3: Rust 编译与全量测试（依赖 Phase 1-2）

- [ ] Task 8: cargo 全量验证
  - [ ] SubTask 8.1: `cargo build --release --features persist` 编译通过，无 warning
  - [ ] SubTask 8.2: `cargo test --features persist` 100% 通过（含新增所有单测）
  - [ ] SubTask 8.3: `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
  - [ ] SubTask 8.4: 复制 `target/release/libxhjob.so` 到 `releases/xhjob-php8.2-linux-x86_64.so`

## Phase 4: tp/repro/ 复现脚本（依赖 Phase 3 的 .so，可与 Phase 5 并行）

- [x] Task 9: 搭建 tp/repro/ 框架
  - [x] SubTask 9.1: 创建 `tp/repro/` 目录 + `runner.php`（扫描 `repro_*.php` 依次执行，捕获输出，统计 PASS/FAIL，支持 `--only=<编号>` 过滤）
  - [x] SubTask 9.2: 创建 `tp/repro/_bootstrap.php` 公共启动逻辑（加载扩展 + 启动/停止 daemon 的 helper，复用 `xhjob_server.php` 模式）

- [x] Task 10: 编写 11 个复现脚本
  - [x] SubTask 10.1: `repro_01_task_hang_watchdog.php` — 派发 `bash -c "read x < /tmp/xhjob_fifo_$$"` + timeout=2，验证 ~4s 后状态 interrupted + 事件 HungDetected
  - [x] SubTask 10.2: `repro_02_pid_reuse_safety.php` — 启动 daemon 记录 pid+starttime，kill -9 daemon，启动一个占位进程占用同 PID（或模拟），验证 xhjob_stop 不误杀（starttime 不匹配）
  - [x] SubTask 10.3: `repro_03_crash_no_duplicate_exec.php` — 派发 sleep 30 + acks_late + persist，等进入 Running 记录 worker_pid，kill -9 daemon，重启，验证新 daemon 不重复派发（LeaseHeld 事件）+ 等子进程结束后任务完成
  - [x] SubTask 10.4: `repro_04_sqlite_integrity_check.php` — 启动 persist daemon 写任务，停止，用 `dd` 破坏 DB 文件中部，重启验证报错 "integrity check failed"
  - [x] SubTask 10.5: `repro_05_send_terminate_drain_align.php` — 派发 sleep 15 + timeout=20，xhjob_stop，验证任务正常完成（drain 未被打断）或标记 interrupted（非 Running 残留）
  - [x] SubTask 10.6: `repro_06_http_cancel.php` — 派发长 HTTP 任务（假 server sleep 30），cancel，验证状态 cancelled + 无 panic
  - [x] SubTask 10.7: `repro_07_hard_timeout_sigkill.php` — 派发 sleep 30 + timeout=2 无 soft_timeout，验证 2s 后 failed + 错误含 "timeout after 2s" + 子进程已 reap（ps 验证）
  - [x] SubTask 10.8: `repro_08_foreign_keys.php` — 启动 persist daemon，插 task + result，直接用 sqlite3 CLI 删 task，验证外键约束拒绝（或级联）
  - [ ] SubTask 10.9: `repro_09_api_token_auth.php` — 配置 api_token，通过 HTTP 调用 /xhjob/list 验证无 token/错 token 返回 401，正确 token 放行
  - [ ] SubTask 10.10: `repro_10_http_controller_integration.php` — 启动 ThinkPHP 内置 server，curl 调用 23 个 /xhjob/* 接口，验证响应结构
  - [x] SubTask 10.11: `repro_11_daemon_sigkill_recovery.php` — 启动 persist daemon + 派发 cron 任务，kill -9 daemon，验证 PID 文件清理 + 重启后任务恢复 + SQLite 一致性（quick_check 通过）

## Phase 5: tp 业务层加固（可与 Phase 4 并行）

- [ ] Task 11: api_token 鉴权中间件
  - [ ] SubTask 11.1: 新建 `tp/app/middleware/XhjobAuth.php`，读 `config('xhjob.api_token')`，校验 `X-Xhjob-Token` header 或 `?token=` query，不匹配 throw 401 HttpResponseException；api_token 为空时放行
  - [ ] SubTask 11.2: `tp/route/app.php` 给 `/xhjob` 路由组挂载 `XhjobAuth` 中间件
  - [ ] SubTask 11.3: 验证中间件在 api_token 为空时向后兼容（现有测试不挂）

- [ ] Task 12: tp 项目可运行性修复
  - [ ] SubTask 12.1: 建立 `tp/extend/Xhjob` → `releases/xhjob-thinkphp8-extend/Xhjob` 符号链接（`ln -sf`），让 PSR-0 解析 `Xhjob\*` 类
  - [ ] SubTask 12.2: 执行 `composer install`（在 tp/ 下）安装 topthink 框架
  - [ ] SubTask 12.3: 验证 `php think` 可运行，`XhjobTask` 控制器可加载

## Phase 6: 跨进程生产模拟测试（依赖 Phase 3-5）

- [ ] Task 13: 增强跨进程测试套件
  - [ ] SubTask 13.1: 在 `tp/xhjob_client_test.php` 基础上新增 SubTask 7.9「容灾场景」，包含：watchdog 假死恢复、SIGKILL 崩溃恢复、PID 复用防护、execution_lease 防重复、SQLite 损坏检测
  - [ ] SubTask 13.2: 运行完整跨进程测试，验证 100%PASS（原 43 + 新增容灾场景，2 SKIP 保留）
  - [x] SubTask 13.3: 运行 `tp/repro/runner.php`，验证全部复现脚本 PASS（55 PASS / 0 FAIL / 6 SKIP，runner exit 0）

## Phase 7: 分维度验证与提交（依赖全部）

- [ ] Task 14: 分维度 100% 验证
  - [ ] SubTask 14.1: 正确率维度 — `cargo test` 100% + `tp/repro/runner.php` 100% PASS + 跨进程测试 100% PASS
  - [ ] SubTask 14.2: 代码质量维度 — `cargo clippy -D warnings` 无 warning + 审查清单逐项核对
  - [ ] SubTask 14.3: 功能完善度维度 — watchdog/lease/PID/integrity/api_token/23 接口测试 全部生效
  - [ ] SubTask 14.4: bug率维度 — 审查问题清单逐项验证 0 未修复
  - [ ] SubTask 14.5: 错误率维度 — 跨进程测试运行期间 0 panic / 0 未捕获 error 日志

- [ ] Task 15: 编译提交
  - [ ] SubTask 15.1: `cargo build --release --features persist` 最终编译
  - [ ] SubTask 15.2: 复制 .so 到 `releases/xhjob-php8.2-linux-x86_64.so`
  - [ ] SubTask 15.3: `git add` 所有修改的 Rust 源码 + tp/ 新增文件 + .so
  - [ ] SubTask 15.4: `git commit` 详细的分维度修复说明
  - [ ] SubTask 15.5: `git push origin main`（需 GitHub 认证，若无则告知用户手动 push）

# Task Dependencies

- Task 1-7（Phase 1-2）可全部并行（无相互依赖）
- Task 8（Phase 3）依赖 Task 1-7 全部完成
- Task 9-12（Phase 4-5）依赖 Task 8（需要新 .so），Task 9-12 之间可并行
- Task 13（Phase 6）依赖 Task 9-12
- Task 14-15（Phase 7）依赖 Task 13

# 风险点

- **PID starttime 校验跨平台**：Linux 读 `/proc/<pid>/stat`，macOS 需 `proc_pidinfo`，Windows 无等价物 → 非 Linux 平台 fallback 为 None（退化为原行为），仅 Linux 启用强校验
- **execution_lease 与 acks_late 语义**：当前 reset 无条件重置所有 Running（C1 fix 覆盖了 C10 acks_late 语义）。引入 lease check 后，acks_late=false 但 worker_pid 存活的任务也不重置 —— 需确认这是否符合预期（应该是，因为重复执行比延迟执行危害更大）
- **watchdog 误杀风险**：factor 默认 2 提供 timeout 的 2 倍容差，但若用户设 timeout=0（无超时）则 watchdog 不应干预 → 需在 watchdog 逻辑中跳过 timeout=0 的任务
- **tp 符号链接**：`tp/extend/Xhjob` 符号链接在 Windows 下可能不工作，但项目目标平台是 Linux，可接受
