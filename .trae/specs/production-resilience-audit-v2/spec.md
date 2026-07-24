# 生产环境容灾与全量代码审查 Spec v2

> change-id: `production-resilience-audit-v2`
> 范围：Rust 核心扩展（src/）+ tp 业务层（tp/）
> 不参考原 spec，基于本轮全量重新审查产出

## Why

前一轮深度审查（commit 4b1ee5b）修复了 11 个 bug，但仅触及部分模块，且对用户明确提出的两大生产容灾场景——**任务假死**与**框架崩溃**——完全没有覆盖。调研发现：

- **任务假死**：daemon 运行期间**没有任何** Running 任务卡死检测机制（`task.started_at` 已持久化但无代码计算 `now - started_at > timeout`；`scan_once` 显式跳过 Running）。子进程 hang 在 IO（卡读 fifo、卡 NFS、卡 DNS）且 timeout 设得大时，task 永远 Running，需重启 daemon 才能恢复。
- **框架崩溃**：`reset_running_to_pending` 无视孤儿子进程存活，**有重复执行风险**（无 fencing token / execution lease）；`is_process_alive` 用 `kill(pid, 0)` 无 PID + starttime 校验，**有 PID 复用风险**（可能 SIGKILL 无关进程）；SQLite open **无 integrity_check**、外键约束未启用；`send_terminate` 只等 10s SIGKILL 但 daemon drain 默认 30s，drain 被打断。
- **tp 业务层**：`XhjobTask` 控制器 23 个 HTTP 接口**零测试覆盖**；`api_token` 配置存在但控制器未校验（裸奔）；`vendor/` 不存在、`extend/` 为空，控制器一加载就 Class not found。

本轮目标：对 Rust 核心 + tp 业务层全量重新审查，正确率/代码质量/功能完善度/bug率/错误率分维度 100% 通过，所有问题用 `tp/repro/` 复现脚本验证修正，并新增任务假死检测与框架崩溃恢复的容灾能力。

## What Changes

### Rust 核心扩展（src/）

#### 新增能力（ADDED）

- **Running 任务 watchdog**（`src/scheduler/`）：新增后台周期扫描任务，每 `XHJOB_WATCHDOG_INTERVAL`（默认 5s）扫描所有 Running 任务，当 `now - started_at > timeout * XHJOB_WATCHDOG_FACTOR`（默认 factor=2，容差防误杀）时自动 `signal_cancel` + 标记 `Interrupted` + 记录 `EventType::HungDetected` 事件。解决子进程 hang 在 IO 但未触发 tokio timeout 的盲区。
- **execution_lease 防重复执行**（`src/store/` + `src/scheduler/`）：Task 新增 `worker_pid: Option<u32>` 字段，dispatch 时写入子进程 PID，`reset_running_to_pending` 前检查 `worker_pid` 对应进程是否仍在运行（`is_process_alive` + starttime 校验），在跑则跳过重置并记录 `EventType::LeaseHeld`。防止 daemon 崩溃后孤儿子进程与新 daemon 重复执行同一任务。
- **PID starttime 校验**（`src/daemon/mod.rs`）：`is_process_alive` 增强为 `is_process_alive_with_starttime(pid, expected_starttime)`，Linux 读 `/proc/<pid>/stat` 第 22 字段 starttime 比对；PID 文件同时写入 starttime。`read_pid` 返回 `(pid, starttime)`，`send_terminate` 校验身份后才发信号。消除 PID 复用导致 SIGKILL 无关进程的 P0 风险。
- **SQLite 启动完整性检查**（`src/store/sqlite.rs`）：`open` 时执行 `PRAGMA quick_check`，失败则返回明确错误（不静默继续）；启用 `PRAGMA foreign_keys=ON` 让 `results.task_id` 外键约束生效。
- **可配置 shutdown drain 超时**（`src/daemon/mod.rs`）：`send_terminate` 的 SIGKILL 等待时间改为 `max(10, XHJOB_SHUTDOWN_DRAIN_SECS + 5)`，保证 daemon 有时间完成 30s drain，避免 in-flight 任务被强杀。

#### 修复（MODIFIED）

