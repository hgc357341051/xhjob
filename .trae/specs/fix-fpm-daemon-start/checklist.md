# Checklist — fix-fpm-daemon-start

## 扩展加载去重
- [x] `src/daemon/unix.rs` `spawn_via_double_fork` 不再无条件传 `-d extension=xhjob.so`
- [x] `src/daemon/windows.rs` `spawn_via_create_process` 同步修改
- [x] 当 xhjob 已通过 php.ini 加载时，re-exec 的 daemon 进程无 "Module already loaded" 警告
- [x] 当 xhjob 未通过 php.ini 加载（CLI `-d extension=` 路径）时，daemon 仍能正常启动

## data_dir 预检
- [x] `src/daemon/mod.rs` `start()` 在 spawn 前检查 data_dir 可写性
- [x] 不可写时返回的 `Err` 含路径、当前 uid、PHP 二进制路径、日志路径
- [x] `xhjob_start` 在预检失败时返回 `false` 且 `xhjob_last_start_error()` 含可操作信息

## spawn 失败诊断
- [x] `spawn_via_double_fork` 在 spawn 后 wait 500ms 捕获立即退出的子进程
- [x] 子进程立即退出时返回的 `Err` 含退出码 + 日志尾部
- [x] `daemon::start()` 轮询超时返回 `Ok(false)` 前读日志尾部并 `set_last_start_error`
- [x] `xhjob_last_start_error()` 函数已注册到 PHP 模块
- [x] `xhjob_start` 失败后 `xhjob_last_start_error()` 返回非空字符串

## `xhjob_diag()` 诊断函数
- [x] `src/lib.rs` 新增 `xhjob_diag(name, data_dir)` 函数
- [x] 返回 JSON 含：`php_binary`、`extension_loaded`、`data_dir`、`data_dir_writable`、`pid_file_path`、`ipc_socket_path`、`log_file_path`、`current_uid`（Unix）、`current_gid`（Unix）、`open_basedir`
- [x] 已注册到 `#[php_module]` 的 `.function(wrap_function!(xhjob_diag))`

## PHP 集成包
- [x] `XhjobService::start()` 失败时调用 `xhjob_last_start_error()` 与 `xhjob_diag()`（用 `function_exists` 探测）
- [x] `ServiceNotRunningException` 消息含失败原因 + 诊断信息
- [x] `XhjobService::diag()` 静态方法包装 `xhjob_diag()`
- [x] `tp/app/controller/XhjobTask.php` 新增 `diag()` action
- [x] `tp/route/app.php` 注册 `/xhjob/diag` 路由

## 编译与测试
- [x] `cargo build --release --features persist` 无 warning
- [x] `cargo test --features persist` 100% 通过
- [x] `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
- [x] 新增单测全部通过（共 13 个，覆盖去重/预检/spawn 诊断/last_start_error/diag 全部功能）：`test_xhjob_loaded_via_php_ini_returns_bool`、`test_dedup_logic_in_command_args`、`test_ini_references_xhjob_extension_detects_directive`、`test_check_data_dir_writable_ok`、`test_check_data_dir_writable_fails_on_unwritable`、`test_check_data_dir_writable_fails_on_path_under_file`、`test_check_child_alive_returns_ok_when_still_running`、`test_check_child_alive_returns_err_when_exited`、`test_read_log_tail_handles_missing_file`、`test_set_and_take_last_start_error`、`test_take_last_start_error_clears`、`test_last_start_error_set_on_failure_branch`、`test_diag_returns_required_fields`、`test_diag_handles_invalid_service_name`

## 端到端验证
- [x] PHP-FPM 上下文调用 `xhjob_start('aaaa', '/tmp/xhjob-test')` 无 "Module already loaded" 警告 + daemon 启动成功
- [x] `data_dir` 设为不可写路径时，`xhjob_start` 返回 `false` + `xhjob_last_start_error()` 含 "not writable" + uid
- [x] `xhjob_diag('aaaa', '/tmp/xhjob-test')` 返回完整 JSON 字段
- [x] ThinkPHP 控制器 `XhjobTask::start` 成功时返回 `{'pid': N, 'started': true}`
- [x] ThinkPHP 控制器 `XhjobTask::start` 失败时异常消息含诊断信息
- [x] 访问 `/xhjob/diag?name=aaaa&data_dir=/tmp/xhjob-test` 返回完整诊断 JSON
