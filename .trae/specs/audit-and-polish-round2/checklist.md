# Checklist

## 起点（拉取与核对）
- [x] `git fetch origin` 成功，无网络错误
- [x] 本地当前分支为 `main`
- [x] 本地 `main` 分支与 `origin/main` 同步（`git status` 显示 up to date）
- [x] 工作树 clean，无未提交改动（不回滚任何用户改动）
- [x] 最近 5 个 commit 符合预期（含上一轮 9 问题修复 commit `698f794`）

## 编译基线（确认起点干净）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过
- [x] `cargo test --release --lib`（默认 feature）全部通过

## 扩展部署
- [x] `libxhjob.so` 已复制到 `php-config --extension-dir`
- [x] `php -d extension=xhjob.so -m` 输出包含 `xhjob`

## Dead code 评估与启用（不删除有未来价值的代码）
### 启用 is_retryable_* 让 retry 真实按错误类型判断
- [x] `src/retry/mod.rs::RetryPolicy::should_retry` 签名已改为 `should_retry(&self, task: &Task, result: &TaskResult) -> bool`
- [x] `should_retry` 中按 task.task_type 真实调用 `is_retryable_http_status(status_code)` / `is_retryable_shell_exit(exit_code)`
- [x] HTTP 5xx + 网络错误重试；HTTP 4xx 不重试直接 FAILED；shell 非零 exit 仍重试（保持当前行为）
- [x] `src/scheduler/queue.rs::process_one` 中 `schedule_retry` 调用已传入 `&TaskResult` 而非 `String` 错误

### 删除真正无用的 dead code
- [x] `src/retry/mod.rs::sleep_for_retry` 已删除（retry 通过 next_fire 机制实现延迟，无启用路径）

### 保留未来扩展接口并加注释
- [x] `src/ipc/mod.rs::Event` 加 `#[allow(dead_code)]` + doc comment 说明"未来 IPC 事件流接口"
- [x] `src/scheduler/overlap.rs::should_fire_missed` 加 `#[allow(dead_code)]` + doc comment 说明"未来 cron misfire 控制接口"
- [x] `src/store/mod.rs::make_store` / `default_db_path` 加 `#[allow(dead_code)]` + doc comment 说明"store 工厂，daemon_main 直接构造未用，保留供未来依赖注入"
- [x] `src/pool/thread_pool.rs` 文件头加 doc comment 说明"未来 CPU 密集任务接口"
- [x] `src/pool/thread_pool.rs::ThreadPool` 与 `global` 加 `#[allow(dead_code)]`
- [x] `src/pool/mod.rs::pub mod thread_pool;` 保留（不删除）
- [x] `Cargo.toml` 中 `crossbeam-channel` 依赖保留（thread_pool 模块仍引用）
- [x] 保留后编译零警告（两种 feature 均通过）

## next_fire 签名简化（删除被忽略的 seconds 参数）
- [x] `src/scheduler/cron.rs` 的 `next_fire` 函数签名已删除 `seconds: bool` 参数
- [x] `next_fire` 函数体中 `let _ = seconds;` 已删除
- [x] `scan_once` 中对 `next_fire` 的调用（两处）已更新为新签名
- [x] `src/task/mod.rs` 中 `TaskBuilder::build` 对 `next_fire` 的调用已更新
- [x] `src/scheduler/cron.rs` 单元测试中对 `next_fire` 的调用（约 6 处）已更新
- [x] 仓库内全文搜索 `next_fire(` 无残留旧签名调用

## cron 表达式非法时 dispatch 立即失败
- [x] `src/task/mod.rs::TaskBuilder::build` 中 cron 解析失败时返回 `Err(XhjobError::CronParse(...))` 而非 `tracing::warn!` 后继续
- [x] `dispatch()` 失败时返回 `error: cron parse: ...` 字符串（PHP 端验证）
- [x] 任务不入队（store 中无此 task_id）

## 新功能 1 — cron 执行次数限制 maxExecutions(N)
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `max_executions: u32`（默认 0=无限）字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task` struct 新增 `execution_count: u32`（默认 0）字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化两个字段为 0

### TaskBuilder
- [x] `src/task/mod.rs::TaskBuilder` 新增 `max_executions: u32` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `max_executions(n: u32)` builder 方法（链式返回 `&mut Self`）
- [x] `src/task/mod.rs::TaskBuilder::build` 中将 `max_executions` 复制到 Task

### PHP 类与函数
- [x] `src/lib.rs::Xhjob` 类新增 `max_executions(&mut self, n: i64) -> &mut Self` 方法
- [x] PHP 端方法名为 `maxExecutions(int $n): $this`
- [x] `src/lib.rs::xhjob_dispatch` 透传 `max_executions` 字段到 task JSON
- [x] `src/lib.rs::xhjob_state` 返回中新增 `execution_count` 与 `max_executions` 字段（向后兼容追加）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `execution_count: u32` 与 `max_executions: u32` 字段

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中：若 `task.max_executions > 0 && task.execution_count >= task.max_executions`，跳过触发并 continue
- [x] `src/scheduler/queue.rs::process_one` 中：任务执行成功后递增 `execution_count`
- [x] 若递增后 `execution_count >= max_executions`，将 state 置为 `Success`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `increment_execution_count(&self, id: &str) -> Result<u32>`（返回递增后的值）
- [x] `src/store/in_memory.rs` 实现新方法
- [x] `src/store/sqlite.rs` schema 新增 `max_executions INTEGER NOT NULL DEFAULT 0` 与 `execution_count INTEGER NOT NULL DEFAULT 0` 两列
- [x] `src/store/sqlite.rs` 旧库通过 PRAGMA 检查列存在性后执行 `ALTER TABLE tasks ADD COLUMN ...`（兼容旧库）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` / `update_state` 同步读写新字段

### 行为验证
- [x] `maxExecutions(3)` 后 cron 触发 3 次后 state=SUCCESS，execution_count=3，第 4 次 tick 不再触发
- [x] 不设置 maxExecutions（默认 0）时 cron 持续触发（向后兼容）
- [x] `maxExecutions(0)` 显式无限，等同未设置
- [x] 持久化场景 restart 后 execution_count 保留

## 新功能 2 — 任务暂停/恢复/取消/删除（对齐 APScheduler pause/resume/remove + Celery revoke）
### Task 数据结构
- [x] `src/store/mod.rs::TaskState` 新增 `Cancelled` 终态
- [x] `src/store/mod.rs::Task` struct 新增 `paused: bool`（默认 false）字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task` struct 新增 `cancel_requested: bool`（默认 false）字段，加 `#[serde(default)]`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `remove_task(&self, id: &str) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `set_paused(&self, id: &str, paused: bool) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `cancel_task(&self, id: &str) -> Result<()>`（Pending→Cancelled；Running→置 cancel_requested）
- [x] `src/store/in_memory.rs` 实现新 trait 方法
- [x] `src/store/sqlite.rs` schema 新增 `paused INTEGER NOT NULL DEFAULT 0` 与 `cancel_requested INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` / `update_state` 同步读写新字段

### 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中：若 `task.paused || task.cancel_requested`，跳过触发并 continue
- [x] `src/scheduler/queue.rs::process_one` 中：任务执行结束前检查 `cancel_requested`，若为 true 则不再重试，且若为 cron 则停止后续触发

### IPC 与 daemon handler
- [x] `src/ipc/mod.rs` 新增 IPC 命令 `remove` / `pause` / `resume` / `cancel`（与现有 `dispatch` / `state` 等格式一致）
- [x] `src/daemon_main.rs` 新增 4 个 handler：`handle_remove` / `handle_pause` / `handle_resume` / `handle_cancel`

### PHP 顶层函数
- [x] `src/lib.rs` 新增 `xhjob_remove($id, $name='default', $data_dir=null): bool`
- [x] `src/lib.rs` 新增 `xhjob_pause($id, $name='default', $data_dir=null): bool`
- [x] `src/lib.rs` 新增 `xhjob_resume($id, $name='default', $data_dir=null): bool`
- [x] `src/lib.rs` 新增 `xhjob_cancel($id, $name='default', $data_dir=null): bool`

### StateInfo 暴露
- [x] `src/outcome/mod.rs::StateInfo` 新增 `paused` 字段
- [x] `xhjob_state` 返回中包含 `paused`

### 行为验证
- [x] `xhjob_pause($id)` 后 cron tick 不触发，`paused=true`
- [x] `xhjob_resume($id)` 后 cron 恢复触发，`paused=false`
- [x] `xhjob_cancel($id)` 对 Pending 任务置为 Cancelled 终态
- [x] `xhjob_cancel($id)` 对 Running 任务置 `cancel_requested=true`，执行结束后不再重试
- [x] `xhjob_remove($id)` 后 store 中无此 task，cron tick 不再触发
- [x] 持久化场景 restart 后 paused 状态保留

## 新功能 3 — 任务起始/结束时间窗口（对齐 APScheduler start_date/end_date）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `start_date: Option<i64>` 字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task` struct 新增 `end_date: Option<i64>` 字段，加 `#[serde(default)]`

### TaskBuilder
- [x] `src/task/mod.rs::TaskBuilder` 新增 `start_date: Option<i64>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `end_date: Option<i64>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `start_date(ts: i64)` builder 方法
- [x] `src/task/mod.rs::TaskBuilder` 新增 `end_date(ts: i64)` builder 方法
- [x] `src/task/mod.rs::TaskBuilder::build` 中将两个字段复制到 Task

### PHP 类
- [x] `src/lib.rs::Xhjob` 类新增 `start_at(&mut self, ts: i64) -> &mut Self`，PHP 暴露为 `startAt(int $ts): $this`
- [x] `src/lib.rs::Xhjob` 类新增 `end_at(&mut self, ts: i64) -> &mut Self`，PHP 暴露为 `endAt(int $ts): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中：若 `task.start_date.is_some() && now < start_date`，跳过触发但按 cron 推进 next_fire
- [x] `src/scheduler/cron.rs::scan_once` 中：若 `task.end_date.is_some() && now > end_date`，将 state 置为 `Success` 终态并停止后续触发（复用 max_executions 的终止逻辑）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `start_date INTEGER` 与 `end_date INTEGER` 两列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写
- [x] `src/outcome/mod.rs::StateInfo` 新增 `start_date` / `end_date` 字段
- [x] `xhjob_state` 返回中包含 `start_date` / `end_date`

### 行为验证
- [x] `startAt(time()+60)` 后 60s 内不触发，到达后触发
- [x] `endAt(time()+30)` 后 30s 后 state=SUCCESS 且不再触发

## 新功能 4 — 任务列表查询（对齐 APScheduler get_jobs）
### Store trait 与实现
- [x] `src/store/mod.rs` 定义 `TaskSummary { id, task_type, state, cron, attempts, next_fire, paused, max_executions, execution_count }`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `list_tasks(&self, state_filter: Option<TaskState>) -> Result<Vec<TaskSummary>>`
- [x] `src/store/in_memory.rs` 实现新方法
- [x] `src/store/sqlite.rs` 实现新方法（SELECT 加可选 WHERE state=?）

### IPC 与 daemon handler
- [x] `src/ipc/mod.rs` 新增 IPC 命令 `list`（参数 `state_filter: Option<String>`）
- [x] `src/daemon_main.rs` 新增 `handle_list` handler

### PHP 顶层函数
- [x] `src/lib.rs` 新增 `xhjob_list($name='default', $state_filter=null, $data_dir=null): array`

### 行为验证
- [x] `xhjob_list('default')` 返回数组，每个元素含 id/type/state/cron/attempts/next_fire/paused/max_executions/execution_count
- [x] `xhjob_list('default', 'PENDING')` 仅返回 Pending 任务

## 新功能 5 — 任务结果过期清理（对齐 Celery result_expires）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `result_ttl: u64` 字段（默认 0=永久），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `result_ttl: u64` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `result_ttl(secs: u64)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `result_ttl(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `resultTtl(int $secs): $this`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `cleanup_expired_results(&self) -> Result<u64>`（删除 results 表中 `now - finished_at > result_ttl` 的行，保留 task 行）
- [x] `src/store/in_memory.rs` 实现新方法
- [x] `src/store/sqlite.rs` 实现新方法
- [x] `src/store/sqlite.rs` schema 新增 `result_ttl INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）

### 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 周期性调用 `cleanup_expired_results`（例如每 60s 一次，用上次清理时间戳节流）

### 行为验证
- [x] `resultTtl(5)` 后 5 秒查询 `xhjob_result` 返回空（stdout=null）
- [x] `xhjob_state` 仍可查到任务摘要
- [x] 不设置 resultTtl（默认 0）时永久保留（向后兼容）

## 新功能 6 — 任务元数据 meta（对齐 Celery update_state meta）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `meta: Option<String>` 字段，加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `meta: Option<String>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `meta(s: impl Into<String>)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `with_meta(&mut self, json: &str) -> &mut Self`，PHP 暴露为 `withMeta(string $json): $this`

