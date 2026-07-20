# 第二轮深度审查与功能完善 Spec

## Why

上一轮 `audit-and-polish-from-user-perspective` 修复了 9 个使用者视角的真实问题（含 1 个 fatal error），并以 43/43 单测 + 7/7 .phpt + 全部 examples 通过收尾。但深入审查代码细节后发现，仓库中仍存在一批上一轮未触及的真实问题，且使用者视角还有可显著提升的功能空白：

1. **Dead code 的"潜在价值"未被启用**：`is_retryable_http_status` / `is_retryable_shell_exit` 已实现但 `should_retry` 中 `let _ = error;` 直接返回 true，导致 HTTP 404 等不可重试错误也会被重试 N 次——这是使用者会踩的真实坑。`Event` / `should_fire_missed` / `thread_pool` / `make_store` 等是未来扩展接口，应加注释保留而非删除。
2. **误导性 API 参数**：`scheduler::cron::next_fire` 接受 `seconds: bool` 但函数体直接 `let _ = seconds;` 忽略它；调用方都传 `false`，给后续维护者造成"是否支持秒级 cron"的疑惑。
3. **cron 表达式非法时静默 warn**：`TaskBuilder::build` 中 cron 解析失败只 `tracing::warn!`，task 仍入队但 `next_fire=None`，永远不被触发。用户得到 task_id 却等不到执行，无法排查。
4. **缺少 cron 执行次数限制**：用户明确希望"定时任务可以指定执行次数，次数执行完毕就停止结束任务"——这是 APScheduler `max_executions` 概念，当前缺失。
5. **文档关键说明缺漏**：`XHJOB_PERSIST` 只在 daemon 启动时读取一次、5/6 段 cron 表达式格式（6 段含秒）未在 README 明确、`examples/cron_http.php` dispatch 后 daemon 仍持续触发 cron 未提示。

本轮聚焦启用真实有价值的功能、引入 cron 执行次数限制、补齐文档与边界测试。允许引入新功能。

## What Changes

### 新功能

- **新增 cron 任务执行次数限制**：`TaskBuilder::max_executions(n)` / PHP `maxExecutions(int $n): $this`
  - `Task` 新增 `max_executions: u32`（0 = 无限，默认）与 `execution_count: u32`（已执行次数，默认 0）
  - cron 触发并执行后 `execution_count += 1`
  - 当 `max_executions > 0 && execution_count >= max_executions` 时，state 置为 `Success` 并停止后续触发
  - SQLite schema 新增两列；旧库自动 `ALTER TABLE` 兼容
  - 顶层函数 `xhjob_dispatch` 透传新字段
- **`should_retry` 按错误类型真实判断**：启用 `is_retryable_http_status` / `is_retryable_shell_exit`
  - HTTP 5xx + 网络错误才重试；4xx 不重试直接 FAILED
  - shell 任何非零 exit 仍重试（保持当前行为）
  - `RetryPolicy::should_retry` 签名调整为接受 `&TaskResult` 而非 `&str`

### 完善

- **cron 表达式非法时 dispatch 立即失败**：`TaskBuilder::build` 中 cron 解析失败返回 `Err`，让 `dispatch()` 立即返回 `error: invalid cron: ...`
- **简化 `next_fire` 签名**：删除被忽略的 `seconds: bool` 参数，调用方与单元测试同步调整
- **保留未来扩展接口并加注释**：`Event` / `should_fire_missed` / `make_store` / `default_db_path` / `thread_pool` 模块均加 `#[allow(dead_code)]` + 文档注释说明保留原因与未来启用路径（如 `Event` 用于未来 IPC 事件流、`thread_pool` 用于未来 CPU 密集任务、`make_store` 用于 store 工厂）
- **删除真正无用的 `sleep_for_retry`**：retry 已通过 `next_fire` 机制实现延迟，此函数无启用路径

### 文档与示例

- README "API 参考" 表新增 `maxExecutions(int $n): $this` 行
- README "环境变量" 表 `XHJOB_PERSIST` 行明确"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
- README "Cron 自定义时区" 一节明确 5/6 段表达式格式（6 段含秒）
- README 新增"Cron 执行次数限制"小节，说明用法与默认值（0 = 无限）
- `examples/cron_http.php` 增加 maxExecutions 示例 + dispatch 后提示 daemon 持续触发需手动 stop

### 测试

- 新增 `tests/boundary_cases.php`：invalid JSON / invalid service name / daemon not running / invalid cron 四条失败路径
- 新增 `tests/max_executions.php`：cron 每 1s 触发 + maxExecutions(3)，验证执行 3 次后 state=SUCCESS 且不再触发
- 既有测试全部不回归

### 编译 + 提交

- 重新编译（两种 feature 零警告）
- 全量测试通过
- 提交并 push 到远程主分支

## Impact

- Affected specs:
  - `audit-and-polish-from-user-perspective`（上一轮产物，本轮在其基础上做深度功能完善，不冲突）
  - `implement-async-task-scheduler`（初始实现，本轮启用其预留接口、新增字段）
