# Tasks

- [ ] Task 1: 起点（拉取主分支并核对工作树）
  - [ ] SubTask 1.1: `git fetch origin && git status`，确认本地 `main` 与 `origin/main` 同步
  - [ ] SubTask 1.2: `git checkout main`（如不在 main），确保站在主分支
  - [ ] SubTask 1.3: `git log --oneline -5` 核对最近提交，确认未回滚任何用户改动

- [ ] Task 2: 编译基线（确认起点干净）
  - [ ] SubTask 2.1: `cargo build --release --features persist` 退出码 0 且无 warning
  - [ ] SubTask 2.2: `cargo build --release`（默认 feature）退出码 0 且无 warning
  - [ ] SubTask 2.3: `cargo test --release --lib --features persist` 全部通过
  - [ ] SubTask 2.4: `cargo test --release --lib`（默认 feature）全部通过

- [ ] Task 3: 部署扩展到 PHP extensions 目录
  - [ ] SubTask 3.1: `cp target/release/libxhjob.so $(php-config --extension-dir)/xhjob.so`
  - [ ] SubTask 3.2: `php -d extension=xhjob.so -m | grep xhjob` 确认加载

- [ ] Task 4: 评估并启用 dead code 真实功能（不删除有未来价值的代码）
  - [ ] SubTask 4.1: 启用 `is_retryable_http_status` / `is_retryable_shell_exit`：修改 `RetryPolicy::should_retry` 签名为 `should_retry(&self, task: &Task, result: &TaskResult) -> bool`，按 task_type 真实调用判断函数
  - [ ] SubTask 4.2: 同步修改 `src/scheduler/queue.rs::process_one` 中调用 `schedule_retry` 的位置，传入 `TaskResult` 而非 `String` 错误
  - [ ] SubTask 4.3: 评估 `sleep_for_retry`（无启用路径）→ 删除
  - [ ] SubTask 4.4: 评估 `Event` 结构体（未来 IPC 事件流接口）→ 加 `#[allow(dead_code)]` + doc comment 说明保留原因
  - [ ] SubTask 4.5: 评估 `should_fire_missed`（未来 cron misfire 控制）→ 加 `#[allow(dead_code)]` + doc comment 说明保留原因
  - [ ] SubTask 4.6: 评估 `make_store` / `default_db_path`（store 工厂，daemon_main 直接构造未用）→ 加 `#[allow(dead_code)]` + doc comment 说明保留原因
  - [ ] SubTask 4.7: 评估 `thread_pool` 模块（未来 CPU 密集任务）→ 加 `#[allow(dead_code)]` 在 `pool::thread_pool::ThreadPool` 与 `global`，并在文件头加 doc comment 说明保留原因

- [ ] Task 5: 简化 `next_fire` 签名（删除被忽略的 `seconds` 参数）
  - [ ] SubTask 5.1: 修改 `src/scheduler/cron.rs` 的 `next_fire` 函数签名，删除 `seconds: bool` 参数与 `let _ = seconds;`
  - [ ] SubTask 5.2: 同步修改 `src/scheduler/cron.rs::scan_once` 中两处 `next_fire` 调用
  - [ ] SubTask 5.3: 同步修改 `src/task/mod.rs::TaskBuilder::build` 中 `next_fire` 调用
  - [ ] SubTask 5.4: 同步修改 `src/scheduler/cron.rs` 单元测试中所有 `next_fire` 调用（约 6 处，删除 `false` 参数）

- [ ] Task 6: cron 表达式非法时 dispatch 立即失败
  - [ ] SubTask 6.1: 修改 `src/task/mod.rs::TaskBuilder::build`：cron 表达式解析失败时返回 `Err(XhjobError::CronParse(...))` 而非 `tracing::warn!` 后继续
  - [ ] SubTask 6.2: 验证 `dispatch()` 失败时返回 `error: cron parse: ...` 字符串（PHP 端）

