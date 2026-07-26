# Tasks — fix-fpm-daemon-start

> 顺序执行：Phase 1 修核心去重 + 预检，Phase 2 加诊断通道，Phase 3 改 PHP 侧，Phase 4 验证。
> 每个 Task 都是可独立验证的小步骤。

## Phase 1: Rust 核心修复（去重 + 预检）

- [x] Task 1: 去重 `-d extension=xhjob.so` 重复加载
  - [x] SubTask 1.1: 在 `src/daemon/mod.rs` 新增辅助函数 `xhjob_loaded_via_php_ini()` + `should_inject_extension_arg()`，通过 FFI 读取 `sapi_module.name` / `php_ini_opened_path` / `php_ini_scanned_files`，FPM/Apache SAPI 直接返回 true；CLI 回退到 grep ini 文件
  - [x] SubTask 1.2: `src/daemon/unix.rs` `spawn_via_double_fork` 中条件传参：`if super::should_inject_extension_arg() { cmd.arg("-d").arg("extension=xhjob.so"); }`
  - [x] SubTask 1.3: `src/daemon/windows.rs` `spawn_via_create_process` 同步修改
  - [x] SubTask 1.4: 单测 `test_xhjob_loaded_via_php_ini_returns_bool`、`test_dedup_logic_in_command_args`、`test_ini_references_xhjob_extension_detects_directive`

- [x] Task 2: data_dir 可写性预检
  - [x] SubTask 2.1: `src/daemon/mod.rs` `start()` 在 `spawn_daemon` 前调用新函数 `check_data_dir_writable(service_name, data_dir)`：解析最终 `data_dir`，尝试 `create_dir_all` + 写 `.xhjob_write_test` + 删除；失败返回 `XhjobError::Io` 含路径、uid/gid、PHP 二进制路径、日志路径
  - [x] SubTask 2.2: Unix 下 `current_uid()` / `current_gid()` 使用 `libc::getuid`/`getgid`
  - [x] SubTask 2.3: 单测 `test_check_data_dir_writable_ok`、`test_check_data_dir_writable_fails_on_unwritable`（Unix 0o555 dir，root 跳过）、`test_check_data_dir_writable_fails_on_path_under_file`

## Phase 2: spawn 失败诊断（Rust）

- [x] Task 3: spawn 后短时 wait 捕获立即退出
  - [x] SubTask 3.1: `src/daemon/mod.rs` 新增 `read_log_tail(path)` 与 `check_child_alive(child, log_path, service_name)` helper（500ms 内 try_wait 轮询，子进程已退出 → 读日志尾部 2KB + 返回 Err；仍在运行 → forget 后 Ok）
  - [x] SubTask 3.2: `src/daemon/unix.rs` `spawn_via_double_fork` 在 `cmd.spawn()` 后调用 `super::check_child_alive(...)`
  - [x] SubTask 3.3: `src/daemon/windows.rs` `spawn_via_create_process` 同步集成
  - [x] SubTask 3.4: 单测 `test_check_child_alive_returns_ok_when_still_running`、`test_check_child_alive_returns_err_when_exited`、`test_read_log_tail_handles_missing_file`

- [x] Task 4: 轮询超时诊断 + `xhjob_last_start_error`
  - [x] SubTask 4.1: `src/lib.rs` 新增 `static LAST_START_ERROR: Mutex<Option<String>>` + `set_last_start_error` / `take_last_start_error`
  - [x] SubTask 4.2: `src/lib.rs` `xhjob_start` 在 `Ok(false)` 分支读 daemon 日志尾部 + 检查 PID/socket 存在性，组装诊断字符串 set + `tracing::error!`；在 `Err(e)` 分支 set 错误信息
  - [x] SubTask 4.3: `src/lib.rs` 新增 `#[php_function] pub fn xhjob_last_start_error() -> Option<String>`，返回 `take_last_start_error()`
  - [x] SubTask 4.4: 注册到 `#[php_module]` 的 `.function(wrap_function!(xhjob_last_start_error))`
  - [x] SubTask 4.5: 单测 `test_set_and_take_last_start_error`、`test_take_last_start_error_clears`、`test_last_start_error_set_on_failure_branch`

- [x] Task 5: `xhjob_diag()` 环境诊断函数
  - [x] SubTask 5.1: `src/lib.rs` 新增 `#[php_function] pub fn xhjob_diag(name, data_dir) -> String`（返回 JSON 字符串，因 ext-php-rs 0.15 的 `IntoZval` 不支持 `serde_json::Value`），含 `php_binary`、`sapi`、`extension_loaded_via_php_ini`、`service_name`、`data_dir`、`data_dir_writable`、`pid_file_path`、`log_file_path`、`ipc_socket_path`、`current_uid`、`current_gid`、`open_basedir`、`last_start_error`
  - [x] SubTask 5.2: 注册到 `#[php_module]` 的 `.function(wrap_function!(xhjob_diag))`
  - [x] SubTask 5.3: 单测 `test_diag_returns_required_fields`、`test_diag_handles_invalid_service_name`
  - [x] SubTask 5.4: `src/daemon/mod.rs` 提 `pub(crate) resolve_data_dir` / `read_open_basedir` / `read_sapi_name` / `current_uid` / `current_gid` / `read_log_tail` 供 lib.rs 调用

## Phase 3: PHP 集成包改进

