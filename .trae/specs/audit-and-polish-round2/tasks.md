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

- [ ] Task 3: 部署扩展到 PHP extensions 目录（确保 PHP 端测试可跑）
  - [ ] SubTask 3.1: `cp target/release/libxhjob.so $(php-config --extension-dir)/xhjob.so`
  - [ ] SubTask 3.2: `php -d extension=xhjob.so -m | grep xhjob` 确认加载

- [ ] Task 4: 删除确认未使用的 dead code
  - [ ] SubTask 4.1: 删除 `src/ipc/mod.rs` 的 `Event` 结构体（未被任何模块引用）
  - [ ] SubTask 4.2: 删除 `src/scheduler/overlap.rs` 的 `should_fire_missed` 函数（未被任何调用方调用）
  - [ ] SubTask 4.3: 删除 `src/store/mod.rs` 的 `make_store` 与 `default_db_path` 函数（`make_store` 未被调用，`default_db_path` 只被 `make_store` 调用）
  - [ ] SubTask 4.4: 删除 `src/retry/mod.rs` 的 `is_retryable_http_status` / `is_retryable_shell_exit` / `sleep_for_retry`（均未被调用）
  - [ ] SubTask 4.5: 删除 `src/pool/thread_pool.rs` 文件，并在 `src/pool/mod.rs` 中移除 `pub mod thread_pool;`（已确认任务派发路径使用 tokio coroutine pool，thread_pool 模块从未被任务派发调用）
  - [ ] SubTask 4.6: 检查 `Cargo.toml` 中 `crossbeam-channel` 依赖是否仅被 thread_pool 使用；若是，移除该依赖

- [ ] Task 5: 简化 `next_fire` 签名（删除被忽略的 `seconds` 参数）
  - [ ] SubTask 5.1: 修改 `src/scheduler/cron.rs` 的 `next_fire` 函数签名，删除 `seconds: bool` 参数
  - [ ] SubTask 5.2: 同步修改 `src/scheduler/cron.rs` 内 `scan_once` 中对 `next_fire` 的调用
  - [ ] SubTask 5.3: 同步修改 `src/task/mod.rs` 中 `TaskBuilder::build` 对 `next_fire` 的调用
  - [ ] SubTask 5.4: 检查 `src/daemon_main.rs` 与其他文件是否还有 `next_fire` 调用，同步更新
  - [ ] SubTask 5.5: 更新 `src/scheduler/cron.rs` 的单元测试中对 `next_fire` 的调用（删除 `false` 参数）

- [ ] Task 6: README 关键说明补齐
  - [ ] SubTask 6.1: 在 README "环境变量" 表的 `XHJOB_PERSIST` 行明确标注"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
  - [ ] SubTask 6.2: 在 README "环境变量" 表的 `XHJOB_THREAD_POOL_SIZE` 行删除或注明该变量当前未被使用（thread_pool 模块已删除，仅 coroutine pool 生效）
  - [ ] SubTask 6.3: 在 README "Cron 自定义时区" 一节明确"5 段 = `min hour day month weekday`；6 段 = `sec min hour day month weekday`（含秒）"
  - [ ] SubTask 6.4: 在 README "API 参考" 表的 `cron(string $expr)` 行补充"5 或 6 段表达式（6 段含秒）"

- [ ] Task 7: examples 注释补齐
  - [ ] SubTask 7.1: `examples/cron_http.php` 在 dispatch 后增加 echo 提示"脚本退出后 daemon 仍持续触发 cron，需运行 `xhjob_stop()` 才能停止"
  - [ ] SubTask 7.2: 检查其余 examples 是否有类似 daemon 持久化的误导，按需补注释