- Affected code:
  - `src/task/mod.rs`（TaskBuilder 新增 `max_executions`，build 中 cron 解析失败返回 Err）
  - `src/store/mod.rs`（Task 新增 `max_executions` / `execution_count` 字段；保留 make_store 加注释）
  - `src/store/sqlite.rs`（schema 新增两列 + 旧库 ALTER TABLE 兼容）
  - `src/store/in_memory.rs`（无需 schema 改动，字段自动同步）
  - `src/scheduler/cron.rs`（next_fire 删 seconds 参数；scan_once 中检查 max_executions）
  - `src/scheduler/queue.rs`（任务执行成功后递增 execution_count、检查上限）
  - `src/scheduler/overlap.rs`（保留 should_fire_missed 加注释）
  - `src/retry/mod.rs`（should_retry 真正按错误类型判断；删 sleep_for_retry）
  - `src/ipc/mod.rs`（保留 Event 加注释）
  - `src/pool/mod.rs` + `src/pool/thread_pool.rs`（保留加注释）
  - `src/lib.rs`（Xhjob 类新增 maxExecutions 方法）
  - `src/daemon_main.rs`（dispatch handler 透传新字段）
  - `README.md`（新增 API 行 + 3 处说明 + 1 个新小节）
  - `examples/cron_http.php`（增加 maxExecutions 示例 + 退出提示）
  - `tests/boundary_cases.php`（新增）
  - `tests/max_executions.php`（新增）
- **BREAKING**: 无（新字段默认值 0 = 无限，向后兼容旧 task JSON）

## ADDED Requirements

### Requirement: Cron 执行次数限制
The system SHALL 支持为 cron 任务指定最大执行次数，到达上限后任务自动终止。

#### Scenario: 设置 maxExecutions(3) 后执行 3 次停止
- **WHEN** 用户 `Xhjob::task()->cron('* * * * * *')->maxExecutions(3)->viaShell('echo hi')->dispatch()`
- **THEN** cron 触发 3 次后任务 state 变为 `SUCCESS`，`execution_count=3`
- **AND** 第 4 次 cron tick 不再触发该任务
- **AND** 持久化场景下 restart 后 execution_count 保留

#### Scenario: 不设置 maxExecutions 时无限触发
- **WHEN** 用户 `Xhjob::task()->cron('* * * * *')->viaShell('echo hi')->dispatch()`（未调用 maxExecutions）
- **THEN** `max_executions=0`，cron 持续触发直至 daemon 停止
- **AND** 行为与当前一致（向后兼容）

#### Scenario: maxExecutions(0) 显式表示无限
- **WHEN** 用户 `maxExecutions(0)`
- **THEN** 等同于未设置，cron 持续触发

### Requirement: Retry 按错误类型真实判断
The system SHALL 仅对可重试错误重试，避免对永久性错误（如 HTTP 4xx）无意义重试。

#### Scenario: HTTP 500 重试
- **WHEN** 任务 `withRetry(3, 1)`，HTTP 返回 500
- **THEN** 重试至多 3 次，最终 state=FAILED，attempts>=3

#### Scenario: HTTP 404 不重试
- **WHEN** 任务 `withRetry(3, 1)`，HTTP 返回 404
- **THEN** 不重试，立即 state=FAILED，attempts=1，last_error 包含 "not retryable" 或 "http 404"

#### Scenario: shell 非零 exit 仍重试
- **WHEN** 任务 `withRetry(3, 1)`，shell exit_code=7
- **THEN** 重试至多 3 次（保持当前行为）

### Requirement: 非法 cron 表达式立即失败
The system SHALL 在 dispatch 时立即拒绝非法 cron 表达式，而非静默入队后永不触发。

#### Scenario: 非法 cron dispatch 返回 error
- **WHEN** 用户 `Xhjob::task()->viaShell('echo hi')->cron('not a cron')->dispatch()`
- **THEN** 返回 `error: invalid cron: ...` 字符串
- **AND** 任务不入队（store 中无此 task_id）

### Requirement: Dead code 评估与保留决策
The system SHALL 对未使用代码做出明确决策：启用 / 保留为未来扩展 / 删除。

#### Scenario: 启用 is_retryable_* 函数
- **WHEN** `should_retry` 被调用
- **THEN** 真正调用 `is_retryable_http_status` / `is_retryable_shell_exit` 判断错误类型

#### Scenario: 保留 Event / should_fire_missed / thread_pool / make_store 并加注释
- **WHEN** 开发者阅读这些代码
- **THEN** 看到清晰的 doc comment 说明"保留原因 + 未来启用路径"

#### Scenario: 删除 sleep_for_retry
- **WHEN** 重新编译
- **THEN** `sleep_for_retry` 不再存在，retry 通过 next_fire 机制实现延迟

## MODIFIED Requirements

### Requirement: 测试覆盖度
在原有测试基础上，要求：
- 全量测试在本机 PHP 8.2 环境实际执行（非仅代码审查）
- 边界场景测试与 maxExecutions 测试在本机实际执行并 PASS
- 既有测试（cargo + .phpt + business + examples）不回归

### Requirement: 文档与代码一致性
- README API 参考表与实际 `Xhjob` 类方法一致（含新增 `maxExecutions`）
- README cron 章节明确 5/6 段表达式格式
- README 环境变量表标注 `XHJOB_PERSIST` 读取时机
- README 新增"Cron 执行次数限制"小节

## REMOVED Requirements

### Requirement: 删除未使用 dead code
**Reason**: 用户反馈要求评估而非删除，应启用有价值的（`is_retryable_*`）、保留有未来扩展价值的（`Event` / `should_fire_missed` / `thread_pool` / `make_store`）、仅删除真正无用的（`sleep_for_retry`）
**Migration**: 已在新 spec 中按此原则重新规划
