# Checklist — fix-fpm-binary-resolution-v2

## validate_php_binary 静态化
- [ ] `src/daemon/mod.rs` `validate_php_binary` 不再 spawn 子进程，改为 `is_file` + Unix `mode & 0o111 != 0`
- [ ] 旧 spawn-based 逻辑保留为 `validate_php_binary_spawn`（私有，单测用）
- [ ] docstring 说明改为静态检查的原因
- [ ] 单测：`test_validate_php_binary_returns_false_for_missing`（已有，仍通过）
- [ ] 单测：`test_validate_php_binary_returns_true_for_executable_file`（新建）
- [ ] 单测：`test_validate_php_binary_returns_false_for_non_executable`（新建）

## resolve_php_binary 候选扩展
- [ ] 新增 `<raw_dir>/../sbin/php` 候选
- [ ] 候选用 `Path::canonicalize` 去重（canonicalize 失败时 fallback 字符串比较）
- [ ] 返回值改为 `(PathBuf, PathBuf, Vec<PhpBinaryCandidate>)`
- [ ] 所有调用点（unix.rs/windows.rs/check_data_dir_writable/lib.rs）同步更新
- [ ] 所有候选失败时 `tracing::warn!` 输出完整候选列表 + 失败原因
- [ ] 单测：`test_resolve_php_binary_records_candidates`

## xhjob_diag 新字段
- [ ] `php_binary_candidates` 字段（JSON 数组，每元素含 path/valid/reason）
- [ ] `xhjob_so_info` 字段（JSON 对象，含 path/size_bytes/mtime_epoch 或 path:null+error）
- [ ] `resolve_xhjob_so_path` 扫描 `/proc/self/maps` 找 xhjob.so
- [ ] docstring 列出两个新字段
- [ ] 单测：`test_diag_includes_php_binary_candidates`
- [ ] 单测：`test_diag_includes_xhjob_so_info`
- [ ] 单测：`test_resolve_xhjob_so_path_returns_option_no_panic`

## 编译与单测
- [ ] `cargo build --release --features persist` 无 warning
- [ ] `cargo test --features persist` 100% 通过（前序 193 + 新增约 6 个）
- [ ] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
- [ ] `target/release/libxhjob.so` 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- [ ] MD5 一致性校验通过

## 端到端验证
- [ ] CLI 调用 `xhjob_diag` 返回 JSON 含 `php_binary_candidates`（数组）+ `xhjob_so_info`（对象）
- [ ] FPM 模拟（`/tmp/fpm-sim/php-fpm`）调用 `xhjob_diag`，`php_binary_candidates` 列出 ≥3 个候选
- [ ] FPM 模拟调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')` 返回 true
- [ ] `xhjob_so_info.path` 指向实际加载的 .so，size 与 release 一致
- [ ] 清理：无残留 daemon，删除测试目录

## README 更新
- [ ] `README.md` 增加"如何确认加载的 .so 版本"小节（指向 `/xhjob/diag` 检查 `xhjob_so_info` 与 `php_binary_raw` 字段）

## Git 提交推送
- [ ] 新分支 `fix-fpm-binary-resolution-v2` 基于当前 main HEAD
- [ ] `git add` 仅目标文件（不使用 `-A`）
- [ ] commit message 说明根因 A（旧 .so）+ 根因 B（validate spawn 不可靠）+ 修复
- [ ] `git push -u origin fix-fpm-binary-resolution-v2` 成功
- [ ] 返回 PR 链接给用户
