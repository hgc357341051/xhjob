# Checklist — fix-daemon-log-dir-fallback

## daemon_main log_dir 解析修复
- [ ] `src/daemon_main.rs` `log_dir` 改为 `resolve_data_dir(current_data_dir().as_deref())`
- [ ] 移除 `/var/log/xhjob` 回退路径
- [ ] `create_dir_all` 失败时回退到 `std::env::temp_dir()` + eprintln 警告
- [ ] 提取为 `fn resolve_log_dir() -> String` 可测函数
- [ ] 更新注释（P0-15 段）说明新解析逻辑

## 单测
- [ ] `test_resolve_log_dir_uses_env_var_when_set`（设 XHJOB_DATA_DIR，断言返回该值）
- [ ] `test_resolve_log_dir_falls_back_to_tmp_when_unset`（清空 env var，断言返回 /tmp 或 temp_dir）

## 编译与单测
- [ ] `cargo build --features persist` 无 warning
- [ ] `cargo test --features persist` 100% 通过（200 + 2 新增）
- [ ] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
- [ ] `cargo build --release --features persist` 编译 .so
- [ ] 复制 .so 到 `releases/xhjob-php8.2-linux-x86_64.so`，记录 MD5

## E2E 验证（uid=1001 复现）
- [ ] 清理 /tmp/xhjob.default.* 残留
- [ ] 以 u1001 运行 `xhjob_start("default")`（不传 data_dir）返回 true
- [ ] `/tmp/xhjob.default.log.<date>` 存在且属主 u1001
- [ ] `/var/log/xhjob` 不存在
- [ ] 清理：无残留 daemon，删除测试文件

## Git 提交推送
- [ ] 新分支 `fix-daemon-log-dir-fallback` 基于当前 main HEAD
- [ ] `git add` 仅目标文件
- [ ] commit message 说明根因 + 修复
- [ ] `git push -u origin fix-daemon-log-dir-fallback` 成功
- [ ] 切到 main，`git merge --ff-only`，`git push origin main` 成功
