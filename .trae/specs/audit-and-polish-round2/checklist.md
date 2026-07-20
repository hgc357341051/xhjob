# Checklist

## 起点（拉取与核对）
- [ ] `git fetch origin` 成功，无网络错误
- [ ] 本地当前分支为 `main`
- [ ] 本地 `main` 分支与 `origin/main` 同步（`git status` 显示 up to date）
- [ ] 工作树 clean，无未提交改动（不回滚任何用户改动）
- [ ] 最近 5 个 commit 符合预期（含上一轮 9 问题修复 commit `698f794`）

## 编译基线
- [ ] `cargo build --release --features persist` 退出码 0 且无 warning
- [ ] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [ ] `cargo test --release --lib --features persist` 全部通过
- [ ] `cargo test --release --lib`（默认 feature）全部通过

## 扩展部署
- [ ] `libxhjob.so` 已复制到 `php-config --extension-dir`
- [ ] `php -d extension=xhjob.so -m` 输出包含 `xhjob`

## Dead code 清理
- [ ] `src/ipc/mod.rs` 的 `Event` 结构体已删除
- [ ] `src/scheduler/overlap.rs` 的 `should_fire_missed` 函数已删除
- [ ] `src/store/mod.rs` 的 `make_store` 与 `default_db_path` 函数已删除
- [ ] `src/retry/mod.rs` 的 `is_retryable_http_status` / `is_retryable_shell_exit` / `sleep_for_retry` 已删除
- [ ] `src/pool/thread_pool.rs` 文件已删除
- [ ] `src/pool/mod.rs` 的 `pub mod thread_pool;` 已删除
- [ ] `Cargo.toml` 中 `crossbeam-channel` 依赖如仅被 thread_pool 使用则已移除
- [ ] 删除后编译零警告（两种 feature 均通过）

## next_fire 签名简化
- [ ] `src/scheduler/cron.rs` 的 `next_fire` 函数签名已删除 `seconds: bool` 参数
- [ ] `next_fire` 函数体中 `let _ = seconds;` 已删除
- [ ] `scan_once` 中对 `next_fire` 的调用已更新为新签名
- [ ] `src/task/mod.rs` 中 `TaskBuilder::build` 对 `next_fire` 的调用已更新
- [ ] `src/scheduler/cron.rs` 单元测试中对 `next_fire` 的调用已更新
- [ ] 仓库内全文搜索 `next_fire(` 无残留旧签名调用

## README 关键说明
- [ ] "环境变量" 表 `XHJOB_PERSIST` 行已标注"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
- [ ] "环境变量" 表 `XHJOB_THREAD_POOL_SIZE` 行已注明当前未使用（或已删除该行）
- [ ] "Cron 自定义时区" 一节已明确 5 段 / 6 段表达式格式（6 段含秒）
- [ ] "API 参考" 表 `cron(string $expr)` 行已补充"5 或 6 段（6 段含秒）"

## examples 注释
- [ ] `examples/cron_http.php` dispatch 后已增加提示"脚本退出后 daemon 仍持续触发 cron，需 `xhjob_stop()` 才能停止"
- [ ] 其余 examples 已检查无类似误导

## 边界场景测试
- [ ] `tests/boundary_cases.php` 已创建
- [ ] 测试 `xhjob_dispatch("{not json", "default")` 返回 `error: invalid json` 字符串 PASS
- [ ] 测试 `xhjob_state("any-id", "1invalid")` 返回 `state=UNKNOWN` + `error` 数组（非 fatal）PASS
- [ ] 测试 `xhjob_result("any-id", "1invalid")` 返回 `error` 数组（非 fatal）PASS
- [ ] 测试 daemon 未启动时 `xhjob_dispatch` 返回 `error:` 前缀字符串 PASS
- [ ] 测试非法 cron 表达式的 dispatch 返回 `error:` 前缀字符串 PASS
- [ ] 脚本输出汇总（N passed, M failed）且 `exit($fail > 0 ? 1 : 0)`

## 修复后编译与回归测试
- [ ] `cargo build --release --features persist` 退出码 0 且无 warning
- [ ] `cargo build --release`（默认 feature）退出码 0 且无 warning
- [ ] `cargo test --release --lib --features persist` 全部通过
- [ ] `cargo test --release --lib`（默认 feature）全部通过
- [ ] `libxhjob.so` 已重新部署到 `php-config --extension-dir`
- [ ] `php run-tests.php tests/` 7 PASS / 3 SKIP / 0 FAIL（无回归）
- [ ] `php tests/boundary_cases.php` 全部 PASS
- [ ] `bash tests/business/cli_bus/run_all.sh` 4 步全部 PASS
- [ ] `php tests/business/fpm_sim/proc_test.php` 10 步全部 PASS
- [ ] `php tests/business/fpm_sim/client_test.php` 10 步全部 PASS
- [ ] `php tests/data_dir_smoke.php` PASS
- [ ] 7 个 `examples/*.php` 全部可运行（无 fatal error）

## 功能模块独立验证（用代码验证执行结果正确性）
- [ ] shell 任务：stdout 与 exit_code 正确
- [ ] retry 任务：重试后 FAILED，attempts>=3
- [ ] cron 任务：cron 触发（tests/cron.phpt PASS）
- [ ] overlap 任务：allowOverlap(false) 排队执行
- [ ] persist 任务：restart 后任务状态可查
- [ ] 多服务：两个服务 PID 不同，stop 一个不影响另一个

## 提交与推送
- [ ] `git status` 核对修改文件清单
- [ ] `git diff` 审查改动内容（确认无意外改动、无删除用户文件）
- [ ] `git add <指定文件>` 暂存改动（不 `git add -A`，避免误加 spec 文档外文件）
- [ ] `git commit -m "..."` 提交到本地 main
- [ ] `git push origin main` 推送到远程主分支
- [ ] `git log origin/main --oneline -5` 确认远程 HEAD 已更新
