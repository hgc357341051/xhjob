# 修复 PHP-FPM 上下文下 daemon 启动失败 Spec

> change-id: `fix-fpm-daemon-start`
> 范围：Rust 核心扩展（src/）+ PHP 集成包（releases/xhjob-thinkphp8-extend/Xhjob/）

## Why

用户在 ThinkPHP 8 + PHP-FPM 生产环境中，从控制器 `XhjobTask::start` 调用 `XhjobService::start()` 时，`xhjob_start('aaaa', '/www/wwwroot/xiaohus/runtime/index')` 返回 `false`，抛出 `ServiceNotRunningException: xhjob_start 返回 false，无法启动 daemon (name=aaaa)`。同时 PHP-FPM 错误日志出现 `PHP Warning: Module "xhjob" is already loaded in Unknown on line 0`。

根本原因分析：

1. **重复加载扩展**：`src/daemon/unix.rs:67` 与 `src/daemon/windows.rs:48` 在 re-exec PHP 时硬编码 `cmd.arg("-d").arg("extension=xhjob.so")`。当生产环境已通过 `php.ini` 加载 xhjob.so（PHP-FPM 标准做法）时，re-exec 出的子进程会先读 `php.ini` 加载一次，再被 `-d extension=xhjob.so` 加载第二次，触发 "already loaded" 警告。该警告本身不致命，但表明 spawn 路径对加载上下文完全无感知。

2. **daemon 启动失败但无诊断**：`daemon::start()`（`src/daemon/mod.rs:457`）调用 `spawn_via_double_fork()` 后，仅轮询 10s 等 PID 文件 + IPC socket 就绪；若超时则返回 `Ok(false)`，PHP 侧仅得到 `false`，**完全没有错误信息**。daemon 子进程的真实失败原因（setsid 失败、data_dir 不可写、PHP 二进制路径错误、`-r` 代码执行报错、权限不足等）被吞掉，用户无法定位。

3. **PHP-FPM 上下文特有障碍**（潜在诱因，需逐项排查）：
   - PHP-FPM worker 通常以 `www`/`www-data` 等低权限用户运行，可能无权写 `data_dir`（如 `/www/wwwroot/xiaohus/runtime/index` 不属于该用户）
   - PHP-FPM worker 可能受 `open_basedir` / `chroot` 限制，导致 `std::env::current_exe()` 返回的 PHP 二进制路径不可访问
   - PHP-FPM 进程可能已是非 session leader，`setsid()` 在 `pre_exec` 中可能失败
   - PHP-FPM 的 `proc_open`/`fork` 可能被 `disable_functions` 禁用（虽然 Rust 直接调 `Command::spawn` 绕过 PHP 层禁用，但环境变量继承可能受影响）
   - re-exec 的 PHP 进程会重新读 `php.ini`，若 ini 中有 `extension=xhjob.so`，加上 `-d extension=xhjob.so` 就重复了

## What Changes

### Rust 核心扩展（src/）

#### 修复（MODIFIED）

- **去重扩展加载**（`src/daemon/unix.rs` + `src/daemon/windows.rs`）：re-exec PHP 时不再无条件传 `-d extension=xhjob.so`。改为检测当前进程是否已加载 xhjob 扩展（通过 ext-php-rs 的 `module_loaded("xhjob")` 或等价机制），已加载则跳过 `-d extension=`；未加载时保留现有行为（兼容 CLI 直接 `-d extension=` 加载的场景）。
- **spawn 失败诊断**（`src/daemon/mod.rs` + `src/daemon/unix.rs`）：`spawn_via_double_fork` 在 `cmd.spawn()` 后，可选地 `wait()` 子进程一小段时间（如 500ms），若子进程过早退出则捕获其退出码 + stderr 末尾内容，写入日志并向上返回结构化错误。`daemon::start()` 在轮询超时返回 `Ok(false)` 前，读取 daemon 日志文件尾部 + 检查 PID 文件是否存在，把失败原因通过 `tracing::error!` 输出到当前进程（PHP-FPM worker）的 stderr，让用户在 PHP-FPM 错误日志中直接看到原因。
- **PHP-FPM 友好的错误传播**（`src/lib.rs` `xhjob_start`）：当 `daemon::start()` 返回 `Ok(false)` 时，除了返回 `false`，还通过 `php_log_err` / `tracing::error!` 输出诊断信息（data_dir 是否可写、PHP 二进制路径、PID 文件路径、日志文件路径、最后一条 daemon 日志），让 PHP 侧 `throw ServiceNotRunningException` 时能携带可操作的错误信息。
- **data_dir 可写性预检**（`src/daemon/mod.rs` `start()`）：在 `spawn_daemon` 之前，检查 `data_dir`（或其回退目录 `/tmp`）是否可写、可创建子目录。不可写时直接返回带路径的错误，避免 spawn 一个必然失败的子进程。