- [ ] Task 8: 新增边界场景测试脚本 `tests/boundary_cases.php`
  - [ ] SubTask 8.1: 测试 `xhjob_dispatch("{not json", "default")` 返回 `error: invalid json` 字符串
  - [ ] SubTask 8.2: 测试 `xhjob_state("any-id", "1invalid")` 返回包含 `state=UNKNOWN` + `error` 的数组（非 PHP fatal）
  - [ ] SubTask 8.3: 测试 `xhjob_result("any-id", "1invalid")` 返回包含 `error` 的数组（非 PHP fatal）
  - [ ] SubTask 8.4: 测试 `xhjob_dispatch` 在 daemon 未启动时返回 `error:` 前缀字符串（先 stop 再 dispatch）
  - [ ] SubTask 8.5: 测试非法 cron 表达式 `xhjob_dispatch('{"task_type":"shell","payload":{"cmd":"echo hi"},"cron":"not a cron"}')` 返回 `error:` 前缀
  - [ ] SubTask 8.6: 在脚本顶部用 `check()` 函数逐项断言，输出 PASS/FAIL 汇总

- [ ] Task 9: 修复后重新编译 + 重跑全量测试（验证无回归）
  - [ ] SubTask 9.1: `cargo build --release --features persist` 退出码 0 且无 warning
  - [ ] SubTask 9.2: `cargo build --release`（默认 feature）退出码 0 且无 warning
  - [ ] SubTask 9.3: `cargo test --release --lib --features persist` 全部通过
  - [ ] SubTask 9.4: `cargo test --release --lib`（默认 feature）全部通过
  - [ ] SubTask 9.5: 重新部署 `libxhjob.so` 到 `php-config --extension-dir`
  - [ ] SubTask 9.6: `php run-tests.php tests/` 全量 .phpt 仍 7 PASS / 3 SKIP / 0 FAIL
  - [ ] SubTask 9.7: `php tests/boundary_cases.php` 全部 PASS
  - [ ] SubTask 9.8: `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
  - [ ] SubTask 9.9: `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
  - [ ] SubTask 9.10: `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
  - [ ] SubTask 9.11: `php tests/data_dir_smoke.php` PASS
  - [ ] SubTask 9.12: 7 个 `examples/*.php` 全部可运行（无 fatal error）

- [ ] Task 10: 单独执行功能模块验证（用代码验证执行结果正确性）
  - [ ] SubTask 10.1: shell 任务：dispatch echo 命令，验证 stdout 与 exit_code（PASS）
  - [ ] SubTask 10.2: retry 任务：dispatch 必失败命令 + withRetry(3)，验证重试后 FAILED，attempts>=3（PASS）
  - [ ] SubTask 10.3: cron 任务：tests/cron.phpt 验证 cron 触发（PASS）
  - [ ] SubTask 10.4: overlap 任务：慢任务 + allowOverlap(false)，验证排队执行（PASS）
  - [ ] SubTask 10.5: persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查（PASS）
  - [ ] SubTask 10.6: 多服务：两个服务 PID 不同且互不干扰（PASS）

- [ ] Task 11: 提交并推送远程主分支
  - [ ] SubTask 11.1: `git status` 核对修改文件清单
  - [ ] SubTask 11.2: `git diff` 审查改动内容
  - [ ] SubTask 11.3: `git add <指定文件>` 暂存改动
  - [ ] SubTask 11.4: `git commit -m "..."` 提交到本地 main
  - [ ] SubTask 11.5: `git push origin main` 推送到远程主分支
  - [ ] SubTask 11.6: `git log origin/main --oneline -5` 确认远程 HEAD 已更新

# Task Dependencies
- Task 1 独立，最先执行（确认起点干净）
- Task 2 依赖 Task 1（确认起点后编译基线）
- Task 3 依赖 Task 2（编译产物部署）
- Task 4、Task 5 可并行（均依赖 Task 2 的编译基线，互不冲突）
- Task 6、Task 7 可并行（文档与示例注释，不依赖代码改动）
- Task 8 依赖 Task 5（边界测试中包含 cron 非法表达式，需 next_fire 已简化）
- Task 9 依赖 Task 4、5、6、7、8（所有改动完成后重新编译测试）
- Task 10 依赖 Task 9（编译产物部署后再验证功能模块）
- Task 11 依赖 Task 9、10（无回归后提交推送）
