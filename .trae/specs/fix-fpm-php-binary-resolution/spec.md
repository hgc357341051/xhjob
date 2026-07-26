# 修复 PHP-FPM 上下文下 re-exec 错误二进制 (php-fpm) Spec

> change-id: `fix-fpm-php-binary-resolution`
> 范围：Rust 核心扩展（src/）+ 编译产物（releases/xhjob-php8.2-linux-x86_64.so）+ git 远程分支提交

## Why

执行控制器 `XhjobTask` 里的 demo 仍然报错（即便 `fix-fpm-daemon-start` 已落地，扩展去重、data_dir 预检、`xhjob_diag`/`xhjob_last_start_error` 都已就绪）：

```
xhjob_start 返回 false，无法启动 daemon (name=tp-demo)
reason: xhjob_start failed: io: daemon child exited prematurely
        with status ExitStatus(unix_wait_status(16384))
diag: { "php_binary": "/www/server/php/82/sbin/php-fpm",
        "sapi": "fpm-fcgi",
        "extension_loaded_via_php_ini": true,
        "data_dir_writable": true, ... }
```

退出码 `16384 = 64 << 8`，对应 BSD `sysexits.h` 的 `EX_USAGE`（命令行参数错误）。daemon 日志文件为空（`<no log file or empty>`）—— 子进程在写任何 tracing 输出之前就被 PHP 二进制拒绝执行了。

**根本原因**：

- `src/daemon/unix.rs:34` 与 `src/daemon/windows.rs:29` 都通过 `std::env::current_exe()` 定位 PHP 二进制以 re-exec 启动 daemon。
- 在 PHP-FPM 上下文（`sapi=fpm-fcgi`）下，`std::env::current_exe()` 返回的是 **`php-fpm`** 二进制（如 `/www/server/php/82/sbin/php-fpm`），不是 CLI `php` 二进制。
- `php-fpm` 是 FastCGI Process Manager，**不接受 `-r`（运行 PHP 代码字符串）和 `-d`（覆盖 ini）这些 CLI 参数**；它只认 `-F`/`-R`/`-g`/`-p` 等 FPM 自己的 flag。所以扩展构造的 `php-fpm -d extension=xhjob.so -r 'xhjob_run_daemon(...);'` 立即被 `php-fpm` 拒绝，进程以 `EX_USAGE (64)` 退出。
- `xhjob_diag()` (`src/lib.rs:203`) 也用 `current_exe()` 上报 `php_binary`，所以诊断 JSON 显示的是 `php-fpm` 路径——这是误导性的，因为真正被用来 spawn 的也是 `php-fpm`，但用户以为是 CLI php。

**结论**：扩展需要在 FPM 上下文中将 re-exec 的二进制从 `php-fpm` **替换为同安装目录下的 CLI `php` 二进制**，并允许用户通过环境变量显式覆盖。

## What Changes

### Rust 核心扩展（src/）

#### 修复（MODIFIED）

- **新增 `resolve_php_binary()` 函数**（`src/daemon/mod.rs`）：统一解析真正可用的 CLI PHP 二进制路径。解析优先级：
  1. 环境变量 `XHJOB_PHP_BINARY`（显式用户覆盖，最高优先级）
  2. `std::env::current_exe()`，但仅当其文件名 **不是** `php-fpm`/`php-cgi`/`lsphp`/`php-cgi`（即本来就是 CLI 二进制）时直接采用
  3. 当 `current_exe()` 文件名是 `php-fpm`（或其它非 CLI SAPI 二进制）时，按以下顺序查找同版本 CLI `php`：
     - 同目录下的 `php`（如 `/www/server/php/82/sbin/php`）
     - 相对 fpm 目录的 `../bin/php`（BT/aapanel 约定：`/www/server/php/82/sbin/php-fpm` → `/www/server/php/82/bin/php`）
     - `which php`（PATH 查找，跨发行版兜底）
  4. 全部失败时回退到原始 `current_exe()`（保持现状以便诊断能继续暴露问题，而不是静默成功）
  - 候选路径必须通过验证：`<candidate> -n -v` 在 1s 内退出码为 0（确认是 CLI PHP 二进制且可执行）。
  - 返回 `(resolved: PathBuf, raw: PathBuf)` 二元组：`resolved` 是实际用于 spawn 的路径，`raw` 是 `current_exe()` 原始值，供诊断同时展示两者。