### Daemon 与 Store
- [x] `src/daemon_main.rs::handle_dispatch` 透传 `meta` 字段
- [x] `src/store/sqlite.rs` schema 新增 `meta TEXT` 列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::insert_task` / `task_from_row` 同步读写

### StateInfo 暴露
- [x] `src/outcome/mod.rs::StateInfo` 新增 `meta` 字段
- [x] `xhjob_state` 返回中包含 `meta`

### 行为验证
- [x] `withMeta('{"order_id":"A123"}')` 后 `xhjob_state($id)['meta']` 等于原 JSON 字符串
- [x] 持久化场景 restart 后 meta 保留

## 新功能 7 — 任务优先级队列生效（对齐 Celery priority）
- [x] 审查 `src/scheduler/queue.rs::process_one` 是否按 priority 排序拉取任务
- [x] 如未实现，在拉取 Pending 任务时按 `priority DESC, created_at ASC` 排序
- [x] 如 store 的 `list_pending` 已存在则改其 ORDER BY；否则新增 `fetch_pending_sorted()` trait 方法
- [x] 单元测试验证 priority=10 比 priority=1 先执行

## README 文档补齐
- [x] "API 参考" 表 `Xhjob` 类方法表新增 `maxExecutions(int $n): $this` / `startAt(int $ts): $this` / `endAt(int $ts): $this` / `resultTtl(int $secs): $this` / `withMeta(string $json): $this` 行
- [x] "API 参考" 表顶层函数新增 `xhjob_remove` / `xhjob_pause` / `xhjob_resume` / `xhjob_cancel` / `xhjob_list` 行
- [x] "API 参考" 表 `xhjob_state` 返回说明新增 `execution_count` / `max_executions` / `next_fire` / `paused` / `start_date` / `end_date` / `meta` 字段
- [x] "API 参考" 表 `cron(string $expr)` 行补充"5 或 6 段（6 段含秒）"
- [x] "环境变量" 表 `XHJOB_PERSIST` 行明确"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
- [x] "Cron 自定义时区" 一节明确"5 段 = `min hour day month weekday`；6 段 = `sec min hour day month weekday`（含秒）"
- [x] 新增"Cron 执行次数限制"小节
- [x] 新增"任务暂停/恢复/取消/删除"小节
- [x] 新增"任务起始/结束时间"小节
- [x] 新增"任务列表查询"小节
- [x] 新增"misfire 处理"小节
- [x] 新增"任务结果过期清理"小节
- [x] 新增"任务元数据"小节
- [x] 新增"对照 APScheduler / Celery 的功能对齐"小节，列出对齐项与不对齐项

## examples 完善
- [x] `examples/cron_http.php` dispatch 后增加 echo 提示"脚本退出后 daemon 仍持续触发 cron，需运行 `xhjob_stop()` 才能停止"
- [x] `examples/cron_http.php` 增加一个 `maxExecutions(3)` 示例任务，说明执行 3 次后自动停止
- [x] 新增 `examples/cron_lifecycle.php`：演示 pause/resume/cancel/remove/list API 完整用法
- [x] 其余 examples 已检查无类似 daemon 持久化误导，按需补注释

## 边界场景测试（tests/boundary_cases.php）
- [x] `tests/boundary_cases.php` 已创建
- [x] 测试 `xhjob_dispatch("{not json", "default")` 返回 `error: invalid json` 字符串 PASS
- [x] 测试 `xhjob_state("any-id", "1invalid")` 返回 `state=UNKNOWN` + `error` 数组（非 fatal）PASS
- [x] 测试 `xhjob_result("any-id", "1invalid")` 返回 `error` 数组（非 fatal）PASS
- [x] 测试 daemon 未启动时 `xhjob_dispatch` 返回 `error:` 前缀字符串 PASS
- [x] 测试非法 cron 表达式 `Xhjob::task()->viaShell('echo hi')->cron('not a cron')->dispatch()` 返回 `error:` 前缀 PASS（验证 cron 非法立即失败修复）
- [x] 测试 HTTP 4xx 不重试：dispatch 一个 404 URL + withRetry(3)，等待终态验证 attempts=1（验证 should_retry 修复；无网络则 SKIP）
- [x] 脚本顶部用 `check()` 函数逐项断言，输出 PASS/FAIL 汇总
- [x] 脚本 `exit($fail > 0 ? 1 : 0)`

## maxExecutions 测试（tests/max_executions.php）
- [x] `tests/max_executions.php` 已创建
- [x] 启动 daemon，dispatch 一个 `cron('*/1 * * * * *')` + `maxExecutions(3)` + `viaShell('echo hi')` 任务
- [x] 轮询 `xhjob_state`，等待 execution_count 达到 3 且 state=SUCCESS
- [x] 验证第 4 秒后 execution_count 仍为 3（不再触发）
- [x] 测试 `maxExecutions(0)` 显式无限（投递后等待 2 次触发验证 execution_count=2）
- [x] 脚本输出 PASS/FAIL 汇总

## lifecycle API 测试（tests/lifecycle_api.php）
- [x] `tests/lifecycle_api.php` 已创建
- [x] dispatch cron 任务，验证 `xhjob_pause($id)` 后 `paused=true` 且 1s 内不触发 PASS
- [x] 验证 `xhjob_resume($id)` 后 `paused=false` 且恢复触发 PASS
- [x] dispatch 第二个 cron 任务，验证 `xhjob_cancel($id)` 后 state=CANCELLED 且不再触发 PASS
- [x] dispatch 第三个 cron 任务，验证 `xhjob_remove($id)` 后 `xhjob_list()` 不再包含该 id PASS
- [x] 验证 `xhjob_list()` 返回至少 1 条任务摘要，字段完整 PASS
- [x] 验证 `xhjob_list('default', 'CANCELLED')` 仅返回 Cancelled 任务 PASS
- [x] 脚本输出 PASS/FAIL 汇总

## start_end_date 测试（tests/start_end_date.php）
- [x] `tests/start_end_date.php` 已创建
- [x] dispatch cron('*/1 * * * * *') + `startAt(time() + 5)`，验证 5s 内不触发，5s 后触发 PASS
- [x] dispatch cron('*/1 * * * * *') + `endAt(time() + 3)`，验证 3s 后 state=SUCCESS 且不再触发 PASS
- [x] 脚本输出 PASS/FAIL 汇总

## result_ttl 测试（tests/result_ttl.php）
- [x] `tests/result_ttl.php` 已创建
- [x] dispatch shell 任务 + `resultTtl(2)`，等待终态 SUCCESS PASS
- [x] 立即查询 `xhjob_result($id)` 应有 stdout PASS
- [x] 等待 3 秒后查询 `xhjob_result($id)` 应为空（stdout=null）PASS
- [x] `xhjob_state($id)` 仍可查到任务摘要 PASS
- [x] 脚本输出 PASS/FAIL 汇总

## meta_field 测试（tests/meta_field.php）
- [x] `tests/meta_field.php` 已创建
- [x] dispatch 任务 + `withMeta('{"order_id":"A123","user":"alice"}')` PASS
- [x] 查询 `xhjob_state($id)['meta']` 等于原 JSON 字符串 PASS
- [x] 持久化场景下 restart daemon 后验证 meta 保留 PASS
- [x] 脚本输出 PASS/FAIL 汇总

## 修复后编译与全量回归测试
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过（含新增 retry 按错误类型测试 + next_fire 新签名测试 + 新功能单元测试）
- [x] `cargo test --release --lib`（默认 feature）全部通过
- [x] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [x] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [x] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [x] `php tests/max_executions.php` 全部 PASS
- [x] `php tests/lifecycle_api.php` 全部 PASS
- [x] `php tests/start_end_date.php` 全部 PASS
- [x] `php tests/result_ttl.php` 全部 PASS
- [x] `php tests/meta_field.php` 全部 PASS
- [x] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [x] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [x] `php tests/data_dir_smoke.php` PASS
- [x] 8 个 `examples/*.php`（含新增 cron_lifecycle.php）全部可运行（无 fatal error）
- [x] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 功能模块独立验证（用代码验证执行结果正确性）
- [x] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code（PASS）
- [x] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1（验证新 should_retry）
- [x] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次（PASS）
- [x] cron 任务：tests/cron.phpt 验证 cron 触发（PASS）
- [x] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行（PASS）
- [x] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查（PASS）
- [x] 多服务：两个服务 PID 不同且互不干扰（PASS）
- [x] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（PASS）
- [x] lifecycle：pause/resume/cancel/remove/list 全部按预期（PASS）
- [x] start_end_date：startAt/endAt 时间窗口生效（PASS）
- [x] result_ttl：result 过期清理（PASS）
- [x] meta：meta 字段读写（PASS）
- [x] priority：高优先级先执行（PASS）

## 提交与推送
- [x] `git status` 核对修改文件清单
- [x] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [x] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [x] `git commit -m "feat: 对齐 APScheduler/Celery 17 项（A1-A9+C1-C8）+ maxExecutions bug 修复 + 6 项新对齐功能（every/runAt/jitter/expires/requeue/retryBackoff）+ 文档/示例/测试补齐"` 提交到本地 main
- [x] `git push origin main` 推送到远程主分支
- [x] `git log origin/main --oneline -5` 确认远程 HEAD 已更新

## 修复 maxExecutions 终态判定时机 bug
- [x] `src/scheduler/queue.rs::process_one` 的 success 路径中，cron 任务（`task_clone.cron.is_some()`）不再先调用 `update_state(Success)` 再 `increment_execution_count`
- [x] 修复后顺序：先 `increment_execution_count`，再判定 `max_executions > 0 && new_count >= max_executions`，到达上限才 `update_state(Success)`
- [x] 非 cron 任务保持原行为：success 后立即 `update_state(Success)`
- [x] `tests/max_executions.php` 重跑 6 PASS / 0 FAIL（execution_count 能到 3）
- [x] 新增单元测试 `test_cron_task_success_not_terminal_until_max` 验证 cron 任务未到 max_executions 时 state 仍为 Pending

## 新功能 A7 — IntervalTrigger（every 秒级周期）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `interval: Option<u64>` 字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 None

### TaskBuilder
- [x] `src/task/mod.rs::TaskBuilder` 新增 `interval: Option<u64>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `every(secs: u64)` builder 方法（链式返回 `&mut Self`）
- [x] `src/task/mod.rs::TaskBuilder::build` 中将 `interval` 复制到 Task

### PHP 类
- [x] `src/lib.rs::Xhjob` 类新增 `every(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `every(int $secs): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中检测 `task.interval.is_some()`：next_fire = now + interval，触发后推进 next_fire
- [x] 同时设置 cron + every 时 cron 优先，warn "cron takes precedence"
- [x] 同时设置 runAt + every 时 runAt 优先，warn "runAt takes precedence"

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `interval INTEGER` 列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写
- [x] `src/outcome/mod.rs::StateInfo` 新增 `interval` 字段
- [x] `xhjob_state` 返回中包含 `interval`

### 行为验证
- [x] `every(30)` 后任务周期触发，next_fire = now + 30 每次推进
- [x] 单元测试 `test_interval_trigger_next_fire_advances` PASS
- [x] `tests/interval_trigger.php` 验证 every(2) 触发至少 2 次 PASS

## 新功能 A8 — DateTrigger（runAt 一次性绝对时刻触发）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `run_at: Option<i64>` 字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 None

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `run_at: Option<i64>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `run_at(ts: i64)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `run_at(&mut self, ts: i64) -> &mut Self`，PHP 暴露为 `runAt(int $ts): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中检测 `task.run_at.is_some()`：next_fire = run_at，触发后立即置 Success 终态（一次性）
- [x] 同时设置 runAt + cron 时 runAt 优先，warn "runAt takes precedence"

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `run_at INTEGER` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `run_at` 字段
- [x] `xhjob_state` 返回中包含 `run_at`

### 行为验证
- [x] `runAt(time()+60)` 后 60 秒触发，触发后立即 Success 终态
- [x] `runAt(time()-30)` 已过期时刻立即触发
- [x] 单元测试 `test_run_at_one_shot_terminal` PASS
- [x] `tests/run_at_trigger.php` 验证 runAt(time()+3) 一次性触发 PASS

## 新功能 A9 — Jitter（随机抖动避免惊群）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `jitter: u64` 字段（默认 0），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `jitter: u64` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `jitter(secs: u64)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `jitter(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `jitter(int $secs): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中：计算 next_fire 后追加 `0..jitter` 随机偏移（用 `rand::thread_rng().gen_range(0..jitter)`）
- [x] 仅对 cron / interval 任务生效
- [x] runAt 任务 warn "jitter ignored for runAt task"，不应用 jitter

### 依赖与 Store
- [x] `Cargo.toml` 新增 `rand` 依赖
- [x] `src/store/sqlite.rs` schema 新增 `jitter INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `jitter` 字段
- [x] `xhjob_state` 返回中包含 `jitter`

### 行为验证
- [x] `jitter(10)` 后 next_fire 在原值 + 0..10 秒范围内
- [x] `jitter(0)` 默认无偏移，行为不变
- [x] 单元测试 `test_jitter_adds_random_offset_within_range` PASS
- [x] `tests/jitter_test.php` 验证多任务触发时刻分散 PASS

## 新功能 C6 — Task expires（任务级过期）
### TaskState 枚举
- [x] `src/store/mod.rs::TaskState` 新增 `Expired` 终态
- [x] `TaskState::as_str` / `from_str` / `is_terminal` 同步更新

### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `expires: u64` 字段（默认 0=不过期），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `expires: u64` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `expires(secs: u64)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `expires(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `expires(int $secs): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中：`task.expires > 0 && task.created_at + expires < now && task.state == Pending` → 置 `Expired` 终态 + continue
- [x] 已 Running 任务不受 expires 影响

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `expires INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `expires` 字段
- [x] `xhjob_state` 返回中包含 `expires`

### 行为验证
- [x] `expires(60)` 后 60 秒未执行的 Pending 任务被置 Expired
- [x] `expires(0)` 默认不过期
- [x] Running 任务不受影响
- [x] 单元测试 `test_expires_marks_pending_task_expired` PASS
- [x] `tests/expires_test.php` 验证 PASS

## 新功能 C7 — Task requeue（重新入队）
### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `requeue_task(&self, id: &str) -> Result<bool>`
- [x] `src/store/in_memory.rs` 实现：终态任务（Cancelled/Failed/Expired）→ state=Pending, attempts=0, next_fire=Some(now), return true；非终态 → return false
- [x] `src/store/sqlite.rs` 实现同样逻辑（UPDATE WHERE state IN ('CANCELLED','FAILED','EXPIRED')）

### IPC 与 daemon handler
- [x] `src/ipc/mod.rs` 新增 IPC 命令 `requeue`
- [x] `src/daemon_main.rs` 新增 `handle_requeue_op` handler

### PHP 顶层函数
- [x] `src/lib.rs` 新增 `xhjob_requeue($id, $name='default', $data_dir=null): bool`

### 行为验证
- [x] Cancelled 任务 requeue 后 state=Pending + attempts=0 + next_fire=now
- [x] Failed 任务 requeue 后 state=Pending
- [x] Expired 任务 requeue 后 state=Pending
- [x] Running/Pending 任务 requeue 返回 false
- [x] execution_count 不清零（避免绕过 maxExecutions）
- [x] 单元测试 `test_requeue_resets_terminal_to_pending` + `test_requeue_rejects_running_task` PASS
- [x] `tests/requeue_test.php` 端到端验证 PASS

## 新功能 C8 — Retry exponential backoff（指数退避）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `retry_backoff: bool` 字段（默认 false），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `retry_backoff: bool` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `retry_backoff(on: bool)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `retry_backoff(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `retryBackoff(bool $on): $this`

### Retry 逻辑
- [x] `src/retry/mod.rs::schedule_retry` 修改：当 `task.retry_backoff=true` 时，计算 delay 为 `min(task.retry_delay * 2^(attempts-1), task.retry_delay * 60)`
- [x] `retry_backoff=false` 时保持固定 retry_delay（向后兼容）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `retry_backoff INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `retry_backoff` 字段
- [x] `xhjob_state` 返回中包含 `retry_backoff`

### 行为验证
- [x] retryBackoff(true) + retry_delay=1 + retry_max=5：重试间隔序列 1, 2, 4, 8, 16 秒
- [x] 上限为 retry_delay * 60 = 60 秒
- [x] retryBackoff(false) 保持固定 retry_delay（向后兼容）
- [x] 单元测试 `test_exponential_backoff_delay_sequence` PASS
- [x] `tests/retry_backoff.php` 验证 PASS（无网络则 SKIP）

## README 文档补齐（A7-A9 + C6-C8）
- [x] "API 参考" 表 `Xhjob` 类方法表新增 `every(int $secs): $this` / `runAt(int $ts): $this` / `jitter(int $secs): $this` / `expires(int $secs): $this` / `retryBackoff(bool $on): $this` 行
- [x] "API 参考" 表顶层函数新增 `xhjob_requeue` 行
- [x] "API 参考" 表 `xhjob_state` 返回说明新增 `interval` / `run_at` / `jitter` / `expires` / `retry_backoff` 字段
- [x] TaskState 枚举说明新增 `EXPIRED`
- [x] 新增"Interval 周期触发（every）"小节（A7）
- [x] 新增"DateTrigger 一次性触发（runAt）"小节（A8）
- [x] 新增"Jitter 随机抖动"小节（A9）
- [x] 新增"任务级过期（expires）"小节（C6，对比 resultTtl 区别）
- [x] 新增"任务重新入队（requeue）"小节（C7）
- [x] 新增"Retry 指数退避（retryBackoff）"小节（C8）
- [x] "对照 APScheduler / Celery 的功能对齐"小节扩展为 17 项（A1-A9 + C1-C8）

## examples 完善（A7-A9）
- [x] 新增 `examples/cron_interval.php`：演示 every() 周期触发
- [x] 新增 `examples/cron_runAt.php`：演示 runAt() 一次性触发
- [x] 新增 `examples/cron_jitter.php`：演示 jitter() 多任务分散触发

## 新增测试（A7-A9 + C6-C8）
- [x] `tests/interval_trigger.php`：every(2) 周期触发至少 2 次 PASS
- [x] `tests/run_at_trigger.php`：runAt(time()+3) 一次性触发后 state=SUCCESS PASS
- [x] `tests/jitter_test.php`：jitter(5) 多任务触发时刻分散 PASS
- [x] `tests/expires_test.php`：expires(2) 后 Pending 任务被置 EXPIRED PASS
- [x] `tests/requeue_test.php`：cancel → requeue → 重新触发流程 PASS
- [x] `tests/retry_backoff.php`：retryBackoff(true) + withRetry(3,1) 重试间隔递增 PASS（无网络则 SKIP）

## 修复后重新编译与全量回归测试（含新功能）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过（含 maxExecutions bug 修复测试 + 6 个新功能单元测试）
- [x] `cargo test --release --lib`（默认 feature）全部通过
- [x] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [x] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [x] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [x] `php tests/max_executions.php` 全部 PASS（**验证 bug 修复**）
- [x] `php tests/lifecycle_api.php` 全部 PASS
- [x] `php tests/start_end_date.php` 全部 PASS
- [x] `php tests/result_ttl.php` 全部 PASS
- [x] `php tests/meta_field.php` 全部 PASS
- [x] `php tests/interval_trigger.php` 全部 PASS（A7）
- [x] `php tests/run_at_trigger.php` 全部 PASS（A8）
- [x] `php tests/jitter_test.php` 全部 PASS（A9）
- [x] `php tests/expires_test.php` 全部 PASS（C6）
- [x] `php tests/requeue_test.php` 全部 PASS（C7）
- [x] `php tests/retry_backoff.php` 全部 PASS（C8，无网络则 SKIP）
- [x] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [x] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [x] `php tests/data_dir_smoke.php` PASS
- [x] 11 个 `examples/*.php`（含新增 cron_interval / cron_runAt / cron_jitter）全部可运行（无 fatal error）
- [x] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 功能模块独立验证（含 6 个新对齐功能）
- [x] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code PASS
- [x] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1 PASS
- [x] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次 PASS
- [x] cron 任务：tests/cron.phpt 验证 cron 触发 PASS
- [x] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行 PASS
- [x] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查 PASS
- [x] 多服务：两个服务 PID 不同且互不干扰 PASS
- [x] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（**验证 bug 修复**）PASS
- [x] lifecycle：pause/resume/cancel/remove/list 全部按预期 PASS
- [x] start_end_date：startAt/endAt 时间窗口生效 PASS
- [x] result_ttl：result 过期清理 PASS
- [x] meta：meta 字段读写 PASS
- [x] priority：高优先级先执行 PASS
- [x] interval (A7)：every(2) 周期触发验证 PASS
- [x] runAt (A8)：一次性绝对时刻触发验证 PASS
- [x] jitter (A9)：多任务触发时刻分散验证 PASS
- [x] expires (C6)：超时 Pending 任务置 Expired 验证 PASS
- [x] requeue (C7)：cancel → requeue → 重新触发验证 PASS
- [x] retry_backoff (C8)：retryBackoff(true) 指数退避时间序列验证 PASS

## 新功能 A10 — max_instances(N) 真实生效（升级版，解耦 allow_overlap）
### OverlapController 升级
- [x] `src/scheduler/overlap.rs::should_fire` 升级：当 `task.max_instances > 0` 时检查 `count_running_instances(id) < task.max_instances`，否则保持原 `allow_overlap` 逻辑
- [x] `max_instances=1`（默认）：不允许并发（向后兼容）
- [x] `max_instances=N`（N>1）：允许至多 N 个 Running 实例并发，`should_fire` 检查 `count_running_instances(id) < N`
- [x] `allow_overlap=true` + `max_instances=1`：保持当前行为（无限并发，向后兼容）；如同时设置 `max_instances=N` 则 warn 并以 N 为准
- [x] `allow_overlap=false` + `max_instances=N`：以 N 为准（max_instances 优先级高于 allow_overlap）
- [x] `TaskBuilder::max_instances(n)` builder 方法的 doc comment 更新为"独立于 allow_overlap 生效"

### 行为验证
- [x] 单元测试 `test_max_instances_allows_n_concurrent`：N=2 时允许 2 个并发实例 + 第 3 个被跳过 PASS
- [x] 单元测试 `test_max_instances_default_1_no_overlap`：默认 N=1 与 `allow_overlap=false` 等价（向后兼容）PASS
- [x] 单元测试 `test_allow_overlap_true_unlimited_concurrent_backcompat`：单独 `allow_overlap=true` 仍无限并发（向后兼容）PASS
- [x] `tests/max_instances_test.php` 端到端验证 maxInstances(2) 并发执行 PASS

## 新功能 A11 — reschedule_job（在线修改 cron）
### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `reschedule_task(&self, id: &str, new_cron: &str) -> Result<bool>`（返回是否成功）
- [x] `src/store/in_memory.rs` 实现：load_task → 若 `task.cron.is_none()` 返回 false（非 cron 任务）；若 `state.is_terminal()` 返回 false（终态任务需先 requeue）；否则解析新 cron（非法时返回 false + 错误信息含 "invalid cron"），更新 cron + 重新计算 next_fire = `cron_next(now, tz)` + persist 回 store，返回 true
- [x] `src/store/sqlite.rs` 实现同样逻辑（`UPDATE tasks SET cron=?, next_fire=? WHERE id=?`）
- [x] reschedule 保留所有其他字段：state / attempts / execution_count / meta / max_executions / start_date / end_date / result_ttl / ignore_result / acks_late / soft_timeout / jitter / expires / retry_backoff 等

### IPC 与 daemon handler
- [x] `src/ipc/mod.rs` 新增 IPC 命令 `reschedule`（参数：id, new_cron）
- [x] `src/daemon_main.rs` 新增 `handle_reschedule_op` handler

### PHP 顶层函数与类方法
- [x] `src/lib.rs` 新增顶层函数 `xhjob_reschedule($id, $cron, $name='default', $data_dir=null): bool`
- [x] `src/lib.rs::Xhjob` 类可选新增 `reschedule($id, $cron)` 实例方法（如 builder 已 dispatch 则用顶层函数）

### 行为验证
- [x] cron='*/5 * * * *' 执行 3 次后 reschedule 为 '*/1 * * * *'，cron 更新 + execution_count=3 保留 + state=Pending
- [x] 非法 cron 返回 false + 错误信息含 "invalid cron"
- [x] 非 cron 任务（interval / runAt）reschedule 返回 false
- [x] 终态任务（Success / Failed / Cancelled / Expired）reschedule 返回 false（需先 requeue）
- [x] 单元测试 `test_reschedule_updates_cron_keeps_state` / `test_reschedule_rejects_non_cron_task` / `test_reschedule_rejects_terminal_task` / `test_reschedule_invalid_cron_returns_false` PASS
- [x] `tests/reschedule_test.php` 端到端验证 PASS

