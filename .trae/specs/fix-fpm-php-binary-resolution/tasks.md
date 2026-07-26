# Tasks — fix-fpm-php-binary-resolution

> 顺序执行：Phase 1 修核心二进制解析，Phase 2 诊断字段，Phase 3 编译 + 单测，Phase 4 端到端验证，Phase 5 git 提交推送。
> 每个 Task 都是可独立验证的小步骤。

## Phase 1: Rust 核心二进制解析

- [x] Task 1: 新增 `resolve_php_binary()` 函数
  - [x] SubTask 1.1: `src/daemon/mod.rs` 新增 `pub(crate) fn resolve_php_binary() -> (PathBuf, PathBuf)`，返回 `(resolved, raw)`。`raw` = `current_exe()` 原始值；`resolved` 按优先级解析：`XHJOB_PHP_BINARY` env → `current_exe()`（若文件名非 fpm/cgi）→ 同目录 `php` → `../bin/php` → `which php` → 回退 `raw`
  - [x] SubTask 1.2: 实现 `fn is_cli_php_binary(path: &Path) -> bool`：判断文件名是否为 CLI php（`php`、`php7.4`、`php8.2` 等），排除 `php-fpm`、`php-cgi`、`lsphp`、`php-cgi`。文件名匹配规则：以 `php` 开头，且不包含 `fpm`/`cgi` 子串
  - [x] SubTask 1.3: 实现 `fn validate_php_binary(path: &Path) -> bool`：用 `Command::new(path).arg("-n").arg("-v").stdout(Stdio::null()).stderr(Stdio::null()).status()`，1s 超时（`wait_timeout` crate 或手动 `try_wait` 轮询），退出码为 0 视为可用
  - [x] SubTask 1.4: 实现 `fn which_php() -> Option<PathBuf>`：遍历 `PATH` 环境变量，找第一个 `php` 可执行文件
  - [x] SubTask 1.5: 单测 `test_resolve_php_binary_returns_cli_directly`、`test_resolve_php_binary_substitutes_fpm`、`test_resolve_php_binary_respects_env_override`、`test_resolve_php_binary_falls_back_to_raw`、`test_is_cli_php_binary_rejects_fpm`、`test_validate_php_binary_returns_false_for_missing`（注：前两个 spec 列出的测试因不可移植已按 spec 指示跳过，实现并验证了后 4 个）

- [x] Task 2: `spawn_via_double_fork` 与 `spawn_via_create_process` 接入
  - [x] SubTask 2.1: `src/daemon/unix.rs` `spawn_via_double_fork` 将 `let exe = std::env::current_exe()...` 替换为 `let (exe, _raw) = super::resolve_php_binary();`
  - [x] SubTask 2.2: `src/daemon/windows.rs` `spawn_via_create_process` 同步替换
  - [x] SubTask 2.3: `src/daemon/mod.rs` `check_data_dir_writable` 错误消息中的 `PHP binary: {:?}` 改用 `resolve_php_binary().0`
  - [x] SubTask 2.4: 编译通过 `cargo build --features persist`（dev profile 即可），无 warning

## Phase 2: 诊断字段 + exit 64 hint

- [x] Task 3: `xhjob_diag` 增加 `php_binary_raw` 字段
  - [x] SubTask 3.1: `src/lib.rs` `xhjob_diag` 中 `let php_binary_raw = std::env::current_exe()...`，`let php_binary = daemon::resolve_php_binary().0...`
  - [x] SubTask 3.2: JSON 输出新增 `"php_binary_raw": php_binary_raw`，保留原 `"php_binary"` 为 resolved 路径
  - [x] SubTask 3.3: docstring 更新：列出 `php_binary_raw` 字段，并说明 `XHJOB_PHP_BINARY` 环境变量可覆盖解析
  - [x] SubTask 3.4: 单测 `test_diag_includes_php_binary_raw`（在已有 `test_diag_returns_required_fields` 基础上断言新字段存在）

- [x] Task 4: `check_child_alive` exit 64 hint
  - [x] SubTask 4.1: `src/daemon/mod.rs` `check_child_alive` 中 `Ok(Some(status))` 分支：检测 `status.code() == Some(64)`（Unix）或 `unix_wait_status` 解码后 exit code 为 64，追加 hint 字符串到错误消息
  - [x] SubTask 4.2: hint 文本：`"hint: exit code 64 (EX_USAGE) typically means the spawned binary rejected CLI flags (-r/-d); if php_binary_raw is php-fpm/php-cgi, set XHJOB_PHP_BINARY to the CLI php binary"`
  - [x] SubTask 4.3: 单测 `test_check_child_alive_hint_on_exit_64`（构造一个立即以 64 退出的 dummy child，断言返回的 Err 含 "EX_USAGE" 与 "XHJOB_PHP_BINARY"）

## Phase 3: 编译 + 单测 + clippy

