# Checklist — fix-fpm-binary-resolution-v2

## validate_php_binary 静态化
- [x] `src/daemon/mod.rs` `validate_php_binary` 不再 spawn 子进程，改为 `is_file` + Unix `mode & 0o111 != 0`
- [x] 旧 spawn-based 逻辑保留为 `validate_php_binary_spawn`（私有，`#[allow(dead_code)]`，单测用）
- [x] docstring 说明改为静态检查的原因（FPM 上下文 spawn 不可靠）
- [x] 单测：`test_validate_php_binary_returns_false_for_missing`（已有，仍通过）
- [x] 单测：`test_validate_php_binary_returns_true_for_executable_file`（新建，tempdir + chmod 0o755）
- [x] 单测：`test_validate_php_binary_returns_false_for_non_executable`（新建，Unix-gated，chmod 0o644）
- [x] 单测：`test_validate_php_binary_spawn_returns_false_for_missing`（新建，覆盖旧 spawn 逻辑）

## resolve_php_binary 候选扩展
- [x] 新增 `<raw_dir>/../sbin/php` 候选（在 `php`、`../bin/php` 之后）
- [x] 候选用 `Path::canonicalize` 去重（canonicalize 失败时 fallback 字符串比较，`canonical_key` helper）
- [x] 返回值改为 `(PathBuf, PathBuf, Vec<PhpBinaryCandidate>)`
- [x] 所有调用点（unix.rs/windows.rs/check_data_dir_writable/lib.rs）同步更新解构 3-tuple
- [x] 所有候选失败时 `tracing::warn!` 输出完整候选列表 + 失败原因（格式 `[{path} -> {reason}, ...]`）
- [x] 单测：`test_resolve_php_binary_records_candidates`（断言 candidates 非空 + 每元素 path/valid/reason）；同步更新 `test_resolve_php_binary_respects_env_override` 与 `test_resolve_php_binary_falls_back_to_raw` 解构 3-tuple
- [x] 额外 bug 修复：`record` 闭包 valid 判定——`valid = reason == "ok" || reason == "current_exe is CLI php"`（修前 raw CLI case 被误判 false）

## xhjob_diag 新字段
- [x] `php_binary_candidates` 字段（JSON 数组，每元素含 path/valid/reason）
- [x] `xhjob_so_info` 字段（JSON 对象，含 path/size_bytes/mtime_epoch 或 path:null+error）
- [x] `resolve_xhjob_so_path` 扫描 `/proc/self/maps` 找 `file_name == "xhjob.so"`（Linux only，非 Linux 返回 None）
- [x] docstring 列出两个新字段 + 说明用途（xhjob_so_info 比对 .so 版本；php_binary_candidates 定位 resolved 路径选择原因）
- [x] 单测：`test_diag_includes_php_binary_candidates`
- [x] 单测：`test_diag_includes_xhjob_so_info`
- [x] 单测：`test_resolve_xhjob_so_path_returns_option_no_panic`
- [x] 同步更新 `test_diag_returns_required_fields` 的 `required_keys` 加入两个新字段

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning（1m06s，exit 0）
- [x] `cargo test --features persist` 100% 通过（200 passed; 0 failed; 0 ignored — 前序 193 + 新增 7 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning（exit 0）
- [x] `target/release/libxhjob.so` 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- [x] MD5 一致性校验通过：`ba6f147358dfd624b3eb9143616d69d6`（11830208 bytes）

## 端到端验证
- [x] CLI 调用 `xhjob_diag` 返回 JSON 含 `php_binary_candidates`（数组，1 个候选 valid=true reason="current_exe is CLI php"）+ `xhjob_so_info`（对象，path 指向扩展目录，size=11830208）
- [x] FPM 模拟（`/tmp/fpm-sim/php-fpm` 真实 binary copy）调用 `xhjob_diag`，`php_binary_candidates` 列出 4 个候选（同目录 php、../bin/php、../sbin/php、which php），前 3 个 valid=false reason="not a file"，第 4 个 valid=true reason="ok"
- [x] FPM 模拟调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')` 返回 true，daemon pid=16517 running=true，PID 文件存在
- [x] `xhjob_so_info.path` 指向 `/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so`，size=11830208 与 `releases/xhjob-php8.2-linux-x86_64.so` 一致
- [x] 清理：停止 daemon、删除 `/tmp/xhjob-tp-demo` 与 `/tmp/fpm-sim`、`pkill xhjob_run_daemon` 无残留进程

## README 更新
- [x] `README.md` 增加"确认加载的 .so 版本（重要）"小节（line 54-101）：PHP 代码片段检查 `xhjob_so_info` + `php_binary_raw` + `php_binary_candidates` 字段；bash 步骤复制 .so 到扩展目录 + 重启 FPM + jq 验证

## Git 提交推送
- [x] 新分支 `fix-fpm-binary-resolution-v2` 基于当前 main HEAD `acc5676` 创建
- [x] `git add` 仅 9 个目标文件（3 spec + 4 Rust 源码 + 1 .so + 1 README，不使用 `-A`）
- [x] commit message 说明根因 A（旧 .so）+ 根因 B（validate spawn 不可靠）+ 修复（静态检查 + 候选扩展 + 诊断字段 + record 闭包 bug 修复）
- [x] `git push -u origin fix-fpm-binary-resolution-v2` 成功，commit `5eb5e26`（9 files, +864/-42）
- [x] 返回 PR 链接给用户：https://github.com/hgc357341051/xhjob/pull/new/fix-fpm-binary-resolution-v2