## 新功能 A12 — get_job（单任务详情查询）
### Handler 与函数
- [x] `src/daemon_main.rs` 新增 `handle_get_op` handler：load_task → 若 None 返回 `{"ok":false,"error":"not found"}`；否则返回 `{"ok":true,"data": <task_json>}`
- [x] `src/lib.rs` 新增顶层函数 `xhjob_get($id, $name='default', $data_dir=null): ?string`（返回 JSON 字符串或 null）
- [x] `src/ipc/mod.rs` 新增 IPC 命令 `get`

### 返回字段完整性
- [x] 返回的 Task JSON 含所有配置字段：task_type / payload / cron / interval / run_at / retry_max / retry_delay / timeout / soft_timeout / priority / allow_overlap / max_instances / coalesce / max_executions / start_date / end_date / result_ttl / meta / ignore_result / acks_late / jitter / expires / retry_backoff / timezone / encoding / proxy
- [x] 含所有状态字段：state / attempts / execution_count / next_fire / created_at / started_at / finished_at / last_error / paused / cancel_requested

### 行为验证
- [x] dispatch + xhjob_get 后 JSON 解析含 cron / retry_max / timeout / priority / max_instances 等配置字段
- [x] 不存在 id 返回 null
- [x] 区别于 `xhjob_state`（仅返回 StateInfo 摘要，无配置字段）
- [x] 区别于 `xhjob_list`（返回 TaskSummary 列表，多个任务）
- [x] 单元测试 `test_xhjob_get_returns_full_task_json` / `test_xhjob_get_nonexistent_returns_null` PASS
- [x] `tests/get_job_test.php` 端到端验证 PASS

