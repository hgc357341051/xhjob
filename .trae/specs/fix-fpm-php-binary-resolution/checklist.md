# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [ ] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)`
- [ ] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw`
- [ ] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp`
- [ ] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用
- [ ] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）

## spawn 接入
- [ ] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [ ] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [ ] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0`

## 诊断字段
- [ ] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段
- [ ] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）
- [ ] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖

## exit 64 hint
- [ ] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint
- [ ] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字

## 编译与单测
- [ ] `cargo build --release --features persist` 无 warning
- [ ] `cargo test --features persist` 100% 通过（前序 187 + 新增约 9 个）
- [ ] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
- [ ] 新增单测覆盖：`test_resolve_php_binary_returns_cli_directly`、`test_resolve_php_binary_substitutes_fpm`、`test_resolve_php_binary_respects_env_override`、`test_resolve_php_binary_falls_back_to_raw`、`test_is_cli_php_binary_rejects_fpm`、`test_validate_php_binary_returns_false_for_missing`、`test_diag_includes_php_binary_raw`、`test_check_child_alive_hint_on_exit_64`

## 编译产物
- [ ] `target/release/libxhjob.so` 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- [ ] MD5 一致性校验通过（`md5sum` 两路径输出相同）

## 端到端验证
- [ ] FPM 上下文（或 CLI 模拟）调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')` 返回 `true`
- [ ] daemon PID 文件 `/tmp/xhjob-tp-demo/xhjob.tp-demo.pid` 存在
- [ ] `xhjob_diag('tp-demo', '/tmp/xhjob-tp-demo')` 返回 `php_binary`（CLI）与 `php_binary_raw`（php-fpm）不同
- [ ] `XHJOB_PHP_BINARY=/path/to/cli/php` 时 spawn 使用该路径
- [ ] daemon 日志无 `EX_USAGE` / exit 64 错误
- [ ] 清理：无残留 daemon 进程，删除 `/tmp/xhjob-tp-demo` 测试目录

## Git 提交推送
- [ ] 新分支 `fix-fpm-php-binary-resolution` 基于当前 HEAD 创建
- [ ] `git add` 仅包含 5 个目标文件（不使用 `-A`）
- [ ] commit message 说明根因（php-fpm 不接受 -r/-d）与修复（resolve_php_binary）
- [ ] `git push -u origin fix-fpm-php-binary-resolution` 成功
- [ ] 返回远程分支链接给用户
# Checklist — fix-fpm# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 95# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x]# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 +# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning (exit# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning (exit 0)
- [x] 新增单# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning (exit 0)
- [x] 新增单测覆盖：`test_is_cli_php_binary_rejects_fpm`、`test_validate_php_binary_returns_false_for_missing`、`test_resolve# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning (exit 0)
- [x] 新增单测覆盖：`test_is_cli_php_binary_rejects_fpm`、`test_validate_php_binary_returns_false_for_missing`、`test_resolve_php_binary_respects_env_override`、`test_resolve_php_binary_falls_back_to_raw`、`test_diag_includes_php_binary_raw`、`test_check_child_alive_hint_on_exit_64`（注：spec 列出的 `test_resolve_php_binary_returns_cli_directly` 与 `# Checklist — fix-fpm-php-binary-resolution

## Rust 核心二进制解析
- [x] `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)` (line 802)
- [x] 优先级：`XHJOB_PHP_BINARY` env → `current_exe()`（若 CLI）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw` (lines 805-884)
- [x] `is_cli_php_binary()` 正确识别 CLI php（`php`/`php8.2` 等），排除 `php-fpm`/`php-cgi`/`lsphp` (line 896, test line 1891)
- [x] `validate_php_binary()` 用 `<candidate> -n -v` 1s 超时验证，退出码 0 才采用 (line 918, test line 1905)
- [x] `which_php()` 跨平台遍历 `PATH`（Unix 无后缀，Windows 加 `.exe`）(line 953)

## spawn 接入
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 用 `resolve_php_binary().0` 替换 `current_exe()`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
- [x] `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 PHP binary 路径用 `resolve_php_binary().0` (lines 1008, 1026)

## 诊断字段
- [x] `src/lib.rs` `xhjob_diag` 返回 JSON 新增 `php_binary_raw` 字段 (line 238)
- [x] `php_binary` 字段值为 `resolve_php_binary().0`（resolved 路径）(line 237, 211)
- [x] docstring 列出 `php_binary_raw` 并提及 `XHJOB_PHP_BINARY` 覆盖 (lines 188-194)

## exit 64 hint
- [x] `src/daemon/mod.rs` `check_child_alive` 在 exit code = 64 时追加 hint (line 700-705)
- [x] hint 含 `"EX_USAGE"` 与 `"XHJOB_PHP_BINARY"` 关键字 (lines 702-704)

## 编译与单测
- [x] `cargo build --release --features persist` 无 warning (3m09s, exit 0)
- [x] `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning (exit 0)
- [x] 新增单测覆盖：`test_is_cli_php_binary_rejects_fpm`、`test_validate_php_binary_returns_false_for_missing`、`test_resolve_php_binary_respects_env_override`、`test_resolve_php_binary_falls_back_to_raw`、`test_diag_includes_php_binary_raw`、`test_check_child_alive_hint_on_exit_64`（注：spec 列出的 `test_resolve_php_binary_returns_cli_directly` 与 `test_resolve_php_binary_substitutes_fpm` 因不可移植按 spec 指示跳过）

## 编译产物
- [x]