#### 新增（ADDED）

- **`xhjob_last_start_error()` 函数**（`src/lib.rs`）：返回上一次 `xhjob_start` 失败的诊断字符串（线程局部或全局 OnceCell），供 PHP 侧 `XhjobService::start()` 在 catch 异常时查询并展示给用户。
- **环境探测函数 `xhjob_diag()`**（`src/lib.rs`）：返回当前进程的关键诊断信息 JSON：PHP 二进制路径、`extension_loaded('xhjob')`、`data_dir` 解析结果与可写性、PID 文件路径、IPC socket 路径、日志文件路径、当前用户 uid/gid、`open_basedir` ini 值。供用户排查与 issue 上报。

### PHP 集成包（releases/xhjob-thinkphp8-extend/Xhjob/）

#### 修复（MODIFIED）

- **`XhjobService::start()` 携带诊断**（`XhjobService.php`）：当 `xhjob_start` 返回 `false` 时，调用 `xhjob_last_start_error()`（若存在）获取失败原因，拼入 `ServiceNotRunningException` 消息；同时调用 `xhjob_diag()` 并以 `--help` 友好格式附在异常消息或 `Throwable::getPrevious()` 中，让用户一眼看到 data_dir 路径、权限、PHP 二进制路径等关键信息。

#### 新增（ADDED）

- **`XhjobService::diag()` 静态方法**（`XhjobService.php`）：包装 `xhjob_diag()`，返回诊断数组，便于在控制器中 `return json($svc::diag())` 快速排查。

## Impact

- **Affected specs**：无（新问题，独立于 production-resilience-audit-v2 / cross-process-functional-verification）
- **Affected code**：
  - Rust: `src/daemon/mod.rs`（start/spawn 诊断 + data_dir 预检）、`src/daemon/unix.rs`（去重 `-d extension=` + spawn 后 wait 诊断）、`src/daemon/windows.rs`（同上）、`src/lib.rs`（新增 `xhjob_last_start_error` / `xhjob_diag` 函数 + `xhjob_start` 错误传播）
  - PHP: `releases/xhjob-thinkphp8-extend/Xhjob/XhjobService.php`（start 携带诊断 + diag 静态方法）
- **BREAKING**：无（仅增强错误信息与新增可选函数，不改变现有 API 契约；`xhjob_start` 仍返回 `bool`，只是失败时多了诊断通道）

## ADDED Requirements

### Requirement: 扩展加载去重
系统 SHALL 在 re-exec PHP 启动 daemon 时，检测 xhjob 扩展是否已在当前进程加载，若已加载则不再传 `-d extension=xhjob.so`，避免 "Module already loaded" 警告。

#### Scenario: php.ini 已加载 xhjob 时无重复加载
- **GIVEN** PHP-FPM 通过 `php.ini` 的 `extension=xhjob.so` 已加载扩展
- **WHEN** 控制器调用 `xhjob_start('aaaa', '/path')`
- **THEN** re-exec 的 daemon PHP 进程不再传 `-d extension=xhjob.so`
- **AND** daemon 日志与 PHP-FPM 错误日志均无 "Module already loaded" 警告
- **AND** daemon 正常启动

#### Scenario: CLI 未通过 ini 加载时保留 -d 注入
- **GIVEN** CLI 运行 `php -d extension=xhjob.so script.php`，`php.ini` 未加载 xhjob
- **WHEN** 脚本调用 `xhjob_start()`
- **THEN** re-exec 的 daemon PHP 进程保留 `-d extension=xhjob.so`（现有行为不变）
- **AND** daemon 正常启动