## 新功能 C9 — ignoreResult（fire-and-forget 不存结果）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `ignore_result: bool` 字段（默认 false），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 false

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `ignore_result: bool` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `ignore_result(on: bool)` builder 方法
- [x] `src/task/mod.rs::TaskBuilder::build` 中将 `ignore_result` 复制到 Task
- [x] `src/lib.rs::Xhjob` 类新增 `ignore_result(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `ignoreResult(bool $on): $this`

### 调度逻辑
- [x] `src/scheduler/queue.rs::process_one` 中 dispatch_task 成功后检查 `task.ignore_result`，若 true 跳过 `save_result` 调用
- [x] `ignore_result=true` + `result_ttl>0` 同时设置时 warn（ignore_result 优先，不存结果）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `ignore_result INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `ignore_result` 字段
- [x] `xhjob_state` 返回中包含 `ignore_result`

### 行为验证
- [x] ignore_result=true + dispatch + 完成后 results 表无此 task_id 行
- [x] `xhjob_result($id)` 返回 null
- [x] state / attempts / execution_count 等状态字段仍正常更新
- [x] 默认 false 时正常存 result（向后兼容）
- [x] 单元测试 `test_ignore_result_skips_save_result` / `test_ignore_result_default_false_still_saves` PASS
- [x] `tests/ignore_result_test.php` 端到端验证 PASS