- [x] Task 6: `XhjobService::start()` 携带诊断
  - [x] SubTask 6.1: `releases/xhjob-thinkphp8-extend/Xhjob/XhjobService.php` `start()` 在 `xhjob_start` 返回 `false` 时，通过 `function_exists` 探测后调用 `xhjob_last_start_error()` + `xhjob_diag()`
  - [x] SubTask 6.2: 信息拼入 `ServiceNotRunningException` 消息：`"...(name={$this->name}); reason: $reason; diag: $diagJson"`
  - [x] SubTask 6.3: `restart()` 同步增强（同样调用 `xhjob_last_start_error` + `xhjob_diag`）；`wait` + `status` 流程不变

- [x] Task 7: `XhjobService::diag()` 静态方法
  - [x] SubTask 7.1: `XhjobService.php` 新增 `public static function diag(?string $name = null, ?string $dataDir = null): array`，包装 `xhjob_diag()`；扩展未加载时抛 `XhjobException`；新增 `use Xhjob\Exception\XhjobException;`
  - [x] SubTask 7.2: `tp/app/controller/XhjobTask.php` 新增 `diag()` action：用 `$this->request->get('name'/'data_dir')` 读 query，调用 `XhjobService::diag()` 后 json 返回
  - [x] SubTask 7.3: `tp/route/app.php` 在 `Route::group('xhjob', ...)` 内 `index` 之后注册 `Route::get('diag', 'XhjobTask/diag');`

## Phase 4: 编译与端到端验证

- [x] Task 8: 编译 + 单测
  - [x] SubTask 8.1: `cargo build --release --features persist` 编译通过，无 warning
  - [x] SubTask 8.2: `cargo test --features persist` 100% 通过（187 passed; 0 failed）
  - [x] SubTask 8.3: `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
  - [x] SubTask 8.4: 复制 `target/release/libxhjob.so` 到 `releases/xhjob-php8.2-linux-x86_64.so` + 安装到 phpenv 扩展目录（MD5 一致：722b48e20141c5bbe77ed56c282e2a8b）

- [x] Task 9: 端到端验证
  - [x] SubTask 9.1: 函数注册检查 + dedup 逻辑验证：xhjob 经 php.ini conf.d/10-xhjob.ini 自动加载时，`should_inject_extension_arg()` 返回 false，re-exec daemon 不传 `-d extension=`，daemon 日志无 "Module already loaded" 警告（CLI SAPI 模拟 FPM 上下文）
  - [x] SubTask 9.2: data_dir 可写性预检：用 path-under-file 技巧（`/tmp/blocker/sub`，ENOTDIR）触发预检失败，`xhjob_start` 返回 false + `xhjob_last_start_error()` 含 "not writable by current user (uid=0, gid=0)" + PHP 二进制路径 + 日志路径；预检在 spawn 前生效，无 daemon 进程被创建
  - [x] SubTask 9.3: `xhjob_diag()` 返回 JSON 含全部 13 个必需字段：php_binary / sapi / extension_loaded_via_php_ini / service_name / data_dir / data_dir_writable / pid_file_path / log_file_path / ipc_socket_path / current_uid / current_gid / open_basedir / last_start_error；`last_start_error` 由 diag 嵌入时不被消费（peek 语义）
  - [x] SubTask 9.4: `XhjobService` 类加载（PSR-4 自动加载）+ `diag()` 静态方法返回数组 + `start()` 失败路径抛 `ServiceNotRunningException`，异常消息含 `reason:` (not writable + uid) + `diag:` (JSON)；`start()` 成功路径返回 PID=21813，`status()` running=true
  - [x] SubTask 9.5: 清理：无残留 daemon 进程，删除全部测试临时目录（/tmp/xhjob-test-dir, /tmp/xhjob-service-test, /tmp/xhjob-phpini-test, /tmp/xhjob-phase1-test）+ 测试脚本 + blocker 文件；交付物（.so / 源码）保持完整

# Task Dependencies

- Task 1（去重）与 Task 2（预检）独立可并行
- Task 3（spawn wait）依赖 Task 1/2 的 spawn 路径变更
- Task 4（last_start_error）依赖 Task 3
- Task 5（diag）独立可并行于 Task 1-4
- Task 6/7（PHP 侧）依赖 Task 4/5 的 Rust 函数已注册
- Task 8（编译）依赖 Task 1-5 全部完成
- Task 9（端到端）依赖 Task 8

# 风险点

- **ext-php-rs 检测扩展加载的 API**：需确认 `ext_php_rs` 是否暴露 `ModuleEntry::find` 或 `extension_loaded` 等价 API。若 API 不可用，退化方案为：在 `daemon::start()` 时检测 `xhjob_run_daemon` 函数是否已注册（通过 `zend_get_module` 反射或检查 `EG(function_table)`），或在 Rust 侧用一个 `OnceCell<bool>` 在 `#[php_module]` 初始化时记录"扩展已加载"
- **`wait_timeout` 在 Windows**：`std::process::Child::wait_timeout` 仅 Unix 可用；Windows 需用 `WaitForSingleObject` 或简单 `try_wait` 轮询
- **日志文件尾部读取**：daemon 日志可能很大，需只读最后 2KB 避免内存爆炸
- **`open_basedir` 读取**：PHP 8 的 `open_basedir` 是 INI 系统，ext-php-rs 需暴露 `zend_ini_string("open_basedir")`；若无，退化方案是不返回该字段