- [ ] Task 7: 新功能 — cron 执行次数限制 `maxExecutions(N)`
  - [ ] SubTask 7.1: `src/store/mod.rs` 的 `Task` struct 新增 `max_executions: u32`（默认 0=无限）与 `execution_count: u32`（默认 0）字段，加 `#[serde(default)]`
  - [ ] SubTask 7.2: `src/store/mod.rs` 的 `Task::new` 初始化新字段为 0
  - [ ] SubTask 7.3: `src/task/mod.rs` 的 `TaskBuilder` 新增 `max_executions: u32` 字段 + `max_executions(n)` builder 方法；`build()` 中将值复制到 Task
  - [ ] SubTask 7.4: `src/lib.rs` 的 `Xhjob` 类新增 `max_executions(&mut self, n: i64) -> &mut Self` 方法，PHP 暴露为 `maxExecutions(int $n): $this`
  - [ ] SubTask 7.5: `src/scheduler/cron.rs::scan_once` 中：若 `task.max_executions > 0 && task.execution_count >= task.max_executions`，跳过触发并 continue
  - [ ] SubTask 7.6: `src/scheduler/queue.rs::process_one` 中：任务执行成功后递增 `execution_count`（新增 store trait 方法 `increment_execution_count(id)` 或扩展 `update_state`）；若递增后 `execution_count >= max_executions`，将 state 置为 `Success`
  - [ ] SubTask 7.7: `src/store/mod.rs::TaskStore` trait 新增 `increment_execution_count(&self, id: &str) -> Result<u32>`（返回递增后的值）
  - [ ] SubTask 7.8: `src/store/in_memory.rs` 实现新方法
  - [ ] SubTask 7.9: `src/store/sqlite.rs` 实现：schema 新增 `max_executions INTEGER NOT NULL DEFAULT 0` 与 `execution_count INTEGER NOT NULL DEFAULT 0` 两列；旧库执行 `ALTER TABLE tasks ADD COLUMN max_executions INTEGER NOT NULL DEFAULT 0` 与 `ALTER TABLE tasks ADD COLUMN execution_count INTEGER NOT NULL DEFAULT 0`（用 PRAGMA 检查列是否存在以兼容旧库）；`task_from_row` / `insert_task` / `update_state` 同步读写
  - [ ] SubTask 7.10: `src/lib.rs` 的 `xhjob_state` 返回中新增 `execution_count` 与 `max_executions` 字段（向后兼容追加）
  - [ ] SubTask 7.11: `src/outcome/mod.rs::StateInfo` 新增 `execution_count: u32` 与 `max_executions: u32` 字段

- [ ] Task 8: README 文档补齐
  - [ ] SubTask 8.1: "API 参考" 表 `Xhjob` 类方法表新增 `maxExecutions(int $n): $this` 行
  - [ ] SubTask 8.2: "API 参考" 表 `xhjob_state` 返回说明新增 `execution_count` / `max_executions` 字段
  - [ ] SubTask 8.3: "环境变量" 表 `XHJOB_PERSIST` 行明确"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
  - [ ] SubTask 8.4: "Cron 自定义时区" 一节明确"5 段 = `min hour day month weekday`；6 段 = `sec min hour day month weekday`（含秒）"
  - [ ] SubTask 8.5: "API 参考" 表 `cron(string $expr)` 行补充"5 或 6 段（6 段含秒）"
  - [ ] SubTask 8.6: 新增"Cron 执行次数限制"小节，说明 `maxExecutions(N)` 用法、默认 0=无限、到达上限后 state=SUCCESS

- [ ] Task 9: examples 完善
  - [ ] SubTask 9.1: `examples/cron_http.php` dispatch 后增加 echo 提示"脚本退出后 daemon 仍持续触发 cron，需运行 `xhjob_stop()` 才能停止"
  - [ ] SubTask 9.2: `examples/cron_http.php` 增加一个 `maxExecutions(3)` 示例任务，说明执行 3 次后自动停止
  - [ ] SubTask 9.3: 检查其余 examples 是否有类似 daemon 持久化误导，按需补注释

- [ ] Task 10: 新增 `tests/boundary_cases.php` 边界场景测试
  - [ ] SubTask 10.1: 测试 `xhjob_dispatch("{not json", "default")` 返回 `error: invalid json` 字符串
  - [ ] SubTask 10.2: 测试 `xhjob_state("any-id", "1invalid")` 返回包含 `state=UNKNOWN` + `error` 的数组（非 PHP fatal）
  - [ ] SubTask 10.3: 测试 `xhjob_result("any-id", "1invalid")` 返回包含 `error` 的数组（非 PHP fatal）
  - [ ] SubTask 10.4: 测试 `xhjob_dispatch` 在 daemon 未启动时返回 `error:` 前缀字符串（先 stop 再 dispatch）
  - [ ] SubTask 10.5: 测试非法 cron 表达式 `Xhjob::task()->viaShell('echo hi')->cron('not a cron')->dispatch()` 返回 `error:` 前缀（验证 Task 6 修复）
  - [ ] SubTask 10.6: 测试 HTTP 4xx 不重试：dispatch 一个 404 URL，等待终态，验证 attempts=1（验证 Task 4 修复，需网络；无网络则 SKIP）
  - [ ] SubTask 10.7: 脚本顶部用 `check()` 函数逐项断言，输出 PASS/FAIL 汇总