- **soft_timeout 对 HTTP 任务的语义**（`src/task/mod.rs` + `src/executor/http.rs`）：当前 TaskBuilder 强制把 HTTP 任务 soft_timeout 重置为 None。本轮明确文档化"HTTP 任务不支持 soft_timeout（协议层无 SIGTERM 等价物）"，并在 `xhjob_inspect` 返回中标注 `soft_timeout_unsupported: true`，避免用户误以为生效。补单测锁定。
- **测试补全**（`src/executor/` + `src/scheduler/` + `src/store/`）：
  - `http.rs` `execute_with_cancel` 的 cancel 路径单测（当前零覆盖）
  - `shell.rs` 纯 hard timeout（无 soft_timeout）触发 SIGKILL 的直接单测
  - `queue.rs` InFlightGuard 在 `tokio::task::JoinHandle::abort()` 下计数器平衡的单测
  - `sqlite.rs` `SqliteStore` 崩溃恢复集成测试（当前只有 InMemoryStore 单测）：写 Running 任务 → drop store → 重新 open → 验证 reset 行为
  - `sqlite.rs` `integrity_check` 失败路径测试
  - `sqlite.rs` 外键约束生效测试（删 task 后 result 级联或拒绝）

### tp 业务层

#### 新增能力（ADDED）

