# 第二轮深度审查与打磨 Spec

## Why

上一轮 `audit-and-polish-from-user-perspective` 修复了 9 个使用者视角的真实问题（含 1 个 fatal error），并以 43/43 单测 + 7/7 .phpt + 全部 examples 通过收尾。但深入审查代码细节后发现，仓库中仍存在一批上一轮未触及的真实问题：

1. **Dead code 累积**：7 个未使用的函数/结构体（`Event` / `should_fire_missed` / `make_store` / `default_db_path` / `is_retryable_http_status` / `is_retryable_shell_exit` / `sleep_for_retry`）和 1 个未被任务派发路径使用的整个 `thread_pool` 模块，增加维护负担、误导读者。
2. **误导性 API 参数**：`scheduler::cron::next_fire` 接受 `seconds: bool` 但函数体直接 `let _ = seconds;` 忽略它；调用方都传 `false`，给后续维护者造成"是否支持秒级 cron"的疑惑。
3. **文档关键说明缺漏**：`XHJOB_PERSIST` 只在 daemon 启动时读取一次（运行中修改不生效）；5 段 vs 6 段 cron 表达式（6 段含秒）未在 README "Cron 自定义时区" 一节明确；`examples/cron_http.php` dispatch 后脚本退出但 daemon 仍在后台跑 cron，未在脚本中提示。
4. **边界测试缺失**：`xhjob_dispatch` 传入非法 JSON、`xhjob_state` / `xhjob_result` 传入非法 service name、daemon 未启动时 dispatch 这三条失败路径未有任何代码验证。

这些不影响已有功能的正确性，但会让二次开发者误判 API 含义、让使用者在边界场景下踩坑。本轮聚焦清理与补齐，不引入新功能。

## What Changes

- 删除确认未被引用的 dead code（7 个函数/结构体 + 1 个 `thread_pool` 模块），减少维护负担
- 简化 `scheduler::cron::next_fire` 签名：删除被忽略的 `seconds: bool` 参数，调用方与单元测试同步调整
- README 补充关键说明：`XHJOB_PERSIST` 仅在 daemon 启动时读取一次、5/6 段 cron 表达式格式（6 段含秒）、daemon 启动后 `XHJOB_*` env 修改不生效
- `examples/cron_http.php` 增加注释说明脚本退出后 daemon 仍持续触发 cron，需手动 `xhjob_stop()`
- 新增边界场景测试脚本：覆盖 invalid JSON dispatch / invalid service name / daemon not running 三条失败路径
- 重新编译（两种 feature 零警告）+ 重跑全量测试（cargo 43+/43+、.phpt、business、examples）确认无回归
- 提交并 push 到远程主分支

- **不引入新功能、不修改 PHP 用户态 API、不回滚任何用户改动**

## Impact

- Affected specs:
  - `audit-and-polish-from-user-perspective`（上一轮产物，本轮在其基础上做深度打磨，不冲突）
  - `implement-async-task-scheduler`（初始实现，本轮清理其遗留 dead code）
- Affected code:
  - `src/ipc/mod.rs`（删 `Event` 结构体）
  - `src/scheduler/overlap.rs`（删 `should_fire_missed` 函数）
  - `src/scheduler/cron.rs`（删 `next_fire` 的 `seconds` 参数及调用方调整）
  - `src/scheduler/mod.rs`（如 `next_fire` 在此被调用，同步调整）
  - `src/store/mod.rs`（删 `make_store` 与 `default_db_path`）
  - `src/retry/mod.rs`（删 `is_retryable_http_status` / `is_retryable_shell_exit` / `sleep_for_retry`）
  - `src/pool/mod.rs`（删 `pub mod thread_pool;`）
  - `src/pool/thread_pool.rs`（整文件删除）
  - `src/task/mod.rs`（调整 `next_fire` 调用方）
  - `src/daemon_main.rs`（调整 `next_fire` 调用方，若有）
  - `README.md`（补充 3 处说明）
  - `examples/cron_http.php`（增加退出提示注释）
  - `tests/boundary_cases.php`（新增）

## ADDED Requirements

### Requirement: Dead code 清理
The system SHALL 移除未被任何调用方引用的 dead code，保持仓库精简。

#### Scenario: 删除未使用函数后编译通过
- **WHEN** 删除 `Event` / `should_fire_missed` / `make_store` / `default_db_path` / `is_retryable_http_status` / `is_retryable_shell_exit` / `sleep_for_retry` 与 `thread_pool` 模块
- **THEN** `cargo build --release` 与 `cargo build --release --features persist` 仍零警告通过
- **AND** `cargo test --release --lib` 与 `cargo test --release --lib --features persist` 仍全部通过

### Requirement: API 签名简化
The system SHALL 移除被忽略的误导性参数，使内部 API 表达真实意图。

#### Scenario: next_fire 不再接受 seconds 参数
- **WHEN** 调用 `scheduler::cron::next_fire(expr, from_ts, timezone)`
- **THEN** 函数根据表达式自身字段数（5 或 6 段）决定是否补 `0 ` 秒前缀，不再依赖外部 `seconds` 提示
- **AND** 旧的 `next_fire(expr, false, from_ts, timezone)` 调用全部更新为新签名

### Requirement: 文档与示例关键说明补齐
The system SHALL 在 README 与 examples 中补充使用者容易踩坑的关键说明。

#### Scenario: README 明确 XHJOB_PERSIST 读取时机
- **WHEN** 用户查阅 README 环境变量表
- **THEN** `XHJOB_PERSIST` 行明确标注"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"

#### Scenario: README 明确 cron 表达式格式
- **WHEN** 用户查阅 README Cron 相关章节
- **THEN** 文档明确"5 段表达式 = `min hour day month weekday`；6 段表达式 = `sec min hour day month weekday`（含秒）"

#### Scenario: cron_http.php 提示 daemon 持续运行
- **WHEN** 用户运行 `examples/cron_http.php`
- **THEN** 脚本输出明确说明脚本退出后 daemon 仍持续触发 cron，需调用 `xhjob_stop()` 才能停止

### Requirement: 边界场景测试覆盖
The system SHALL 通过代码独立验证关键失败路径的返回结果正确性。

#### Scenario: dispatch 非法 JSON
- **WHEN** 调用 `xhjob_dispatch("{not json", "default")`
- **THEN** 返回 `string`，以 `error:` 前缀开头，且包含 `invalid json` 字样

#### Scenario: 非法 service name
- **WHEN** 调用 `xhjob_state("any-id", "1invalid")`（首字符为数字，违反 `^[a-zA-Z]...`）
- **THEN** 返回数组包含 `state=UNKNOWN` 与 `error` 字段，而非 PHP fatal

#### Scenario: daemon 未启动时 dispatch
- **WHEN** daemon 未启动，调用 `xhjob_dispatch('{"task_type":"shell","payload":{"cmd":"echo hi"}}')`
- **THEN** 返回以 `error:` 开头的字符串（连接失败提示），而非 PHP fatal 或卡死

## MODIFIED Requirements

### Requirement: 测试覆盖度
在原有测试基础上，要求：
- 全量测试在本机 PHP 8.2 环境实际执行（非仅代码审查）
- 边界场景测试脚本（`tests/boundary_cases.php`）在本机实际执行并 PASS
- 既有测试（cargo + .phpt + business + examples）不回归

## REMOVED Requirements

（无移除项——本轮为清理与补齐，不删除任何用户可见能力）