- [ ] Task 11: 新增 `tests/max_executions.php` 执行次数限制测试
  - [ ] SubTask 11.1: 启动 daemon，dispatch 一个 `cron('*/1 * * * * *')` + `maxExecutions(3)` + `viaShell('echo hi')` 任务
  - [ ] SubTask 11.2: 轮询 `xhjob_state`，等待 execution_count 达到 3 且 state=SUCCESS
  - [ ] SubTask 11.3: 验证第 4 秒后 execution_count 仍为 3（不再触发）
  - [ ] SubTask 11.4: 测试 `maxExecutions(0)` 显式无限（投递后等待 2 次触发验证 execution_count=2）
  - [ ] SubTask 11.5: 脚本输出 PASS/FAIL 汇总

- [ ] Task 12: 重新编译 + 重跑全量测试（验证无回归）
  - [ ] SubTask 12.1: `cargo build --release --features persist` 退出码 0 且无 warning
  - [ ] SubTask 12.2: `cargo build --release`（默认 feature）退出码 0 且无 warning
  - [ ] SubTask 12.3: `cargo test --release --lib --features persist` 全部通过（新增的 retry 按错误类型测试 + next_fire 新签名测试）
  - [ ] SubTask 12.4: `cargo test --release --lib`（默认 feature）全部通过
  - [ ] SubTask 12.5: 重新部署 `libxhjob.so` 到 `php-config --extension-dir`
  - [ ] SubTask 12.6: `php run-tests.php tests/` 全量 .phpt 仍 7 PASS / 3 SKIP / 0 FAIL
  - [ ] SubTask 12.7: `php tests/boundary_cases.php` 全部 PASS（4xx 测试可 SKIP）
  - [ ] SubTask 12.8: `php tests/max_executions.php` 全部 PASS
  - [ ] SubTask 12.9: `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
  - [ ] SubTask 12.10: `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
  - [ ] SubTask 12.11: `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
  - [ ] SubTask 12.12: `php tests/data_dir_smoke.php` PASS
  - [ ] SubTask 12.13: 7 个 `examples/*.php` 全部可运行（无 fatal error），cron_http.php 验证 maxExecutions 示例
  - [ ] SubTask 12.14: `php tests/functional_verify.php` 15 步全部 PASS（验证既有功能未回归）

- [ ] Task 13: 单独执行功能模块验证（用代码验证执行结果正确性）
  - [ ] SubTask 13.1: shell 任务：dispatch echo 命令，验证 stdout 与 exit_code（PASS）
  - [ ] SubTask 13.2: retry 任务：dispatch HTTP 404 + withRetry(3)，验证不重试 attempts=1（验证新 should_retry）
  - [ ] SubTask 13.3: retry 任务：dispatch shell 失败 + withRetry(3)，验证重试至多 3 次（PASS）
  - [ ] SubTask 13.4: cron 任务：tests/cron.phpt 验证 cron 触发（PASS）
  - [ ] SubTask 13.5: overlap 任务：慢任务 + allowOverlap(false)，验证排队执行（PASS）
  - [ ] SubTask 13.6: persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查（PASS）
  - [ ] SubTask 13.7: 多服务：两个服务 PID 不同且互不干扰（PASS）
  - [ ] SubTask 13.8: maxExecutions：cron + maxExecutions(3)，验证执行 3 次后停止（PASS）

- [ ] Task 14: 提交并推送远程主分支
  - [ ] SubTask 14.1: `git status` 核对修改文件清单
  - [ ] SubTask 14.2: `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
  - [ ] SubTask 14.3: `git add <指定文件>` 暂存改动（不 `git add -A`）
  - [ ] SubTask 14.4: `git commit -m "feat: 新增 maxExecutions + retry 按错误类型判断 + cron 非法表达式立即失败 + 文档/示例/测试补齐"` 提交到本地 main
  - [ ] SubTask 14.5: `git push origin main` 推送到远程主分支
  - [ ] SubTask 14.6: `git log origin/main --oneline -5` 确认远程 HEAD 已更新

# Task Dependencies
- Task 1 独立，最先执行（确认起点干净）
- Task 2 依赖 Task 1（确认起点后编译基线）
- Task 3 依赖 Task 2（编译产物部署）
- Task 4、Task 5、Task 6 可并行（均依赖 Task 2 的编译基线，互不冲突）
- Task 7 依赖 Task 5（next_fire 简化后再叠加 max_executions 逻辑）
- Task 8、Task 9 可并行（文档与示例，不依赖代码改动）
- Task 10 依赖 Task 4、6（边界测试包含非法 cron + 4xx 不重试）
- Task 11 依赖 Task 7（maxExecutions 功能完成后测试）
- Task 12 依赖 Task 4-11（所有改动完成后重新编译测试）
- Task 13 依赖 Task 12（编译产物部署后再验证功能模块）
- Task 14 依赖 Task 12、13（无回归后提交推送）