- **`tp/repro/` 复现脚本目录**：每个审查问题一个独立 PHP 复现脚本，命名 `repro_<编号>_<简述>.php`，配套 `runner.php` 批量执行 + 统计 PASS/FAIL。每个脚本结构：`=== 复现 <问题> ===` → 触发问题场景 → 断言修正后行为 → `PASS`/`FAIL`。覆盖：
  - `repro_01_task_hang_watchdog.php`：派发 `sleep 60` + timeout=2 任务，验证 watchdog 在 ~4s（2*timeout）内标记 Interrupted
  - `repro_02_pid_reuse_safety.php`：模拟 daemon 崩溃后 PID 文件残留 + PID 被复用，验证 starttime 校验拒绝误杀
  - `repro_03_crash_no_duplicate_exec.php`：派发 acks_late 任务 + 模拟 daemon 崩溃 + 验证孤儿子进程在跑时新 daemon 不重复执行
  - `repro_04_sqlite_integrity_check.php`：人为损坏 SQLite DB，验证启动报错而非静默继续
  - `repro_05_send_terminate_drain_align.php`：派发 sleep 15 任务 + stop daemon，验证 drain 完成（非 SIGKILL 打断）
  - `repro_06_http_cancel.php`：派发长 HTTP 任务 + cancel，验证 cancel 路径生效
  - `repro_07_hard_timeout_sigkill.php`：派发 sleep 30 + timeout=2（无 soft_timeout），验证 SIGKILL 触发
  - `repro_08_foreign_keys.php`：删 task 后验证 result 外键约束生效
  - `repro_09_api_token_auth.php`：无 token / 错 token 访问 /xhjob/* 返回 401
  - `repro_10_http_controller_integration.php`：通过 ThinkPHP 路由调用 23 个接口，验证控制器→OOP→扩展链路
- **api_token 鉴权中间件**（`tp/app/middleware/XhjobAuth.php`）：读取 `config/xhjob.php` 的 `api_token`，对 `/xhjob/*` 路由校验 `X-Xhjob-Token` header（或 `?token=` query），不匹配返回 401。token 为空时跳过（向后兼容）。
- **HTTP 接口集成测试**（`tp/repro/repro_10_http_controller_integration.php`）：通过 `php think` 或内置 server 启动 ThinkPHP，curl 调用 23 个 `/xhjob/*` 接口，验证响应结构与扩展真实调用链路。

#### 修复（MODIFIED）

- **tp 项目可运行性**：建立 `tp/extend/Xhjob` → `releases/xhjob-thinkphp8-extend/Xhjob` 的符号链接（或复制），让 PSR-0 自动加载能解析 `Xhjob\*` 类；确保 `composer install` 后控制器可加载。
- **任务假死生产模拟**（`tp/repro/repro_01_task_hang_watchdog.php`）：真实模拟卡读 fifo 的假死任务（`bash -c "read x < /tmp/xhjob_test_fifo_$RANDOM"`），验证 watchdog 检测与恢复。
- **框架崩溃生产模拟**（`tp/repro/repro_11_daemon_sigkill_recovery.php`）：用 `kill -9 <daemon_pid>` 模拟 OOM kill，验证 PID 文件清理、SQLite 一致性、任务恢复、无重复执行。

### 验证标准（分维度 100%）

| 维度 | 可验证硬标准 |
|------|-------------|
| **正确率** | `cargo test` 100% 通过（含新增 watchdog/lease/PID/sqlite 单测）+ `tp/repro/runner.php` 全部 PASS + 跨进程生产模拟测试 100% PASS |
| **代码质量** | `cargo clippy --all-targets -- -D warnings` 无 warning + 所有 P0/P1 bug 修复 + 代码审查清单逐项验证 |
| **功能完善度** | watchdog 生效 + 崩溃恢复无重复执行 + 23 HTTP 接口测试覆盖 + api_token 鉴权 + integrity_check 生效 |
| **bug率** | 审查问题清单（P0/P1/P2）逐项复现验证 0 个未修复 |
| **错误率** | 跨进程测试运行期间 0 panic / 0 未捕获 error 日志 |

## Impact

- **Affected specs**: 本 spec 取代 `cross-process-functional-verification`（已完成的 43 PASS 测试作为基线保留，新增容灾测试叠加）
- **Affected code**:
  - Rust: `src/scheduler/queue.rs`（watchdog 集成）、`src/scheduler/cron.rs`（scan_once 联动）、`src/store/mod.rs`（Task 加 worker_pid 字段）、`src/store/sqlite.rs`（integrity_check + foreign_keys + lease check）、`src/store/in_memory.rs`（lease check）、`src/daemon/mod.rs`（PID starttime）、`src/daemon/unix.rs`（PID 文件写 starttime）、`src/daemon_main.rs`（watchdog 启动）、`src/executor/http.rs`（cancel 单测）、`src/executor/shell.rs`（hard timeout 单测）、`src/task/mod.rs`（soft_timeout 文档化）
  - tp: `tp/app/middleware/XhjobAuth.php`（新增）、`tp/route/app.php`（中间件挂载）、`tp/extend/Xhjob`（符号链接）、`tp/repro/`（新增目录 + 11 复现脚本 + runner）、`tp/config/xhjob.php`（无改动，api_token 已存在）
- **BREAKING**: Task 结构新增 `worker_pid` 字段（SQLite schema 加列，自动迁移）；PID 文件格式从纯 PID 变为 `pid\nstarttime`（向后兼容旧格式）

## ADDED Requirements

### Requirement: Running 任务假死检测（watchdog）
系统 SHALL 在 daemon 运行期间周期性扫描所有 Running 状态任务，当任务执行时长超过 `timeout * factor` 时自动取消并标记为 Interrupted，防止子进程 hang 导致任务永久卡死。

#### Scenario: 子进程卡在 IO 未触发 tokio timeout
- **WHEN** 派发 `bash -c "read x < /tmp/nonexistent_fifo"` + timeout=2 + watchdog factor=2 的 shell 任务
- **THEN** ~4s 后任务状态变为 `interrupted`，事件流含 `HungDetected` 事件，子进程被 reap

#### Scenario: 正常长任务不被误杀
- **WHEN** 派发 `sleep 3` + timeout=10 + watchdog factor=2 的任务
- **THEN** 任务在 3s 后正常 success，watchdog 不干预

#### Scenario: watchdog 可关闭
- **WHEN** 设置 `XHJOB_WATCHDOG_INTERVAL=0`
- **THEN** watchdog 不启动，行为回退到修正前

### Requirement: 崩溃后防重复执行（execution_lease）
系统 SHALL 在 daemon 重启恢复时检查 Running 任务的 `worker_pid` 对应进程是否仍存活，若存活则跳过重置（保持 Running 或标记 LeaseHeld），避免孤儿子进程与新 daemon 重复执行同一任务产生重复副作用。

#### Scenario: 孤儿子进程在跑时不重复执行
- **GIVEN** daemon 派发 sleep 30 任务，子进程 PID=1234 正在运行
- **WHEN** daemon 被 SIGKILL 崩溃后重启
- **THEN** reset_running_to_pending 检测到 PID 1234 仍存活，跳过重置该任务，记录 `LeaseHeld` 事件，新 daemon 不重复派发

#### Scenario: 孤儿子进程已退出时正常恢复
- **GIVEN** daemon 派发任务，子进程已自然退出
- **WHEN** daemon 重启
- **THEN** reset_running_to_pending 检测到 worker_pid 不存活，正常重置为 Pending 重新执行

### Requirement: PID 复用防护（starttime 校验）
系统 SHALL 在判断进程存活时除 PID 外额外校验进程 starttime（Linux 读 `/proc/<pid>/stat`），PID 文件同时持久化 starttime，防止 daemon 崩溃后 PID 被无关进程复用导致 `send_terminate` 误杀。

#### Scenario: PID 被复用时不误杀
- **GIVEN** daemon PID=1000 starttime=100000 崩溃，PID 文件残留
- **WHEN** PID 1000 被新进程（starttime=200000）复用，调用 `xhjob_stop`
- **THEN** starttime 校验失败，不发送信号，清理 stale PID 文件，返回"已停止"

### Requirement: SQLite 启动完整性检查
系统 SHALL 在打开 SQLite 数据库时执行 `PRAGMA quick_check`，失败时返回明确错误而非静默继续；SHALL 启用 `PRAGMA foreign_keys=ON` 让外键约束生效。

#### Scenario: 数据库文件损坏
- **WHEN** daemon 启动时 SQLite quick_check 失败
- **THEN** 返回 `XhjobError::store("integrity check failed: ...")`，daemon 不启动，日志记录详情

#### Scenario: 外键约束生效
- **WHEN** 删除一个仍有 result 关联的 task
- **THEN** 因 foreign_keys=ON，行为符合外键约束（级联或拒绝，视 schema 定义）

### Requirement: tp/repro/ 复现脚本框架
系统 SHALL 在 `tp/repro/` 目录提供每个审查问题的独立 PHP 复现脚本，每个脚本复现一个问题场景并断言修正后行为，配套 `runner.php` 批量执行并统计 PASS/FAIL。

#### Scenario: 批量执行复现
- **WHEN** 运行 `php -d extension=libxhjob.so tp/repro/runner.php`
- **THEN** 依次执行所有 `repro_*.php`，输出每个脚本的 PASS/FAIL，汇总 `N PASS / M FAIL`

### Requirement: api_token 鉴权中间件
系统 SHALL 对 `/xhjob/*` 路由校验 `X-Xhjob-Token` header（或 `?token=` query），与 `config/xhjob.php` 的 `api_token` 比对，不匹配返回 401。当 `api_token` 配置为空时跳过校验（向后兼容）。

#### Scenario: 无 token 访问被拒
- **GIVEN** `api_token=secret123`
- **WHEN** 不带 token 访问 `/xhjob/list`
- **THEN** 返回 401 Unauthorized

#### Scenario: 正确 token 放行
- **GIVEN** `api_token=secret123`
- **WHEN** 带 `X-Xhjob-Token: secret123` 访问 `/xhjob/list`
- **THEN** 正常返回任务列表

#### Scenario: api_token 为空时不拦截
- **GIVEN** `api_token` 未配置
- **WHEN** 不带 token 访问 `/xhjob/list`
- **THEN** 正常返回（向后兼容）

## MODIFIED Requirements

### Requirement: send_terminate 超时对齐 drain
`send_terminate` 的 SIGKILL 等待时间 SHALL 为 `max(10, XHJOB_SHUTDOWN_DRAIN_SECS + 5)`，保证 daemon 有时间完成 30s drain，避免 in-flight 任务被强杀导致状态停留在 Running。

#### Scenario: 长任务 drain 不被打断
- **GIVEN** daemon 有 sleep 15 的 in-flight 任务，`XHJOB_SHUTDOWN_DRAIN_SECS=30`
- **WHEN** 调用 `xhjob_stop`
- **THEN** send_terminate 等待最多 35s 才 SIGKILL，daemon 有时间完成 drain，任务正常结束或标记 Interrupted

### Requirement: reset_running_to_pending 检查 worker_pid
`reset_running_to_pending` SHALL 在重置前检查 `worker_pid` 进程是否存活（含 starttime 校验），存活则跳过该任务重置并记录 `LeaseHeld` 事件。

### Requirement: soft_timeout 对 HTTP 任务明确不支持
TaskBuilder SHALL 对 HTTP 任务保留 soft_timeout 字段但标注 `soft_timeout_unsupported: true` 在 inspect 输出，并打印 warning；HTTP executor SHALL 不读取该字段（保持现有行为）。测试 SHALL 锁定此语义。

## REMOVED Requirements

无。本轮全部为新增与加固，不移除现有功能。