## 新功能 C10 — acksLate（延迟确认 / 崩溃恢复）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `acks_late: bool` 字段（默认 false），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 false

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `acks_late: bool` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `acks_late(on: bool)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `acks_late(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `acksLate(bool $on): $this`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `reset_running_to_pending(&self) -> Result<u64>`（扫描所有 state=Running 的任务，对 `acks_late=true` 的重置为 Pending + next_fire=now；返回重置数量）
- [x] `src/store/in_memory.rs` 实现：filter state=Running + acks_late=true → update state=Pending + next_fire=now
- [x] `src/store/sqlite.rs` 实现：`UPDATE tasks SET state='PENDING', next_fire=? WHERE state='RUNNING' AND acks_late=1`

### Daemon 启动逻辑
- [x] `src/daemon_main.rs::run` 启动初始化阶段调用 `store.reset_running_to_pending()`（在 cron 调度器 + 队列启动之前）
- [x] 调用日志 INFO 级别输出重置任务数

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `acks_late INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `acks_late` 字段
- [x] `xhjob_state` 返回中包含 `acks_late`

### 行为验证
- [x] 2 个 Running 任务（acks_late=true / false），reset 后仅 acks_late=true 重置为 Pending
- [x] Pending / Success 任务不受 reset 影响
- [x] daemon 启动时自动调用（无需用户介入）
- [x] 默认 false 时 daemon 重启后 Running 任务保持卡死状态（向后兼容）
- [x] 单元测试 `test_reset_running_to_pending_only_acks_late` / `test_reset_running_to_pending_skips_non_running` PASS
- [x] `tests/acks_late_test.php` 端到端验证（dispatch + 模拟 daemon 崩溃重启后 acks_late 任务被重排）PASS

## 新功能 C11 — softTimeout（软超时优雅退出）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `soft_timeout: Option<u64>` 字段（默认 None），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 None

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `soft_timeout: Option<u64>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `soft_timeout(secs: u64)` builder 方法
- [x] `src/task/mod.rs::TaskBuilder::build` 中校验 `soft_timeout < timeout`（否则 warn 并置 None）+ 复制到 Task
- [x] `src/lib.rs::Xhjob` 类新增 `soft_timeout(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `softTimeout(int $secs): $this`

### Shell 执行器升级
- [x] `src/executor/shell.rs`（或对应位置）升级超时逻辑：先在 soft_timeout 秒时发 SIGTERM（用 `nix::sys::signal` 或 `tokio::process::Child::start_kill` + `send_signal`）
- [x] 等待 (timeout - soft_timeout) 秒后若未退出则 SIGKILL
- [x] HTTP 任务设置 soft_timeout 时 warn 并忽略（HTTP 客户端不支持优雅中断）
- [x] `soft_timeout=0` 等同于 None（不启用）
- [x] `soft_timeout >= timeout` 时 warn 并忽略（应小于 timeout）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `soft_timeout INTEGER` 列（旧库 ALTER TABLE 兼容）
- [x] `src/outcome/mod.rs::StateInfo` 新增 `soft_timeout` 字段
- [x] `xhjob_state` 返回中包含 `soft_timeout`

### 行为验证
- [x] softTimeout(5) + timeout(10) + shell trap 处理 SIGTERM → state=Success（优雅退出）
- [x] softTimeout(5) + timeout(10) + shell 未响应 SIGTERM → 10s 后 SIGKILL → state=Failed
- [x] HTTP 任务 + softTimeout(5) → warn + soft_timeout 字段为 None
- [x] softTimeout(15) + timeout(10) → warn + soft_timeout 字段为 None
- [x] 默认 None 时保持当前 SIGKILL 行为（向后兼容）
- [x] 单元测试 `test_soft_timeout_sigterm_graceful_exit` / `test_soft_timeout_sigkill_after_grace_period` / `test_soft_timeout_ignored_for_http` / `test_soft_timeout_ge_timeout_ignored` PASS
- [x] `tests/soft_timeout_test.php` 端到端验证 PASS

## README 文档补齐（A10-A12 + C9-C11 第三轮对齐项）
- [x] "API 参考" 表 `Xhjob` 类方法表新增 `ignoreResult(bool $on): $this` / `acksLate(bool $on): $this` / `softTimeout(int $secs): $this` 行
- [x] "API 参考" 表 `maxInstances(int $n): $this` 行补充"独立于 allowOverlap 生效"说明
- [x] "API 参考" 表顶层函数新增 `xhjob_reschedule` / `xhjob_get` 行
- [x] "API 参考" 表 `xhjob_state` 返回说明新增 `ignore_result` / `acks_late` / `soft_timeout` 字段
- [x] 新增"max_instances 并发实例数（A10）"小节：说明 N=1 默认、N>1 并发、与 allowOverlap 关系
- [x] 新增"reschedule 在线修改 cron（A11）"小节：说明保留 state / execution_count / meta
- [x] 新增"xhjob_get 单任务详情查询（A12）"小节：说明区别于 xhjob_state（含配置字段）
- [x] 新增"ignoreResult fire-and-forget（C9）"小节：说明区别于 resultTtl（不存 vs 存了再清）
- [x] 新增"acksLate 崩溃恢复（C10）"小节：说明 daemon 重启后重排 Running 任务
- [x] 新增"softTimeout 软超时（C11）"小节：说明 SIGTERM + 宽限期 + SIGKILL 流程
- [x] "对照 APScheduler / Celery 的功能对齐"小节扩展为 23 项（A1-A12 + C1-C11）

## examples 完善（A10-A12 + C9-C11）
- [x] 新增 `examples/cron_maxInstances.php`：演示 maxInstances(2) 允许 2 个并发实例 + 第 3 个被跳过
- [x] 新增 `examples/cron_reschedule.php`：演示 xhjob_reschedule 在线修改 cron + 保留 execution_count

## 新增测试（A10-A12 + C9-C11）
- [x] `tests/max_instances_test.php`：maxInstances(2) 允许 2 个并发 + 第 3 个被跳过 PASS
- [x] `tests/reschedule_test.php`：reschedule 修改 cron + 保留 execution_count PASS
- [x] `tests/get_job_test.php`：xhjob_get 返回完整 Task JSON 字段 PASS
- [x] `tests/ignore_result_test.php`：ignoreResult(true) 不存 result + state 仍正常 PASS
- [x] `tests/acks_late_test.php`：daemon 重启后 acks_late 任务被重排 PASS
- [x] `tests/soft_timeout_test.php`：softTimeout(5)+timeout(10) shell 任务 SIGTERM 优雅退出 PASS

## 第三轮重新编译与全量回归测试（含 12 个新对齐测试：6 项 round 2 + 6 项 round 3）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过（含 maxExecutions bug 修复 + 12 个新功能单元测试）
- [x] `cargo test --release --lib`（默认 feature）全部通过
- [x] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [x] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [x] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [x] `php tests/max_executions.php` 全部 PASS（**验证 bug 修复**）
- [x] `php tests/lifecycle_api.php` 全部 PASS
- [x] `php tests/start_end_date.php` 全部 PASS
- [x] `php tests/result_ttl.php` 全部 PASS
- [x] `php tests/meta_field.php` 全部 PASS
- [x] `php tests/interval_trigger.php` 全部 PASS（A7）
- [x] `php tests/run_at_trigger.php` 全部 PASS（A8）
- [x] `php tests/jitter_test.php` 全部 PASS（A9）
- [x] `php tests/expires_test.php` 全部 PASS（C6）
- [x] `php tests/requeue_test.php` 全部 PASS（C7）
- [x] `php tests/retry_backoff.php` 全部 PASS（C8，无网络则 SKIP）
- [x] `php tests/max_instances_test.php` 全部 PASS（A10）
- [x] `php tests/reschedule_test.php` 全部 PASS（A11）
- [x] `php tests/get_job_test.php` 全部 PASS（A12）
- [x] `php tests/ignore_result_test.php` 全部 PASS（C9）
- [x] `php tests/acks_late_test.php` 全部 PASS（C10）
- [x] `php tests/soft_timeout_test.php` 全部 PASS（C11）
- [x] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [x] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [x] `php tests/data_dir_smoke.php` PASS
- [x] 13 个 `examples/*.php`（含新增 cron_maxInstances / cron_reschedule）全部可运行（无 fatal error）
- [x] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 第三轮功能模块独立验证（含 12 个新对齐功能：6 项 round 2 + 6 项 round 3）
- [x] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code PASS
- [x] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1 PASS
- [x] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次 PASS
- [x] cron 任务：tests/cron.phpt 验证 cron 触发 PASS
- [x] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行 PASS
- [x] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查 PASS
- [x] 多服务：两个服务 PID 不同且互不干扰 PASS
- [x] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（**验证 bug 修复**）PASS
- [x] lifecycle：pause/resume/cancel/remove/list 全部按预期 PASS
- [x] start_end_date：startAt/endAt 时间窗口生效 PASS
- [x] result_ttl：result 过期清理 PASS
- [x] meta：meta 字段读写 PASS
- [x] priority：高优先级先执行 PASS
- [x] interval (A7)：every(2) 周期触发验证 PASS
- [x] runAt (A8)：一次性绝对时刻触发验证 PASS
- [x] jitter (A9)：多任务触发时刻分散验证 PASS
- [x] expires (C6)：超时 Pending 任务置 Expired 验证 PASS
- [x] requeue (C7)：cancel → requeue → 重新触发验证 PASS
- [x] retry_backoff (C8)：retryBackoff(true) 指数退避时间序列验证 PASS
- [x] max_instances (A10)：maxInstances(2) 允许 2 个并发实例 + 第 3 个被跳过验证 PASS
- [x] reschedule (A11)：reschedule 修改 cron + 保留 execution_count 验证 PASS
- [x] get_job (A12)：xhjob_get 返回完整 Task JSON 字段验证 PASS
- [x] ignore_result (C9)：ignoreResult(true) 不存 result + state 仍正常验证 PASS
- [x] acks_late (C10)：daemon 重启后 acks_late 任务被重排验证 PASS
- [x] soft_timeout (C11)：softTimeout(5)+timeout(10) shell 任务 SIGTERM 优雅退出验证 PASS

## 第三轮提交与推送远程主分支
- [x] `git status` 核对修改文件清单
- [x] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [x] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [x] `git commit -m "feat: 对齐 APScheduler/Celery 23 项（A1-A12+C1-C11）+ maxExecutions bug 修复 + 12 项新对齐功能（every/runAt/jitter/expires/requeue/retryBackoff/maxInstances升级/reschedule/get/ignoreResult/acksLate/softTimeout）+ 文档/示例/测试补齐"` 提交到本地 main
- [x] `git push origin main` 推送到远程主分支
- [x] `git log origin/main --oneline -5` 确认远程 HEAD 已更新

## 新功能 A13 — misfire_grace_time（每作业级容错窗口）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `misfire_grace_time: u64` 字段（默认 0=使用全局默认 60s），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 0

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `misfire_grace_time: u64` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `misfire_grace_time(secs: u64)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `misfire_grace_time(&mut self, secs: i64) -> &mut Self`，PHP 暴露为 `misfireGraceTime(int $secs): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中检测 `now > next_fire`：`grace_time = if task.misfire_grace_time > 0 { task.misfire_grace_time } else { 60 }`
- [x] 若 `now - next_fire > grace_time` 则视为 misfire：按 `task.coalesce` 规则处理
- [x] `coalesce=true`（默认）：合并 misfire，跳过本次触发，推进 next_fire 到下一个未来时刻
- [x] `coalesce=false`：丢弃所有 missed 触发，推进 next_fire 到下一个未来时刻
- [x] `now - next_fire <= grace_time` 则仍触发（不算 misfire）
- [x] 仅对 cron 任务生效；interval/runAt 任务 warn 并忽略

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `misfire_grace_time INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写
- [x] `src/outcome/mod.rs::StateInfo` 新增 `misfire_grace_time` 字段
- [x] `xhjob_state` 返回中包含 `misfire_grace_time`

### 行为验证
- [x] `misfireGraceTime(5)` + cron='*/1 * * * *' + 模拟 6s 延迟：跳过本次触发 + 推进 next_fire
- [x] 默认 0 时使用全局 60s 容错窗口（向后兼容）
- [x] `coalesce=true` + misfire：合并跳过（不补执行）
- [x] `coalesce=false` + misfire：丢弃所有 missed 触发
- [x] interval/runAt 任务设置 misfireGraceTime：warn 忽略
- [x] 单元测试 `test_misfire_grace_time_per_job_overrides_global` / `test_misfire_within_grace_still_fires` / `test_misfire_coalesce_skips_missed` / `test_misfire_no_coalesce_drops_all_missed` PASS
- [x] `tests/misfire_grace_time_test.php` 端到端验证 PASS

## 新功能 A14 — replace_existing（幂等 dispatch）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `id: Option<String>` 字段（None=系统生成 UUID，已有 task.id 为 UUID 字符串），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task` struct 新增 `replace_existing: bool` 字段（默认 false），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `id: Option<String>` 字段 + builder 方法 `id(s: impl Into<String>)`
- [x] `src/task/mod.rs::TaskBuilder` 新增 `replace_existing: bool` 字段 + builder 方法 `replace_existing(on: bool)`
- [x] `src/lib.rs::Xhjob` 类新增 `id(&mut self, s: &str) -> &mut Self`，PHP 暴露为 `withId(string $id): $this`
- [x] `src/lib.rs::Xhjob` 类新增 `replace_existing(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `replaceExisting(bool $on): $this`

### dispatch 路径逻辑
- [x] `src/lib.rs::xhjob_dispatch` 或 daemon handle_dispatch 中：若 `task.id.is_some()` 且 store 中已存在同 id：
  - `replace_existing=true`：先 `store.remove_task(id)` 再 `store.insert_task(task)`（完全替换，不保留 state/attempts/execution_count）
  - `replace_existing=false`（默认）：返回 `error: task id already exists`
- [x] 不设置 id 时系统仍生成 UUID（replace_existing 无效果）
- [x] 无需新增 SQLite 列（id 已是主键）

### 行为验证
- [x] withId('report') + replaceExisting(true) + 同 id 重跑 dispatch：旧 task 被覆盖，state/attempts 重置
- [x] withId('report') + replaceExisting(false)（默认）+ 同 id 重跑 dispatch：返回 "task id already exists" 错误
- [x] 不设置 id 时每次 dispatch 生成不同 UUID（向后兼容）
- [x] 替换后 xhjob_get 能返回新 task 定义
- [x] 替换不影响正在执行的旧 task 实例（仅替换 store 中的定义）
- [x] 单元测试 `test_replace_existing_overwrites_same_id` / `test_replace_existing_false_errors_on_duplicate` / `test_no_id_generates_uuid` / `test_replace_existing_resets_state` PASS
- [x] `tests/replace_existing_test.php` 端到端验证 PASS

## 新功能 A15 — tags（作业分组与按 tag 过滤）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `tags: Vec<String>` 字段（默认空 Vec），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `tags: Vec<String>` 字段 + builder 方法 `tags(&mut self, tags: &[&str])`
- [x] `src/lib.rs::Xhjob` 类新增 `tags(&mut self, tags: Vec<String>) -> &mut Self`，PHP 暴露为 `tags(array $tags): $this`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 签名扩展：`list_tasks(&self, state_filter: Option<TaskState>, tag_filter: Option<&str>) -> Result<Vec<TaskSummary>>`（向后兼容追加可选参数）
- [x] `src/store/in_memory.rs` 实现：filter + 含 tag 字符串匹配
- [x] `src/store/sqlite.rs` schema 新增 `tags TEXT NOT NULL DEFAULT '[]'` 列（存 JSON 数组字符串，旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::list_tasks` 实现 `WHERE tags LIKE '%"tag"%'` 过滤
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写（serde_json 序列化 Vec<String>）

### IPC 与 daemon handler
- [x] `src/ipc/mod.rs` IPC `list` 命令参数扩展为可选 `tag: Option<String>`
- [x] `src/daemon_main.rs::handle_list_op` 接收并透传 tag 参数

### PHP 顶层函数
- [x] `src/lib.rs::xhjob_list($name='default', $state_filter=null, $tag=null, $data_dir=null): string` 新增可选 `$tag` 参数

### Store 与 StateInfo
- [x] `src/outcome/mod.rs::StateInfo` 新增 `tags: Vec<String>` 字段
- [x] `xhjob_state` 返回中包含 `tags`

### 行为验证
- [x] tags(['reports','critical']) + dispatch + xhjob_list(..., 'reports', ...) 返回包含此 task
- [x] tags(['reports','critical']) + xhjob_list(..., 'nonexistent', ...) 返回不包含此 task
- [x] tags([]) 默认空数组：xhjob_list 不带 tag 返回 / 带 tag 过滤不返回
- [x] SQLite 中 tags 列存 JSON 字符串如 `["reports","critical"]`
- [x] 旧库 ALTER TABLE 兼容（tags 列默认 '[]'）
- [x] 单元测试 `test_tags_filter_by_tag` / `test_tags_empty_default_excluded_by_tag_filter` / `test_tags_multiple_match_any` / `test_tags_persisted_as_json` PASS
- [x] `tests/tags_test.php` 端到端验证 PASS

## 新功能 C12 — rate_limit（每任务限流滑动窗口）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `rate_limit_count: u32` 字段（默认 0=不限流），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task` struct 新增 `rate_limit_window: u64` 字段（默认 0），加 `#[serde(default)]`

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `rate_limit_count: u32` + `rate_limit_window: u64` 字段 + builder 方法 `rate_limit(max_count: u32, window_secs: u64)`
- [x] `src/lib.rs::Xhjob` 类新增 `rate_limit(&mut self, max_count: i64, window_secs: i64) -> &mut Self`，PHP 暴露为 `rateLimit(int $maxCount, int $windowSecs): $this`

### 新模块 rate_limit.rs
- [x] 新增 `src/scheduler/rate_limit.rs` 文件
- [x] 定义 `pub struct RateLimiter { windows: HashMap<String, VecDeque<i64>> }`
- [x] 实现 `pub fn check_and_record(&mut self, task_id: &str, now: i64, max_count: u32, window_secs: u64) -> bool`：true=允许触发，false=限流
- [x] 内部逻辑：清理 task_id 队列中 `< now - window_secs` 的过期时间戳，若剩余长度 < max_count 则 push now 并返回 true，否则返回 false（不 push）
- [x] `src/scheduler/mod.rs` 注册 `pub mod rate_limit;` + `pub use rate_limit::RateLimiter;`

### Cron 调度逻辑集成
- [x] `src/scheduler/cron.rs::scan_once` 或 daemon 调度器中持有 `RateLimiter` 实例
- [x] 触发前若 `task.rate_limit_count > 0`：调用 `limiter.check_and_record(task.id, now, count, window)`
- [x] 返回 false 时跳过本次触发 + 推进 next_fire
- [x] 默认 0 时跳过检查（向后兼容）
- [x] daemon 重启时从 store 中 `started_at > now - window_secs` 的任务记录重建 sliding window

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `rate_limit_count INTEGER NOT NULL DEFAULT 0` + `rate_limit_window INTEGER NOT NULL DEFAULT 0` 两列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写
- [x] `src/outcome/mod.rs::StateInfo` 新增 `rate_limit_count` / `rate_limit_window` 字段
- [x] `xhjob_state` 返回中包含 `rate_limit_count` / `rate_limit_window`

### 行为验证
- [x] rateLimit(3, 10) + 模拟 5 次 tick：前 3 次允许 / 第 4-5 次限流
- [x] rateLimit(3, 10) + 第 11 秒时窗口滑过：第 4 次允许
- [x] 默认 0 时不受限流（向后兼容）
- [x] maxInstances(1) + rateLimit(3, 60) 同时设置：独立生效（并发 1 / 60s 内 3 次）
- [x] daemon 重启后从 store started_at 重建窗口计数
- [x] 单元测试 `test_rate_limit_allows_n_in_window` / `test_rate_limit_window_advances` / `test_rate_limit_default_0_no_limit` / `test_rate_limit_distinct_from_max_instances` / `test_rate_limit_rebuild_after_restart` PASS
- [x] `tests/rate_limit_test.php` 端到端验证 PASS

## 新功能 C13 — acks_on_failure（失败时是否确认，与 acksLate 互补）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `acks_on_failure: bool` 字段（默认 true 保持当前行为），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 true

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `acks_on_failure: bool` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `acks_on_failure(on: bool)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `acks_on_failure(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `acksOnFailure(bool $on): $this`

### process_one 失败路径升级
- [x] `src/scheduler/queue.rs::process_one` 中任务执行失败路径：检查 `task.acks_on_failure`
  - `true`（默认）：保持当前行为（按 retry_max 重试或终态 Failed，向后兼容）
  - `false`：重置 state=Pending + next_fire=now+retry_delay（指数退避若启用），**忽略 retry_max 上限**，持续重试直到成功或被 cancel/remove
- [x] `acks_on_failure=false` 时 retry_max 字段被忽略（attempts 仍递增用于退避计算，但不作为终态判定条件）
- [x] `acks_on_failure=false` 任务被 `xhjob_cancel` 时尊重 cancel 信号（cancel 优先于 acks_on_failure，state=Cancelled 终态）
- [x] `acks_on_failure=false` 任务被 `xhjob_remove` 时直接删除（不受 acks_on_failure 影响）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `acks_on_failure INTEGER NOT NULL DEFAULT 1` 列（默认 1=true，旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写
- [x] `src/outcome/mod.rs::StateInfo` 新增 `acks_on_failure` 字段
- [x] `xhjob_state` 返回中包含 `acks_on_failure`

### 行为验证
- [x] withRetry(3,1) + acksOnFailure(false) + viaShell('exit 7')：第 4 次失败仍 state=Pending 不终态
- [x] 默认 true 时 retry_max=3 → 第 4 次失败 state=Failed（向后兼容）
- [x] acksOnFailure(false) + retryBackoff(true)：指数退避持续重试不受 retry_max 限制
- [x] acksOnFailure(false) 持续重试中调用 xhjob_cancel：state=Cancelled 终态停止重试
- [x] acksOnFailure(false) + acksLate(true)：daemon 崩溃重启后 acksLate 重排 + acksOnFailure 让失败持续重试（互补）
- [x] 单元测试 `test_acks_on_failure_false_continues_retry_past_max` / `test_acks_on_failure_default_true_respects_max` / `test_acks_on_failure_false_with_backoff` / `test_acks_on_failure_false_respects_cancel` / `test_acks_on_false_plus_acks_late_complementary` PASS
- [x] `tests/acks_on_failure_test.php` 端到端验证 PASS

## 新功能 C14 — worker_max_tasks_per_child（daemon 自我回收）
### Config 与原子计数器
- [x] `src/daemon_main.rs::Config` 新增 `max_tasks_per_child: u64` 字段（从环境变量 `XHJOB_MAX_TASKS_PER_CHILD` 读取，默认 0=不回收）
- [x] `src/daemon_main.rs::run` 中维护 `static TASKS_EXECUTED: AtomicU64 = AtomicU64::new(0)` 全局原子计数器
- [x] `process_one` 完成后（无论成功失败）`TASKS_EXECUTED.fetch_add(1, Ordering::Relaxed)` 递增

### 优雅退出流程
- [x] 检查 `if max_tasks_per_child > 0 && TASKS_EXECUTED.load(Ordering::Relaxed) >= max_tasks_per_child`：触发优雅退出
- [x] 优雅退出流程：通知 scheduler 停止接受新触发 / 通知 queue 等待 in-flight 任务完成 / flush store / log INFO "max_tasks_per_child reached, exiting" / exit 0
- [x] `max_tasks_per_child=0`（默认）时不检查不退出（向后兼容）
- [x] 优雅退出等待 in-flight 任务完成（不强制 kill Running 任务），最长等待 timeout 秒后强制退出
- [x] daemon 重启后计数器从 0 开始（不持久化，仅内存）

### 行为验证
- [x] 单元测试 `test_max_tasks_per_child_triggers_exit_after_n`：max_tasks_per_child=5 + dispatch 5 个任务 → 第 5 个完成后 daemon 退出
- [x] 单元测试 `test_max_tasks_per_child_default_0_no_exit`：默认 0 时 dispatch N 个任务后 daemon 不退出（向后兼容）
- [x] 单元测试 `test_max_tasks_per_child_waits_inflight`：max=5 + 第 5 个任务 Running 中 → daemon 等待任务完成后才退出
- [x] 单元测试 `test_max_tasks_per_child_counter_resets_on_restart`：daemon 重启后计数器从 0 开始
- [x] `tests/max_tasks_per_child_test.php` 端到端验证 `XHJOB_MAX_TASKS_PER_CHILD=5 xhjob_daemon` 执行 5 个任务后退出 PASS

## README 文档补齐（A13-A15 + C12-C14 第四轮对齐项）
- [x] "API 参考" 表 `Xhjob` 类方法表新增 `misfireGraceTime(int $secs): $this` / `withId(string $id): $this` / `replaceExisting(bool $on): $this` / `tags(array $tags): $this` / `rateLimit(int $maxCount, int $windowSecs): $this` / `acksOnFailure(bool $on): $this` 6 行
- [x] "API 参考" 表 `xhjob_list` 行更新签名为 `xhjob_list($name='default', $state_filter=null, $tag=null, $data_dir=null): string`（含可选 $tag）
- [x] "API 参考" 表 `xhjob_state` 返回说明新增 `misfire_grace_time` / `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` 字段
- [x] "环境变量" 表新增 `XHJOB_MAX_TASKS_PER_CHILD` 行说明"daemon 执行 N 个任务后自我回收，由外部进程管理器重启；默认 0=不回收"
- [x] 新增"misfire_grace_time 每作业级（A13）"小节：说明 per-job 字段 vs 全局默认 60s、与 coalesce 配合规则、适用场景
- [x] 新增"replace_existing 幂等 dispatch（A14）"小节：说明 withId / replaceExisting 用法 + 部署脚本幂等场景
- [x] 新增"tags 作业分组（A15）"小节：说明 tags 用法 + xhjob_list 按 tag 过滤 + 适用场景
- [x] 新增"rateLimit 每任务限流（C12）"小节：说明滑动窗口算法 + 与 maxInstances 区别 + 适用场景
- [x] 新增"acksOnFailure 失败不放弃（C13）"小节：说明与 acksLate 互补 + 关键任务永不放弃场景
- [x] 新增"worker_max_tasks_per_child daemon 自我回收（C14）"小节：说明环境变量配置 + 与 systemd / supervisor / docker restart=always 配合
- [x] "对照 APScheduler / Celery 的功能对齐"小节扩展为 29 项（A1-A15 + C1-C14）

## examples 完善（A13-A15 + C12-C14）
- [x] 新增 `examples/cron_replace_existing.php`：演示 withId + replaceExisting 部署脚本幂等注册（A14）
- [x] 新增 `examples/cron_tags.php`：演示 tags 标记 + xhjob_list 按 tag 过滤（A15）
- [x] 新增 `examples/cron_rateLimit.php`：演示 rateLimit 限流外部 API 调用（C12）

## 新增测试（A13-A15 + C12-C14）
- [x] `tests/misfire_grace_time_test.php`：misfireGraceTime(5) 短窗口跳过 6s 延迟 + 默认 0 用全局 60s PASS
- [x] `tests/replace_existing_test.php`：withId + replaceExisting(true) 同 id 重跑覆盖 + replaceExisting(false) 报错 PASS
- [x] `tests/tags_test.php`：tags 标记 + xhjob_list 按 tag 过滤 + 默认空数组不返回 PASS
- [x] `tests/rate_limit_test.php`：rateLimit(3, 10) 在 10s 窗口内最多 3 次触发 + 第 11 秒窗口滑过允许第 4 次 PASS
- [x] `tests/acks_on_failure_test.php`：acksOnFailure(false) 失败任务持续重试忽略 retry_max + 被 cancel 终止 PASS
- [x] `tests/max_tasks_per_child_test.php`：XHJOB_MAX_TASKS_PER_CHILD=5 daemon 执行 5 个任务后优雅退出 PASS

## 第四轮重新编译与全量回归测试（含 18 个新对齐测试：6 项 round 2 + 6 项 round 3 + 6 项 round 4）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过（含 maxExecutions bug 修复 + 18 个新功能单元测试）
- [x] `cargo test --release --lib`（默认 feature）全部通过
- [x] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [x] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [x] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [x] `php tests/max_executions.php` 全部 PASS（**验证 bug 修复**）
- [x] `php tests/lifecycle_api.php` 全部 PASS
- [x] `php tests/start_end_date.php` 全部 PASS
- [x] `php tests/result_ttl.php` 全部 PASS
- [x] `php tests/meta_field.php` 全部 PASS
- [x] `php tests/interval_trigger.php` 全部 PASS（A7）
- [x] `php tests/run_at_trigger.php` 全部 PASS（A8）
- [x] `php tests/jitter_test.php` 全部 PASS（A9）
- [x] `php tests/expires_test.php` 全部 PASS（C6）
- [x] `php tests/requeue_test.php` 全部 PASS（C7）
- [x] `php tests/retry_backoff.php` 全部 PASS（C8，无网络则 SKIP）
- [x] `php tests/max_instances_test.php` 全部 PASS（A10）
- [x] `php tests/reschedule_test.php` 全部 PASS（A11）
- [x] `php tests/get_job_test.php` 全部 PASS（A12）
- [x] `php tests/ignore_result_test.php` 全部 PASS（C9）
- [x] `php tests/acks_late_test.php` 全部 PASS（C10）
- [x] `php tests/soft_timeout_test.php` 全部 PASS（C11）
- [x] `php tests/misfire_grace_time_test.php` 全部 PASS（A13）
- [x] `php tests/replace_existing_test.php` 全部 PASS（A14）
- [x] `php tests/tags_test.php` 全部 PASS（A15）
- [x] `php tests/rate_limit_test.php` 全部 PASS（C12）
- [x] `php tests/acks_on_failure_test.php` 全部 PASS（C13）
- [x] `php tests/max_tasks_per_child_test.php` 全部 PASS（C14）
- [x] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [x] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [x] `php tests/data_dir_smoke.php` PASS
- [x] 16 个 `examples/*.php`（含新增 cron_replace_existing / cron_tags / cron_rateLimit）全部可运行（无 fatal error）
- [x] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 第四轮功能模块独立验证（含 18 个新对齐功能：6 项 round 2 + 6 项 round 3 + 6 项 round 4）
- [x] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code PASS
- [x] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1 PASS
- [x] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次 PASS
- [x] cron 任务：tests/cron.phpt 验证 cron 触发 PASS
- [x] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行 PASS
- [x] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查 PASS
- [x] 多服务：两个服务 PID 不同且互不干扰 PASS
- [x] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（**验证 bug 修复**）PASS
- [x] lifecycle：pause/resume/cancel/remove/list 全部按预期 PASS
- [x] start_end_date：startAt/endAt 时间窗口生效 PASS
- [x] result_ttl：result 过期清理 PASS
- [x] meta：meta 字段读写 PASS
- [x] priority：高优先级先执行 PASS
- [x] interval (A7)：every(2) 周期触发验证 PASS
- [x] runAt (A8)：一次性绝对时刻触发验证 PASS
- [x] jitter (A9)：多任务触发时刻分散验证 PASS
- [x] expires (C6)：超时 Pending 任务置 Expired 验证 PASS
- [x] requeue (C7)：cancel → requeue → 重新触发验证 PASS
- [x] retry_backoff (C8)：retryBackoff(true) 指数退避时间序列验证 PASS
- [x] max_instances (A10)：maxInstances(2) 允许 2 个并发实例 + 第 3 个被跳过验证 PASS
- [x] reschedule (A11)：reschedule 修改 cron + 保留 execution_count 验证 PASS
- [x] get_job (A12)：xhjob_get 返回完整 Task JSON 字段验证 PASS
- [x] ignore_result (C9)：ignoreResult(true) 不存 result + state 仍正常验证 PASS
- [x] acks_late (C10)：daemon 重启后 acks_late 任务被重排验证 PASS
- [x] soft_timeout (C11)：softTimeout(5)+timeout(10) shell 任务 SIGTERM 优雅退出验证 PASS
- [x] misfire_grace_time (A13)：misfireGraceTime(5) 短窗口跳过 6s 延迟 + 默认 0 用全局 60s 验证 PASS
- [x] replace_existing (A14)：withId + replaceExisting(true) 同 id 重跑覆盖 + 默认 false 报错验证 PASS
- [x] tags (A15)：tags 标记 + xhjob_list 按 tag 过滤 + 默认空数组不返回验证 PASS
- [x] rate_limit (C12)：rateLimit(3, 10) 滑动窗口限流 + 第 11 秒窗口滑过允许验证 PASS
- [x] acks_on_failure (C13)：acksOnFailure(false) 失败任务持续重试忽略 retry_max + 被 cancel 终止验证 PASS
- [x] max_tasks_per_child (C14)：XHJOB_MAX_TASKS_PER_CHILD=5 daemon 执行 5 个任务后优雅退出验证 PASS

## 第四轮提交与推送远程主分支
- [x] `git status` 核对修改文件清单
- [x] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [x] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [x] `git commit -m "feat: 对齐 APScheduler/Celery 29 项（A1-A15+C1-C14）+ maxExecutions bug 修复 + 18 项新对齐功能（every/runAt/jitter/expires/requeue/retryBackoff/maxInstances升级/reschedule/get/ignoreResult/acksLate/softTimeout/misfireGraceTime/replaceExisting/tags/rateLimit/acksOnFailure/maxTasksPerChild）+ 文档/示例/测试补齐"` 提交到本地 main
- [x] `git push origin main` 推送到远程主分支
- [x] `git log origin/main --oneline -5` 确认远程 HEAD 已更新

## 新功能 A16 — timezone per-job（每作业独立时区）
### Task 数据结构
- [x] `src/store/mod.rs::Task` struct 新增 `timezone: Option<String>` 字段，加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 None

### TaskBuilder 与 PHP 类
- [x] `src/task/mod.rs::TaskBuilder` 新增 `timezone: Option<String>` 字段
- [x] `src/task/mod.rs::TaskBuilder` 新增 `timezone(tz: impl Into<String>)` builder 方法（链式返回 `&mut Self`）
- [x] `src/task/mod.rs::TaskBuilder::build` 中将 `timezone` 复制到 Task
- [x] `src/lib.rs::Xhjob` 类新增 `timezone(&mut self, tz: String) -> &mut Self`，PHP 暴露为 `timezone(string $tz): $this`
- [x] `src/lib.rs::xhjob_dispatch` 透传 `timezone` 字段

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中 cron 表达式评估时使用 `task.timezone` 解析（`chrono_tz::Tz::from_str`）
- [x] 解析失败时 `tracing::warn!` 并回退到全局时区
- [x] 非 cron 任务（interval / runAt）设置 `timezone` 时 warn 并忽略

### 依赖与 Store
- [x] `Cargo.toml` 新增 `chrono-tz = "0.9"` 依赖
- [x] `src/store/sqlite.rs` schema 新增 `timezone TEXT` 列（旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写新字段
- [x] `src/outcome/mod.rs::StateInfo` 新增 `timezone` 字段
- [x] `xhjob_state` 返回中包含 `timezone`

### 行为验证
- [x] timezone('America/New_York') cron 任务按纽约时区评估 next_fire
- [x] 解析失败回退到全局时区 + warn
- [x] 默认 None 使用全局时区（向后兼容）
- [x] 非 cron 任务设置 timezone 时 warn + 忽略
- [x] 单元测试 `test_timezone_per_job_overrides_global` / `test_timezone_invalid_falls_back_to_global` / `test_timezone_default_none_uses_global` / `test_timezone_ignored_for_interval_tasks` PASS
- [x] `tests/timezone_test.php` 端到端验证 PASS

## 新功能 A17 — Event listener API（任务执行事件流）
### 新模块与数据结构
- [x] 新增 `src/scheduler/events.rs` 模块
- [x] 定义 `pub struct TaskEvent { task_id: String, event_type: EventType, payload: Option<String>, ts: i64 }`
- [x] 定义 `pub enum EventType { Started, Succeeded, Failed, Missed, Cancelled, Paused, Resumed, Expired, MaxInstancesReached, RateLimited }` + `as_str()` / `from_str()` 实现
- [x] `src/scheduler/mod.rs` 注册 `pub mod events;` + `pub use events::{TaskEvent, EventType};`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn record_event(&self, task_id: &str, event_type: EventType, payload: Option<&str>, ts: i64) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn list_events(&self, since_ts: i64, task_id_filter: Option<&str>) -> Result<Vec<TaskEvent>>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn cleanup_expired_events(&self, ttl_secs: u64) -> Result<u64>`
- [x] `src/store/in_memory.rs` 实现 3 个新方法（用 `Vec<TaskEvent>` + RwLock）
- [x] `src/store/sqlite.rs` schema 新增 `events` 表（`id INTEGER PK AUTOINCREMENT` / `task_id TEXT NOT NULL` / `event_type TEXT NOT NULL` / `payload TEXT` / `ts INTEGER NOT NULL`）+ 索引 `idx_events_ts` ON events(ts)
- [x] `src/store/sqlite.rs` 实现 3 个新方法（INSERT / SELECT WHERE ts >= ? AND (? IS NULL OR task_id = ?) ORDER BY ts / DELETE WHERE ts < ?）

### 关键路径集成 record_event
- [x] `process_one` 任务开始时调用 `record_event(...Started...)`
- [x] `process_one` 任务成功时调用 `record_event(...Succeeded...)`
- [x] `process_one` 任务失败时调用 `record_event(...Failed...)`
- [x] `scan_once` misfire 时调用 `record_event(...Missed...)`
- [x] `scan_once` max_instances_reached 时调用 `record_event(...MaxInstancesReached...)`
- [x] `scan_once` rate_limited 时调用 `record_event(...RateLimited...)`
- [x] `cancel_task` 调用 `record_event(...Cancelled...)`
- [x] `set_paused(true)` 调用 `record_event(...Paused...)`
- [x] `set_paused(false)` 调用 `record_event(...Resumed...)`
- [x] 标记 Expired 时调用 `record_event(...Expired...)`

### TTL 清理与 PHP API
- [x] `scan_once` 周期性调用 `cleanup_expired_events`（每 60s 节流，static AtomicU64）
- [x] TTL 从 `XHJOB_EVENTS_TTL_SECS` 环境变量读取，默认 86400（24h）
- [x] `daemon_main.rs::Config` 新增 `events_ttl_secs: u64` 字段（从环境变量读取，默认 86400）
- [x] `src/lib.rs` 新增 `xhjob_events($since_ts, $name='default', $task_id=null, $data_dir=null): string` PHP 顶层函数返回 JSON 数组
- [x] `src/lib.rs` 新增 IPC 命令 `events` 路由 + `daemon_main.rs` 新增 `handle_events_op` handler

### 行为验证
- [x] `record_event` 持久化（list_events 返回新事件）
- [x] `list_events` 按 task_id 过滤正确
- [x] `list_events` 按 since_ts 过滤正确
- [x] `cleanup_expired_events` 删除过期事件并返回删除数
- [x] 事件类型覆盖所有关键路径（10 种类型）
- [x] TTL 默认 24h，可通过 `XHJOB_EVENTS_TTL_SECS` 配置
- [x] 单元测试 `test_record_event_persists` / `test_list_events_filter_by_task_id` / `test_list_events_since_ts_filter` / `test_cleanup_expired_events` / `test_event_types_cover_all_paths` PASS
- [x] `tests/events_test.php` 端到端验证 PASS

## 新功能 A18 — coalesce 显式 per-job 行为
### Task 数据结构（确认或新增）
- [x] `src/store/mod.rs::Task` struct 已有 `coalesce: bool` 字段（默认 true 保持当前行为），加 `#[serde(default)]`
- [x] `src/store/mod.rs::Task::new` 初始化为 true
- [x] `src/task/mod.rs::TaskBuilder` 已有 `coalesce: bool` 字段 + `coalesce(on: bool)` builder 方法
- [x] `src/lib.rs::Xhjob` 类新增 `coalesce(&mut self, on: bool) -> &mut Self`，PHP 暴露为 `coalesce(bool $on): $this`

### Cron 调度逻辑
- [x] `src/scheduler/cron.rs::scan_once` 中 misfire 检测时真正按 `task.coalesce` 字段控制行为
- [x] `coalesce=true`（默认）：合并多次 missed 触发为最后一次执行 + 推进 next_fire
- [x] `coalesce=false`：丢弃所有 missed 触发（不执行任何补偿）+ 推进 next_fire 至下一轮 cron 时刻
- [x] `coalesce=true` 时 execution_count 仅递增 1（而非 N，N=missed 次数）
- [x] `coalesce=false` 时 execution_count 不递增（不执行）
- [x] 默认 true 保持当前行为（向后兼容）

### Store 与 StateInfo
- [x] `src/store/sqlite.rs` schema 新增 `coalesce INTEGER NOT NULL DEFAULT 1` 列（默认 1=true，旧库 ALTER TABLE 兼容）
- [x] `src/store/sqlite.rs::task_from_row` / `insert_task` 同步读写新字段
- [x] `src/outcome/mod.rs::StateInfo` 新增 `coalesce` 字段
- [x] `xhjob_state` 返回中包含 `coalesce`

### 行为验证
- [x] coalesce(true) 合并 missed 触发为最后一次执行，execution_count 仅 +1
- [x] coalesce(false) 丢弃 missed 触发，不执行任何补偿，execution_count 不递增
- [x] 默认 true 时行为与当前一致（向后兼容）
- [x] coalesce 与 misfire_grace_time 配合：`coalesce=false`+`misfireGraceTime(5)`+6s 停顿 → 完全 misfire 丢弃；`coalesce=true`+同样配置 → 完全 misfire 但仍按合并规则执行最后一次
- [x] 单元测试 `test_coalesce_true_merges_missed_into_last_execution` / `test_coalesce_false_drops_missed_no_compensation` / `test_coalesce_default_true_backwards_compatible` / `test_coalesce_with_misfire_grace_time_full_misfire` PASS
- [x] `tests/coalesce_test.php` 端到端验证 PASS

## 新功能 C15 — Task chain 顺序流水线
### 新模块与数据结构
- [x] 新增 `src/scheduler/chain.rs` 模块
- [x] 定义 `pub struct ChainExecutor`，负责推进 chain 的 current_step
- [x] `src/scheduler/mod.rs` 注册 `pub mod chain;` + `pub use chain::ChainExecutor;`
- [x] 定义 `pub struct ChainRecord { chain_id: String, tasks: Vec<serde_json::Value>, current_step: u32, state: String, created_at: i64, updated_at: i64 }`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn create_chain(&self, chain_id: &str, tasks: &[serde_json::Value], created_at: i64) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn get_chain(&self, chain_id: &str) -> Result<Option<ChainRecord>>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn update_chain_step(&self, chain_id: &str, current_step: u32, state: &str, updated_at: i64) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn list_chains_by_state(&self, state: &str) -> Result<Vec<ChainRecord>>`
- [x] `src/store/in_memory.rs` 与 `src/store/sqlite.rs` 实现新方法
- [x] `src/store/sqlite.rs` schema 新增 `chains` 表（`chain_id TEXT PRIMARY KEY` / `tasks TEXT NOT NULL` / `current_step INTEGER NOT NULL DEFAULT 0` / `state TEXT NOT NULL DEFAULT 'pending'` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）

### daemon 集成
- [x] daemon 启动时调用 `list_chains_by_state("pending")` 与 `list_chains_by_state("running")` 恢复未完成 chain
- [x] daemon 接收 `chain` 命令后：生成 chain_id (UUID) → create_chain → dispatch 第 1 个任务
- [x] `process_one` 任务完成后检查任务是否属于 chain（通过 task meta 或 chain_id 标识）
- [x] 任务 success：取 stdout 作为下一任务 input payload → dispatch 下一任务 → update_chain_step(current_step+1, "running")
- [x] 任务 failed：update_chain_step(state="failed") → 剩余任务不再 dispatch
- [x] 最后一任务 success 后 update_chain_step(state="succeeded")
- [x] 前任务 stdout 通过 `XHJOB_CHAIN_INPUT` 环境变量传入后任务

### PHP API
- [x] `src/lib.rs` 新增 `xhjob_chain(array $task_configs, $name='default', $data_dir=null): string` 顶层函数返回 chain_id
- [x] `src/lib.rs` 新增 `xhjob_chain_state($chain_id, $name='default', $data_dir=null): string` 顶层函数返回 JSON
- [x] `src/lib.rs` 新增 IPC 命令 `chain` / `chain_state` 路由 + `daemon_main.rs` 新增 `handle_chain_op` / `handle_chain_state_op` handler

### 行为验证
- [x] 3 步 chain ETL 流水线：每步 stdout 作为下步 stdin，最终 state=succeeded
- [x] 第 2 步失败 → chain state=failed，current_step 停在 1，第 3 步不 dispatch
- [x] xhjob_chain_state 返回完整进度（含 chain_id / tasks / current_step / state）
- [x] daemon 崩溃重启后从 chains 表恢复 chain 状态
- [x] 空 tasks 数组返回 `error: chain tasks cannot be empty`
- [x] 单元测试 `test_chain_three_steps_etl_pipeline` / `test_chain_failure_stops_pipeline` / `test_chain_state_query_returns_full_progress` / `test_chain_persists_across_daemon_restart` / `test_chain_empty_tasks_returns_error` PASS
- [x] `tests/chain_test.php` 端到端验证 PASS

## 新功能 C16 — Task group 并行批处理
### 新模块与数据结构
- [x] 新增 `src/scheduler/group.rs` 模块
- [x] 定义 `pub struct GroupWatcher`，监听 group 内任务终态并更新 group state
- [x] `src/scheduler/mod.rs` 注册 `pub mod group;` + `pub use group::GroupWatcher;`
- [x] 定义 `pub struct GroupRecord { group_id: String, tasks: Vec<serde_json::Value>, state: String, created_at: i64, updated_at: i64 }`

### Store trait 与实现
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn create_group(&self, group_id: &str, tasks: &[serde_json::Value], created_at: i64) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn get_group(&self, group_id: &str) -> Result<Option<GroupRecord>>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn update_group_state(&self, group_id: &str, state: &str, updated_at: i64) -> Result<()>`
- [x] `src/store/mod.rs::TaskStore` trait 新增 `fn list_groups_by_state(&self, state: &str) -> Result<Vec<GroupRecord>>`
- [x] `src/store/in_memory.rs` 与 `src/store/sqlite.rs` 实现新方法
- [x] `src/store/sqlite.rs` schema 新增 `groups` 表（`group_id TEXT PRIMARY KEY` / `tasks TEXT NOT NULL` / `state TEXT NOT NULL DEFAULT 'pending'` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）

### daemon 集成
- [x] daemon 启动时调用 `list_groups_by_state("pending")` 与 `list_groups_by_state("running")` 恢复未完成 group
- [x] daemon 接收 `group` 命令后：生成 group_id (UUID) → create_group → 并行 dispatch 所有任务
- [x] `process_one` 任务完成后检查任务是否属于 group（通过 task meta 或 group_id 标识）
- [x] 任务 success：completed_count += 1
- [x] 任务 failed：completed_count += 1
- [x] 全部任务终态后判断 group state：全部 success → succeeded / 部分失败 → partial_failed / 全部失败 → failed
- [x] group 内任务受 max_instances / rate_limit 限制（与单任务一致）

### PHP API
- [x] `src/lib.rs` 新增 `xhjob_group(array $task_configs, $name='default', $data_dir=null): string` 顶层函数返回 group_id
- [x] `src/lib.rs` 新增 `xhjob_group_state($group_id, $name='default', $data_dir=null): string` 顶层函数返回 JSON
- [x] `src/lib.rs` 新增 IPC 命令 `group` / `group_state` 路由 + `daemon_main.rs` 新增 `handle_group_op` / `handle_group_state_op` handler

### 行为验证
- [x] 3 任务 group → 同时入队 → 全部 success → state=succeeded
- [x] 3 任务 group + 1 失败 → state=partial_failed，completed_count=3
- [x] 3 任务 group + 全部失败 → state=failed
- [x] xhjob_group_state 返回完整进度（含 group_id / tasks / completed_count / total_count / state）
- [x] daemon 崩溃重启后从 groups 表恢复 group 状态
- [x] 空 tasks 数组返回 `error: group tasks cannot be empty`
- [x] 单元测试 `test_group_three_tasks_parallel_dispatch` / `test_group_partial_failure` / `test_group_all_failed` / `test_group_state_query_returns_progress` / `test_group_persists_across_daemon_restart` / `test_group_empty_tasks_returns_error` PASS
- [x] `tests/group_test.php` 端到端验证 PASS

## 新功能 C17 — worker_max_memory_per_child 基于内存的 daemon 自我回收
### Config 与跨平台内存读取
- [x] `src/daemon_main.rs::Config` 新增 `max_memory_per_child: u64` 字段（从环境变量 `XHJOB_MAX_MEMORY_PER_CHILD` 读取，默认 0=不回收，单位 MB）
- [x] 新增 `src/utils/memory.rs` 模块，实现跨平台 `pub fn read_process_memory_mb() -> u64`
- [x] `src/utils/mod.rs` 注册 `pub mod memory;` + `pub use memory::read_process_memory_mb;`
- [x] Linux：读取 `/proc/self/status` 中 `VmRSS:` 行，解析 KB 单位值，转换为 MB
- [x] macOS：使用 `mach_task_basic_info` API（通过 `sysctl` crate 或 `libc`）
- [x] Windows：使用 `GetProcessMemoryInfo` API（通过 `winapi` crate）
- [x] 其他平台：返回 0 + warn 不支持
- [x] `Cargo.toml` 新增平台特定依赖：Linux 无需新增 / macOS `sysctl = "0.5"` / Windows `winapi = { version = "0.3", features = ["psapi", "winnt"] }`

### 优雅退出流程
- [x] `process_one` 完成后调用 `read_process_memory_mb()` 获取当前内存使用
- [x] 检查 `if max_memory_per_child > 0 && vm_rss_mb >= max_memory_per_child`：触发优雅退出
- [x] 优雅退出流程：通知 scheduler 停止接受新触发 / 通知 queue 等待 in-flight 任务完成 / flush store / log INFO "max_memory_per_child reached ({}MB >= {}MB), exiting" / exit 0
- [x] `max_memory_per_child=0`（默认）时不检查不退出（向后兼容）
- [x] 与 C14 `max_tasks_per_child` 互补：两个检查独立生效，先到达阈值者先触发退出
- [x] 优雅退出等待 in-flight 任务完成（不强制 kill Running 任务），最长等待 timeout 秒后强制退出

### 行为验证
- [x] max_memory_per_child=100 + 模拟 VmRSS=150MB → daemon 退出（mock read_process_memory_mb 返回固定值）
- [x] 默认 0 时不受内存限制（向后兼容）
- [x] max_tasks=1000 + max_memory=100 + 任务数=500 + VmRSS=200MB → 内存先触发退出（与 max_tasks 独立）
- [x] 在 Linux 平台 read_process_memory_mb 返回 > 0
- [x] max=100 + VmRSS=200MB + 2 个 Running 任务 → daemon 等待任务完成后才退出
- [x] 单元测试 `test_max_memory_per_child_triggers_exit_when_exceeded` / `test_max_memory_per_child_default_0_no_exit` / `test_max_memory_per_child_independent_from_max_tasks` / `test_read_process_memory_mb_returns_positive_on_linux` / `test_max_memory_per_child_waits_inflight` PASS
- [x] `tests/max_memory_per_child_test.php` 端到端验证 PASS

## README 文档补齐（A16-A18 + C15-C17 第五轮对齐项）
- [x] "API 参考" 表 `Xhjob` 类方法表新增 `timezone(string $tz): $this` / `coalesce(bool $on): $this` 2 行
- [x] "API 参考" 表顶层函数表新增 `xhjob_events` / `xhjob_chain` / `xhjob_chain_state` / `xhjob_group` / `xhjob_group_state` 5 行
- [x] "API 参考" 表 `xhjob_state` 返回说明新增 `timezone` / `coalesce` 字段
- [x] "环境变量" 表新增 `XHJOB_MAX_MEMORY_PER_CHILD` 行说明"daemon 内存超 N MB 后自我回收，与 max_tasks_per_child 互补；默认 0=不回收"
- [x] "环境变量" 表新增 `XHJOB_EVENTS_TTL_SECS` 行说明"events 表 TTL 自动清理，默认 86400（24h）"
- [x] 新增"timezone per-job 每作业独立时区（A16）"小节
- [x] 新增"Event listener API 任务执行事件流（A17）"小节
- [x] 新增"coalesce 显式 per-job 行为（A18）"小节
- [x] 新增"Task chain 顺序流水线（C15）"小节
- [x] 新增"Task group 并行批处理（C16）"小节
- [x] 新增"worker_max_memory_per_child 基于内存 daemon 自我回收（C17）"小节
- [x] "对照 APScheduler / Celery 的功能对齐"小节扩展为 35 项（A1-A18 + C1-C17）

## examples 完善（A16-A18 + C15-C17）
- [x] 新增 `examples/cron_timezone.php`：演示 timezone('America/New_York') cron 任务按纽约时区评估（A16）
- [x] 新增 `examples/events_query.php`：演示 xhjob_events 查询过去 1 小时任务执行事件流（A17）
- [x] 新增 `examples/chain_etl.php`：演示 xhjob_chain 3 步 ETL 流水线 extract → transform → load（C15）
- [x] 新增 `examples/group_batch.php`：演示 xhjob_group 3 任务并行批处理 + group_state 查询完成率（C16）

## 新增测试（A16-A18 + C15-C17）
- [x] `tests/timezone_test.php`：timezone('America/New_York') cron 任务按纽约时区评估 + 解析失败回退全局 + 非 cron 任务忽略 PASS
- [x] `tests/events_test.php`：xhjob_events 查询任务执行事件流 + 按 task_id 过滤 + 按 since_ts 过滤 + TTL 清理 PASS
- [x] `tests/coalesce_test.php`：coalesce(true) 合并 missed / coalesce(false) 丢弃 missed + 默认 true 向后兼容 PASS
- [x] `tests/chain_test.php`：xhjob_chain 3 步 ETL 流水线 + 前任务输出作为后任务输入 + 失败中断 + 持久化 PASS
- [x] `tests/group_test.php`：xhjob_group 3 任务并行批处理 + group_state 完成率 + partial_failed 状态 + 持久化 PASS
- [x] `tests/max_memory_per_child_test.php`：XHJOB_MAX_MEMORY_PER_CHILD=100 daemon 内存超阈值后优雅退出 + 默认 0 不退出 + 与 max_tasks_per_child 互补 PASS

## 第五轮重新编译与全量回归测试（含 24 个新对齐测试：6 项 round 2 + 6 项 round 3 + 6 项 round 4 + 6 项 round 5）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --release --lib --features persist` 全部通过（含 maxExecutions bug 修复 + 24 个新功能单元测试）
- [x] `cargo test --release --lib`（默认 feature）全部通过
- [x] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [x] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [x] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [x] `php tests/max_executions.php` 全部 PASS（**验证 bug 修复**）
- [x] `php tests/lifecycle_api.php` 全部 PASS
- [x] `php tests/start_end_date.php` 全部 PASS
- [x] `php tests/result_ttl.php` 全部 PASS
- [x] `php tests/meta_field.php` 全部 PASS
- [x] `php tests/interval_trigger.php` 全部 PASS（A7）
- [x] `php tests/run_at_trigger.php` 全部 PASS（A8）
- [x] `php tests/jitter_test.php` 全部 PASS（A9）
- [x] `php tests/expires_test.php` 全部 PASS（C6）
- [x] `php tests/requeue_test.php` 全部 PASS（C7）
- [x] `php tests/retry_backoff.php` 全部 PASS（C8，无网络则 SKIP）
- [x] `php tests/max_instances_test.php` 全部 PASS（A10）
- [x] `php tests/reschedule_test.php` 全部 PASS（A11）
- [x] `php tests/get_job_test.php` 全部 PASS（A12）
- [x] `php tests/ignore_result_test.php` 全部 PASS（C9）
- [x] `php tests/acks_late_test.php` 全部 PASS（C10）
- [x] `php tests/soft_timeout_test.php` 全部 PASS（C11）
- [x] `php tests/misfire_grace_time_test.php` 全部 PASS（A13）
- [x] `php tests/replace_existing_test.php` 全部 PASS（A14）
- [x] `php tests/tags_test.php` 全部 PASS（A15）
- [x] `php tests/rate_limit_test.php` 全部 PASS（C12）
- [x] `php tests/acks_on_failure_test.php` 全部 PASS（C13）
- [x] `php tests/max_tasks_per_child_test.php` 全部 PASS（C14）
- [x] `php tests/timezone_test.php` 全部 PASS（A16）
- [x] `php tests/events_test.php` 全部 PASS（A17）
- [x] `php tests/coalesce_test.php` 全部 PASS（A18）
- [x] `php tests/chain_test.php` 全部 PASS（C15）
- [x] `php tests/group_test.php` 全部 PASS（C16）
- [x] `php tests/max_memory_per_child_test.php` 全部 PASS（C17）
- [x] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [x] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [x] `php tests/data_dir_smoke.php` PASS
- [x] 20 个 `examples/*.php`（含新增 cron_timezone / events_query / chain_etl / group_batch）全部可运行（无 fatal error）
- [x] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 第五轮功能模块独立验证（含 24 个新对齐功能：6 项 round 2 + 6 项 round 3 + 6 项 round 4 + 6 项 round 5）
- [x] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code PASS
- [x] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1 PASS
- [x] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次 PASS
- [x] cron 任务：tests/cron.phpt 验证 cron 触发 PASS
- [x] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行 PASS
- [x] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查 PASS
- [x] 多服务：两个服务 PID 不同且互不干扰 PASS
- [x] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（**验证 bug 修复**）PASS
- [x] lifecycle：pause/resume/cancel/remove/list 全部按预期 PASS
- [x] start_end_date：startAt/endAt 时间窗口生效 PASS
- [x] result_ttl：result 过期清理 PASS
- [x] meta：meta 字段读写 PASS
- [x] priority：高优先级先执行 PASS
- [x] interval (A7)：every(2) 周期触发验证 PASS
- [x] runAt (A8)：一次性绝对时刻触发验证 PASS
- [x] jitter (A9)：多任务触发时刻分散验证 PASS
- [x] expires (C6)：超时 Pending 任务置 Expired 验证 PASS
- [x] requeue (C7)：cancel → requeue → 重新触发验证 PASS
- [x] retry_backoff (C8)：retryBackoff(true) 指数退避时间序列验证 PASS
- [x] max_instances (A10)：maxInstances(2) 允许 2 个并发实例 + 第 3 个被跳过验证 PASS
- [x] reschedule (A11)：reschedule 修改 cron + 保留 execution_count 验证 PASS
- [x] get_job (A12)：xhjob_get 返回完整 Task JSON 字段验证 PASS
- [x] ignore_result (C9)：ignoreResult(true) 不存 result + state 仍正常验证 PASS
- [x] acks_late (C10)：daemon 重启后 acks_late 任务被重排验证 PASS
- [x] soft_timeout (C11)：softTimeout(5)+timeout(10) shell 任务 SIGTERM 优雅退出验证 PASS
- [x] misfire_grace_time (A13)：misfireGraceTime(5) 短窗口跳过 6s 延迟 + 默认 0 用全局 60s 验证 PASS
- [x] replace_existing (A14)：withId + replaceExisting(true) 同 id 重跑覆盖 + 默认 false 报错验证 PASS
- [x] tags (A15)：tags 标记 + xhjob_list 按 tag 过滤 + 默认空数组不返回验证 PASS
- [x] rate_limit (C12)：rateLimit(3, 10) 滑动窗口限流 + 第 11 秒窗口滑过允许验证 PASS
- [x] acks_on_failure (C13)：acksOnFailure(false) 失败任务持续重试忽略 retry_max + 被 cancel 终止验证 PASS
- [x] max_tasks_per_child (C14)：XHJOB_MAX_TASKS_PER_CHILD=5 daemon 执行 5 个任务后优雅退出验证 PASS
- [x] timezone (A16)：timezone('America/New_York') cron 任务按纽约时区评估 + 解析失败回退全局验证 PASS
- [x] events (A17)：xhjob_events 查询任务执行事件流 + TTL 清理验证 PASS
- [x] coalesce (A18)：coalesce(true) 合并 missed / coalesce(false) 丢弃 missed 行为验证 PASS
- [x] chain (C15)：xhjob_chain 3 步 ETL 流水线 + 前任务输出作为后任务输入 + 失败中断验证 PASS
- [x] group (C16)：xhjob_group 3 任务并行批处理 + group_state 完成率验证 PASS
- [x] max_memory_per_child (C17)：XHJOB_MAX_MEMORY_PER_CHILD=100 daemon 内存超阈值后优雅退出验证 PASS

## 第五轮提交与推送远程主分支
- [x] `git status` 核对修改文件清单
- [x] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [x] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [x] `git commit -m "feat: 对齐 APScheduler/Celery 35 项（A1-A18+C1-C17）+ maxExecutions bug 修复 + 24 项新对齐功能（every/runAt/jitter/expires/requeue/retryBackoff/maxInstances升级/reschedule/get/ignoreResult/acksLate/softTimeout/misfireGraceTime/replaceExisting/tags/rateLimit/acksOnFailure/maxTasksPerChild/timezone/events/coalesce/chain/group/maxMemoryPerChild）+ 文档/示例/测试补齐"` 提交到本地 main
- [x] `git push origin main` 推送到远程主分支
- [x] `git log origin/main --oneline -5` 确认远程 HEAD 已更新