- [x] Task 5: Release 编译 + 全量单测 + clippy
  - [x] SubTask 5.1: `cargo build --release --features persist` 编译通过（3m09s），无 warning
  - [x] SubTask 5.2: `cargo test --features persist` 100% 通过（193 passed; 0 failed; 0 ignored — 前序 187 + 新增 6 个）
  - [x] SubTask 5.3: `cargo clippy --all-targets --features persist -- -D warnings` 无 warning（exit 0）
  - [x] SubTask 5.4: 复制 `target/release/libxhjob.so` 到 `releases/xhjob-php8.2-linux-x86_64.so`，MD5 一致：`4302f0ca7cf1d2dfc9cf31b33e668739`

## Phase 4: 端到端验证

- [x] Task 6: CLI 模拟 FPM 上下文验证
  - [x] SubTask 6.1: 构造测试：将 `current_exe()` 模拟为 `php-fpm`（通过 cp 真实 PHP 二进制到 `/tmp/fpm-sim/php-fpm`，避免 symlink 被 `/proc/self/exe` 自动解析），验证 resolved 路径为 CLI `php`
  - [x] SubTask 6.2: FPM 模拟场景验证：用 `/tmp/fpm-sim/php-fpm` 调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')` 返回 `true`，daemon 启动成功（pid=14407）、PID 文件存在、`status()` running=true
  - [x] SubTask 6.3: `xhjob_diag('tp-demo', '/tmp/xhjob-tp-demo')` 返回 JSON 含 `php_binary`（`/root/.phpenv/shims/php`）与 `php_binary_raw`（`/tmp/fpm-sim/php-fpm`），两者不同
  - [x] SubTask 6.4: `XHJOB_PHP_BINARY=/root/.phpenv/versions/8.2snapshot/bin/php` 时 `xhjob_diag` 的 `php_binary` 字段使用该路径，跳过自动探测
  - [x] SubTask 6.5: 清理：停止 daemon、删除 `/tmp/xhjob-tp-demo` 与 `/tmp/fpm-sim` 测试目录、`pkill xhjob_run_daemon` 无残留进程
  - [x] SubTask 6.6（额外）: CLI 上下文回归验证：直接 `php -d extension=xhjob.so` 调用 `xhjob_start` 同样返回 true（pid=14286），证明修复未破坏 CLI 路径

## Phase 5: Git 提交推送

- [x] Task 7: 创建分支 + 提交 + 推送
  - [x] SubTask 7.1: `git checkout -b fix-fpm-php-binary-resolution`（基于当前 main HEAD `79e0de8`）
  - [x] SubTask 7.2: `git status` 确认改动文件列表（5 修改 + 3 新 spec 文档）
  - [x] SubTask 7.3: `git add` 仅 8 个目标文件（5 代码/产物 + 3 spec 文档，未使用 `-A`）
  - [x] SubTask 7.4: `git commit` 用 `git -c user.name=trae-agent -c user.email=trae-agent@users.noreply.github.com`（不修改 global/local config）创建 commit `9610568`，message 含根因、修复、验证详情
  - [x] SubTask 7.5: `git push -u origin fix-fpm-php-binary-resolution` 成功推送（用 fine-grained PAT with Contents:Write 认证 `gh auth`），PR 链接：https://github.com/hgc357341051/xhjob/pull/new/fix-fpm-php-binary-resolution

# Task Dependencies

- Task 1（resolve_php_binary）是基础，Task 2/3/4 都依赖它
- Task 2（spawn 接入）与 Task 3（diag 字段）与 Task 4（exit hint）独立可并行
- Task 5（编译测试）依赖 Task 1-4 全部完成
- Task 6（端到端）依赖 Task 5
- Task 7（git 提交）依赖 Task 6 验证通过

# 风险点

- **`wait_timeout` crate 引入**：为避免新增依赖，`validate_php_binary` 用 `try_wait` 轮询（100ms × 10 = 1s）而非 `wait_timeout` crate，保持 `Cargo.toml` 不变
- **`which php` 跨平台**：Unix 用 `std::env::var("PATH")` + 手动遍历；Windows 同样遍历 `PATH` 但加 `.exe` 后缀。不引入 `which` crate
- **`php -n -v` 验证副作用**：`-n` 跳过 php.ini 加载，避免验证时触发 `extension=xhjob.so` 重复加载警告；`-v` 仅打印版本后退出，无副作用
- **FPM 上下文 `current_exe()` 返回值**：某些 FPM 部署（如 systemd 单元）可能将 `current_exe()` 报告为 systemd 本身或 wrapper 脚本；此时 `is_cli_php_binary()` 返回 false，回退到 `which php`。若 `which php` 也失败，回退到原始 `current_exe()`，让诊断继续暴露问题
- **MD5 一致性**：重新编译后 `target/release/libxhjob.so` 与 `releases/xhjob-php8.2-linux-x86_64.so` MD5 必须一致（用 `md5sum` 校验）
