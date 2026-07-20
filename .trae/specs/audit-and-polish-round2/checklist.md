# Checklist

## 起点（拉取与核对）
- [ ] `git fetch origin` 成功，无网络错误
- [ ] 本地当前分支为 `main`
- [ ] 本地 `main` 分支与 `origin/main` 同步（`git status` 显示 up to date）
- [ ] 工作树 clean，无未提交改动（不回滚任何用户改动）
- [ ] 最近 5 个 commit 符合预期（含上一轮 9 问题修复 commit `698f794`）

## 编译基线（确认起点干净）
- [ ] `cargo build --release --features persist` 退出码 0 且无 warning
- [ ] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [ ] `cargo test --release --lib --features persist` 全部通过
- [ ] `cargo test --release --lib`（默认 feature）全部通过

## 扩展部署
- [ ] `libxhjob.so` 已复制到 `php-config --extension-dir`
- [ ] `php -d extension=xhjob.so -m` 输出包含 `xhjob`

## Dead code 评估与启用（不删除有未来价值的代码）
### 启用 is_retryable_* 让 retry 真实按错误类型判断
- [ ] `src/retry/mod.rs::RetryPolicy::should_retry` 签名已改为 `should_retry(&self, task: &Task, result: &TaskResult) -> bool`
- [ ] `should_retry` 中按 task.task_type 真实调用 `is_retryable_http_status(status_code)` / `is_retryable_shell_exit(exit_code)`
- [ ] HTTP 5xx + 网络错误重试；HTTP 4xx 不重试直接 FAILED；shell 非零 exit 仍重试（保持当前行为）
- [ ] `src/scheduler/queue.rs::process_one` 中 `schedule_retry` 调用已传入 `&TaskResult` 而非 `String` 错误

### 删除真正无用的 dead code
- [ ] `src/retry/mod.rs::sleep_for_retry` 已删除（retry 通过 next_fire 机制实现延迟，无启用路径）

### 保留未来扩展接口并加注释
- [ ] `src/ipc/mod.rs::Event` 加 `#[allow(dead_code)]` + doc comment 说明"未来 IPC 事件流接口"
- [ ] `src/scheduler/overlap.rs::should_fire_missed` 加 `#[allow(dead_code)]` + doc comment 说明"未来 cron misfire 控制接口"
- [ ] `src/store/mod.rs::make_store` / `default_db_path` 加 `#[allow(dead_code)]` + doc comment 说明"store 工厂，daemon_main 直接构造未用，保留供未来依赖注入"
- [ ] `src/pool/thread_pool.rs` 文件头加 doc comment 说明"未来 CPU 密集任务接口"
- [ ] `src/pool/thread_pool.rs::ThreadPool` 与 `global` 加 `#[allow(dead_code)]`
- [ ] `src/pool/mod.rs::pub mod thread_pool;` 保留（不删除）
- [ ] `Cargo.toml` 中 `crossbeam-channel` 依赖保留（thread_pool 模块仍引用）
- [ ] 保留后编译零警告（两种 feature 均通过）

## next_fire 签名简化（删除被忽略的 seconds 参数）
- [ ] `src/scheduler/cron.rs` 的 `next_fire` 函数签名已删除 `seconds: bool` 参数
- [ ] `next_fire` 函数体中 `let _ = seconds;` 已删除
- [ ] `scan_once` 中对 `next_fire` 的调用（两处）已更新为新签名
- [ ] `src/task/mod.rs` 中 `TaskBuilder::build` 对 `next_fire` 的调用已更新
- [ ] `src/scheduler/cron.rs` 单元测试中对 `next_fire` 的调用（约 6 处）已更新
- [ ] 仓库内全文搜索 `next_fire(` 无残留旧签名调用

## cron 表达式非法时 dispatch 立即失败
- [ ] `src/task/mod.rs::TaskBuilder::build` 中 cron 解析失败时返回 `Err(XhjobError::CronParse(...))` 而非 `tracing::warn!` 后继续
- [ ] `dispatch()` 失败时返回 `error: cron parse: ...` 字符串（PHP 端验证）
- [ ] 任务不入队（store 中无此 task_id）