### Requirement: spawn 失败诊断
系统 SHALL 在 daemon 启动失败时（`xhjob_start` 返回 `false`）提供结构化诊断信息，包括失败原因、关键路径与权限状态，让用户无需翻阅多处日志即可定位。

#### Scenario: data_dir 不可写
- **GIVEN** PHP-FPM worker 以 `www` 用户运行，`data_dir='/www/wwwroot/xiaohus/runtime/index'` 属主为 `root` 且不可写
- **WHEN** 调用 `xhjob_start('aaaa', '/www/wwwroot/xiaohus/runtime/index')`
- **THEN** spawn 前预检发现 `data_dir` 不可写
- **AND** `xhjob_start` 返回 `false`
- **AND** `xhjob_last_start_error()` 返回 `"data_dir '/www/wwwroot/xiaohus/runtime/index' is not writable by current user (uid=33)"`
- **AND** `XhjobService::start()` 抛出的 `ServiceNotRunningException` 消息含上述诊断

#### Scenario: daemon 子进程启动后立即退出
- **GIVEN** re-exec 的 PHP 进程因 `open_basedir` 限制无法访问 PHP 二进制路径
- **WHEN** 调用 `xhjob_start()`
- **THEN** spawn 后短时 `wait()` 捕获到子进程退出码非 0
- **AND** 读取 daemon 日志文件尾部（若有内容）
- **AND** `xhjob_last_start_error()` 返回含退出码与日志尾部的诊断字符串
- **AND** `xhjob_start` 返回 `false`

#### Scenario: 轮询超时且无 PID 文件
- **GIVEN** daemon 子进程启动但 10s 内未写 PID 文件 + 未 bind IPC socket
- **WHEN** 调用 `xhjob_start()`
- **THEN** 轮询超时后读取 daemon 日志文件尾部
- **AND** `xhjob_last_start_error()` 返回 `"daemon did not become ready within 10s; last log lines: ..."`
- **AND** `xhjob_start` 返回 `false`

### Requirement: 环境诊断函数
系统 SHALL 提供 `xhjob_diag()` 函数返回当前进程的环境诊断 JSON，覆盖 daemon 启动相关的全部关键路径与状态。

#### Scenario: 排查启动失败
- **WHEN** 调用 `xhjob_diag()`
- **THEN** 返回 JSON 含：`php_binary`（`std::env::current_exe()`）、`extension_loaded`（bool）、`data_dir`（解析后的路径）、`data_dir_writable`（bool）、`pid_file_path`、`ipc_socket_path`、`log_file_path`、`current_uid`（Unix）、`current_gid`（Unix）、`open_basedir`（ini 值）

### Requirement: XhjobService 诊断方法
`XhjobService` SHALL 提供静态 `diag()` 方法包装 `xhjob_diag()`，并在 `start()` 失败时把诊断信息附入异常。

#### Scenario: 控制器快速排查
- **WHEN** 在控制器中调用 `XhjobService::diag()`
- **THEN** 返回诊断数组，可直 `return json($arr)`

#### Scenario: start 失败异常含诊断
- **WHEN** `XhjobService::start()` 抛出 `ServiceNotRunningException`
- **THEN** 异常消息含 `xhjob_last_start_error()` 内容
- **AND** 异常的 `getPrevious()` 或消息含 `xhjob_diag()` 摘要

## MODIFIED Requirements

### Requirement: xhjob_start 错误传播
`xhjob_start` 在返回 `false` 前 SHALL 通过 `tracing::error!` 与线程局部变量记录失败原因，使 PHP 侧可通过 `xhjob_last_start_error()` 查询。现有返回 `bool` 的签名保持不变（向后兼容）。

#### Scenario: 失败原因可查询
- **GIVEN** `xhjob_start()` 因 data_dir 不可写返回 `false`
- **WHEN** 紧接着调用 `xhjob_last_start_error()`
- **THEN** 返回非空字符串描述失败原因

## REMOVED Requirements

无。