- **`src/daemon/unix.rs` `spawn_via_double_fork`**：用 `resolve_php_binary()` 替换直接 `std::env::current_exe()` 调用；spawn 时使用 `resolved` 路径。其它逻辑（`-d extension=` 去重、`-r` 代码字符串、env vars、log redirect、double-fork）保持不变。
- **`src/daemon/windows.rs` `spawn_via_create_process`**：同步修改。
- **`src/daemon/mod.rs` `check_data_dir_writable`**：错误消息里的 `PHP binary: {:?}` 改用 `resolve_php_binary()` 的 `resolved`（让预检失败消息里的二进制路径反映实际会被 spawn 的二进制，而不是误导性的 `php-fpm`）。
- **`src/daemon/mod.rs` `check_child_alive` 错误增强**：当子进程退出码是 `64`（`EX_USAGE`）时，在错误消息追加 hint：`"hint: exit code 64 (EX_USAGE) typically means the spawned binary rejected CLI flags (-r/-d); if php_binary is php-fpm/php-cgi, set XHJOB_PHP_BINARY to the CLI php binary"`。
- **`src/lib.rs` `xhjob_diag`**：返回 JSON 中 `php_binary` 字段改为 `resolved` 路径，**新增 `php_binary_raw` 字段**（`current_exe()` 原始值），让用户一眼看出"实际用的 CLI php"vs"当前进程的原始二进制（可能是 php-fpm）"。其它字段不变。

#### 新增（ADDED）

- **环境变量 `XHJOB_PHP_BINARY`**：用户可显式指定 CLI PHP 二进制路径，绕过自动探测。文档化在 `xhjob_diag` 的 docstring 中（不创建独立 md 文档）。

### 编译产物

- **重新编译 .so**：`cargo build --release --features persist` 生成 `target/release/libxhjob.so`，覆盖 `releases/xhjob-php8.2-linux-x86_64.so`。MD5 一致性自检。

### Git 提交

- **新分支**：在当前 `HEAD` 基础上创建分支 `fix-fpm-php-binary-resolution`，提交所有 Rust/PHP 改动 + 重新编译的 `.so`，推送到远程 `origin`。
- **不**修改 `main`/`master`；用户后续按需合并 PR。

## Impact

- **Affected specs**：
  - `fix-fpm-daemon-start`（前序）：本 spec 在其诊断基础设施之上修复新发现的根因；不改动前序 spec 已落地的去重/预检/诊断函数签名。
- **Affected code**：
  - `src/daemon/mod.rs`：新增 `resolve_php_binary()`；`check_data_dir_writable` / `check_child_alive` 用其结果；`check_child_alive` 增加 exit-64 hint。
  - `src/daemon/unix.rs`：`spawn_via_double_fork` 用 `resolve_php_binary()`。
  - `src/daemon/windows.rs`：`spawn_via_create_process` 同步。
  - `src/lib.rs`：`xhjob_diag` 增加 `php_binary_raw` 字段；docstring 提及 `XHJOB_PHP_BINARY`。
  - `releases/xhjob-php8.2-linux-x86_64.so`：重新编译替换。
- **BREAKING**：无。
  - `xhjob_start` / `xhjob_stop` / `xhjob_restart` / `xhjob_status` / `xhjob_last_start_error` 签名与返回类型不变。
  - `xhjob_diag` 返回的 JSON 仅**新增** `php_binary_raw` 字段（向后兼容追加；PHP 侧 `XhjobService::diag()` 用 `json_decode` 解析，新增字段不影响）。
  - 无 PHP 用户侧 API 变更。

## ADDED Requirements

### Requirement: FPM 上下文自动解析 CLI PHP 二进制
系统 SHALL 在 re-exec 启动 daemon 前，将 `std::env::current_exe()` 返回的非 CLI 二进制（`php-fpm` / `php-cgi` / `lsphp`）替换为同版本安装下的 CLI `php` 二进制，使 `-r` / `-d` 参数能被正确解析。

#### Scenario: PHP-FPM 上下文自动替换 php-fpm 为 CLI php
- **GIVEN** PHP-FPM worker 进程 `current_exe()=/www/server/php/82/sbin/php-fpm`，且 `/www/server/php/82/bin/php` 存在且可执行
- **WHEN** 控制器调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')`
- **THEN** daemon 实际 spawn 的命令使用 `/www/server/php/82/bin/php`（而非 `php-fpm`）
- **AND** daemon 子进程正常启动并写入 PID 文件 + 绑定 IPC socket
- **AND** `xhjob_start` 返回 `true`
- **AND** daemon 日志无 `EX_USAGE`/exit 64 错误