## 新功能 — cron 执行次数限制 maxExecutions(N)
### Task 数据结构
- [ ] `src/store/mod.rs::Task` struct 新增 `max_executions: u32`（默认 0=无限）字段，加 `#[serde(default)]`
- [ ] `src/store/mod.rs::Task` struct 新增 `execution_count: u32`（默认 0）字段，加 `#[serde(default)]`
- [ ] `src/store/mod.rs::Task::new` 初始化两个字段为 0

### TaskBuilder
- [ ] `src/task/mod.rs::TaskBuilder` 新增 `max_executions: u32` 字段
- [ ] `src/task/mod.rs::TaskBuilder` 新增 `max_executions(n: u32)` builder 方法（链式返回 `&mut Self`）
- [ ] `src/task/mod.rs::TaskBuilder::build` 中将 `max_executions` 复制到 Task

### PHP 类与函数
- [ ] `src/lib.rs::Xhjob` 类新增 `max_executions(&mut self, n: i64) -> &mut Self` 方法
- [ ] PHP 端方法名为 `maxExecutions(int $n): $this`
- [ ] `src/lib.rs::xhjob_dispatch` 透传 `max_executions` 字段到 task JSON
- [ ] `src/lib.rs::xhjob_state` 返回中新增 `execution_count` 与 `max_executions` 字段（向后兼容追加）
- [ ] `src/outcome/mod.rs::StateInfo` 新增 `execution_count: u32` 与 `max_executions: u32` 字段

### Cron 调度逻辑
- [ ] `src/scheduler/cron.rs::scan_once` 中：若 `task.max_executions > 0 && task.execution_count >= task.max_executions`，跳过触发并 continue
- [ ] `src/scheduler/queue.rs::process_one` 中：任务执行成功后递增 `execution_count`
- [ ] 若递增后 `execution_count >= max_executions`，将 state 置为 `Success`

### Store trait 与实现
- [ ] `src/store/mod.rs::TaskStore` trait 新增 `increment_execution_count(&self, id: &str) -> Result<u32>`（返回递增后的值）
- [ ] `src/store/in_memory.rs` 实现新方法
- [ ] `src/store/sqlite.rs` schema 新增 `max_executions INTEGER NOT NULL DEFAULT 0` 与 `execution_count INTEGER NOT NULL DEFAULT 0` 两列
- [ ] `src/store/sqlite.rs` 旧库通过 PRAGMA 检查列存在性后执行 `ALTER TABLE tasks ADD COLUMN ...`（兼容旧库）
- [ ] `src/store/sqlite.rs::task_from_row` / `insert_task` / `update_state` 同步读写新字段

### 行为验证
- [ ] `maxExecutions(3)` 后 cron 触发 3 次后 state=SUCCESS，execution_count=3，第 4 次 tick 不再触发
- [ ] 不设置 maxExecutions（默认 0）时 cron 持续触发（向后兼容）
- [ ] `maxExecutions(0)` 显式无限，等同未设置
- [ ] 持久化场景 restart 后 execution_count 保留

## README 文档补齐
- [ ] "API 参考" 表 `Xhjob` 类方法表新增 `maxExecutions(int $n): $this` 行
- [ ] "API 参考" 表 `xhjob_state` 返回说明新增 `execution_count` / `max_executions` 字段
- [ ] "API 参考" 表 `cron(string $expr)` 行补充"5 或 6 段（6 段含秒）"
- [ ] "环境变量" 表 `XHJOB_PERSIST` 行明确"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
- [ ] "Cron 自定义时区" 一节明确"5 段 = `min hour day month weekday`；6 段 = `sec min hour day month weekday`（含秒）"
- [ ] 新增"Cron 执行次数限制"小节，说明 `maxExecutions(N)` 用法、默认 0=无限、到达上限后 state=SUCCESS

## examples 完善
- [ ] `examples/cron_http.php` dispatch 后增加 echo 提示"脚本退出后 daemon 仍持续触发 cron，需运行 `xhjob_stop()` 才能停止"
- [ ] `examples/cron_http.php` 增加一个 `maxExecutions(3)` 示例任务，说明执行 3 次后自动停止
- [ ] 其余 examples 已检查无类似 daemon 持久化误导，按需补注释

## 边界场景测试（tests/boundary_cases.php）
- [ ] `tests/boundary_cases.php` 已创建
- [ ] 测试 `xhjob_dispatch("{not json", "default")` 返回 `error: invalid json` 字符串 PASS
- [ ] 测试 `xhjob_state("any-id", "1invalid")` 返回 `state=UNKNOWN` + `error` 数组（非 fatal）PASS
- [ ] 测试 `xhjob_result("any-id", "1invalid")` 返回 `error` 数组（非 fatal）PASS
- [ ] 测试 daemon 未启动时 `xhjob_dispatch` 返回 `error:` 前缀字符串 PASS
- [ ] 测试非法 cron 表达式 `Xhjob::task()->viaShell('echo hi')->cron('not a cron')->dispatch()` 返回 `error:` 前缀 PASS（验证 cron 非法立即失败修复）
- [ ] 测试 HTTP 4xx 不重试：dispatch 一个 404 URL + withRetry(3)，等待终态验证 attempts=1（验证 should_retry 修复；无网络则 SKIP）
- [ ] 脚本顶部用 `check()` 函数逐项断言，输出 PASS/FAIL 汇总
- [ ] 脚本 `exit($fail > 0 ? 1 : 0)`

## maxExecutions 测试（tests/max_executions.php）
- [ ] `tests/max_executions.php` 已创建
- [ ] 启动 daemon，dispatch 一个 `cron('*/1 * * * * *')` + `maxExecutions(3)` + `viaShell('echo hi')` 任务
- [ ] 轮询 `xhjob_state`，等待 execution_count 达到 3 且 state=SUCCESS
- [ ] 验证第 4 秒后 execution_count 仍为 3（不再触发）
- [ ] 测试 `maxExecutions(0)` 显式无限（投递后等待 2 次触发验证 execution_count=2）
- [ ] 脚本输出 PASS/FAIL 汇总

## 修复后编译与全量回归测试
- [ ] `cargo build --release --features persist` 退出码 0 且无 warning
- [ ] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [ ] `cargo test --release --lib --features persist` 全部通过（含新增 retry 按错误类型测试 + next_fire 新签名测试）
- [ ] `cargo test --release --lib`（默认 feature）全部通过
- [ ] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [ ] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [ ] `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
- [ ] `php tests/max_executions.php` 全部 PASS
- [ ] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [ ] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [ ] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [ ] `php tests/data_dir_smoke.php` PASS
- [ ] 7 个 `examples/*.php` 全部可运行（无 fatal error），cron_http.php 验证 maxExecutions 示例
- [ ] `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

## 功能模块独立验证（用代码验证执行结果正确性）
- [ ] shell 任务：dispatch echo 命令，验证 stdout 与 exit_code（PASS）
- [ ] retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1（验证新 should_retry）
- [ ] retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次（PASS）
- [ ] cron 任务：tests/cron.phpt 验证 cron 触发（PASS）
- [ ] overlap 任务：慢任务 + allowOverlap(false)，验证排队执行（PASS）
- [ ] persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查（PASS）
- [ ] 多服务：两个服务 PID 不同且互不干扰（PASS）
- [ ] maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（PASS）

## 提交与推送
- [ ] `git status` 核对修改文件清单
- [ ] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [ ] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [ ] `git commit -m "feat: 新增 maxExecutions + retry 按错误类型判断 + cron 非法表达式立即失败 + 文档/示例/测试补齐"` 提交到本地 main
- [ ] `git push origin main` 推送到远程主分支
- [ ] `git log origin/main --oneline -5` 确认远程 HEAD 已更新