#### Scenario: XHJOB_PHP_BINARY 显式覆盖
- **GIVEN** 环境变量 `XHJOB_PHP_BINARY=/usr/local/bin/php`，且该路径存在且可执行
- **WHEN** 调用 `xhjob_start()`
- **THEN** daemon spawn 使用 `/usr/local/bin/php`，跳过自动探测
- **AND** 即便 `current_exe()` 是 `php-fpm`，也不触发 fpm 替换逻辑

#### Scenario: 探测全部失败时回退到 current_exe
- **GIVEN** `current_exe()=/usr/local/php-fpm`，同目录无 `php`、`../bin/php` 不存在、`which php` 失败
- **WHEN** 调用 `xhjob_start()`
- **THEN** spawn 使用原始 `current_exe()`（保持现状以让诊断能继续暴露问题，而非静默成功）
- **AND** `xhjob_last_start_error()` 在子进程 `EX_USAGE` 退出后被写入含 hint 的错误消息
- **AND** `xhjob_diag()` 返回的 `php_binary` 与 `php_binary_raw` 相同

### Requirement: 候选 PHP 二进制验证
系统 SHALL 在采用候选 CLI 二进制前，通过 `<candidate> -n -v` 验证其可执行且为 CLI SAPI；验证失败时跳过该候选继续尝试下一个，避免 spawn 一个必然失败的进程。

#### Scenario: 候选二进制不可执行
- **GIVEN** `current_exe()=/opt/php/sbin/php-fpm`，同目录 `php` 是损坏的软链
- **AND** `../bin/php` 存在且 `php -n -v` 返回 0
- **WHEN** 调用 `xhjob_start()`
- **THEN** 跳过同目录的损坏 `php`，采用 `../bin/php`
- **AND** daemon 正常启动

### Requirement: 诊断 JSON 暴露原始与解析后二进制
`xhjob_diag()` SHALL 在返回 JSON 中同时包含 `php_binary`（实际用于 spawn 的解析后路径）与 `php_binary_raw`（`current_exe()` 原始值），让用户能在 FPM 上下文一眼区分两者。

#### Scenario: FPM 上下文 diag 返回两个不同路径
- **GIVEN** PHP-FPM worker `current_exe()=/www/server/php/82/sbin/php-fpm`，解析后 CLI 路径 `/www/server/php/82/bin/php`
- **WHEN** 调用 `xhjob_diag('tp-demo', '/tmp/xhjob-tp-demo')`
- **THEN** 返回 JSON 含 `"php_binary": "/www/server/php/82/bin/php"`
- **AND** 含 `"php_binary_raw": "/www/server/php/82/sbin/php-fpm"`

### Requirement: exit 64 错误 hint
`check_child_alive` SHALL 在子进程退出码为 64（`EX_USAGE`）时，在错误消息中追加 hint，提示用户检查 `php_binary` 是否为 `php-fpm`/`php-cgi` 并设置 `XHJOB_PHP_BINARY`。

#### Scenario: 子进程 EX_USAGE 退出时 hint 出现在 last_start_error
- **GIVEN** daemon 子进程退出码为 64
- **WHEN** `xhjob_start` 失败后调用 `xhjob_last_start_error()`
- **THEN** 返回的错误字符串包含 `"hint: exit code 64 (EX_USAGE)"`
- **AND** 包含 `"set XHJOB_PHP_BINARY"`

## MODIFIED Requirements

### Requirement: xhjob_start 在 FPM 上下文成功启动 daemon
`xhjob_start` SHALL 在 PHP-FPM 上下文（`sapi=fpm-fcgi`）下成功启动 daemon，前提是同安装目录存在可用的 CLI `php` 二进制或用户设置了 `XHJOB_PHP_BINARY`。

#### Scenario: 控制器 demo 在 FPM 上下文成功启动
- **GIVEN** ThinkPHP 8 + PHP-FPM 8.2，`php-fpm` 位于 `/www/server/php/82/sbin/php-fpm`，CLI `php` 位于 `/www/server/php/82/bin/php`
- **WHEN** 控制器 `XhjobTask::start` 调用 `XhjobService::start()` 启动 `name=tp-demo`
- **THEN** `xhjob_start` 返回 `true`
- **AND** `XhjobService::start()` 返回 daemon PID > 0
- **AND** `XhjobService::status()` 返回 `['running' => true, 'pid' => <N>]`

## REMOVED Requirements

无。
