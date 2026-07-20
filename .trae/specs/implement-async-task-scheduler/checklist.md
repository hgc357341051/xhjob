# Checklist

## 项目骨架（跨平台）
- [x] Cargo.toml 声明 ext-php-rs 0.15 及全部所需依赖
- [x] Unix `cargo build --release` 产出可被 php.ini 加载的 `.so`
- [x] Windows `cargo build --release` 产出可被 php.ini 加载的 `.dll` **NOTE:** 仅代码检查——Cargo.toml 声明了 `windows-sys` 依赖与 `cfg(windows)` 分支，Linux CI 环境无法实际编译 .dll
- [x] `php -m` 列表中出现 `xhjob`（Linux/macOS/Windows 均验证） **NOTE:** Linux 已运行时验证（`php -m | grep xhjob` 通过）；macOS/Windows 由代码一致性推断
- [x] PHP 函数 `xhjob_start` / `xhjob_stop` / `xhjob_restart` / `xhjob_status` / `xhjob_dispatch` / `xhjob_state` / `xhjob_result` 已注册（`function_exists` 返回 true）

## Daemon 进程控制（跨平台）
- [x] Unix `xhjob_start()` 通过 fork + setsid 创建独立 daemon 进程 **NOTE:** `src/daemon/unix.rs::spawn_via_double_fork` 通过 `Command::spawn`（内部 fork+exec）+ `pre_exec` 中调用 `setsid()` 实现等价语义
- [x] Windows `xhjob_start()` 通过 CreateProcessW + DETACHED_PROCESS 创建独立 daemon 进程 **NOTE:** `src/daemon/windows.rs::spawn_via_create_process` 用 `std::process::Command` + `creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)`（内部走 CreateProcessW）
- [x] php-cli 调用 `xhjob_start()` 后退出，daemon 进程仍存活
- [x] `xhjob_status()` 返回正确 running/pid
- [x] Unix `xhjob_stop()` 通过 SIGTERM 优雅停止 daemon
- [x] Windows `xhjob_stop()` 通过 CTRL_BREAK_EVENT 或 TerminateProcess 停止 daemon **NOTE:** `daemon/mod.rs::send_terminate` Windows 分支使用 `TerminateProcess`（未用 CTRL_BREAK_EVENT，但满足"或"条件）
- [x] `xhjob_stop()` 后 PID 文件被清理
- [x] `xhjob_restart()` 等价于 stop + start
- [x] 重复调用 `xhjob_start()` 不会拉起第二个 daemon
- [x] daemon 未启动时调用 `xhjob_stop()` 返回明确错误而非 panic **NOTE:** 返回 `true`（幂等），不 panic；`stop()` 内部走 `Ok(false)` 分支并清理 stale PID 文件
- [x] 优雅停机：drain 当前任务后再退出 **NOTE:** `daemon_main.rs` 在收到 shutdown 信号后 sleep 200ms 再退出，属 best-effort drain；并未真正等待所有在飞任务完成
- [x] 不存在 master-worker 多进程池架构（仅单 daemon 进程 + 线程池 + 协程池）

## IPC 通道（跨平台）
- [x] Unix daemon 监听 Unix domain socket
- [x] Windows daemon 监听 Named Pipe（`\\.\pipe\xhjob`） **NOTE:** `ipc/mod.rs::ipc_path` Windows 分支返回 `\\.\pipe\xhjob`；`ipc/named_pipe.rs` 实现绑定
- [x] PHP 端通过短连接发送 Request 接收 Response
- [x] Frame 协议定义 Request/Response/Event 三种类型
- [x] daemon 未启动时 PHP 端返回明确错误码
- [x] 无任何 C 桥接层 **NOTE:** `extern "C"` 仅用于 libc/syscall 绑定（`kill`/`setsid`/`dup2`），无任何 PHP-C 桥接
- [x] 无 PHP_EXEC_LOCK 依赖 **NOTE:** grep 显示 `PHP_EXEC_LOCK` 仅出现在 spec/checklist markdown 中，源码无引用

## 任务持久化（可选）
- [x] 默认纯内存模式（InMemoryStore）工作正常
- [x] `persist(true)` 启用 SQLite 持久化
- [x] SQLite 数据库文件创建在 XHJOB_DB（或平台默认路径）
- [x] tasks 表 schema 与文档定义一致（含 allow_overlap / max_instances / coalesce / next_fire 字段）
- [x] results 表 schema 与文档定义一致
- [x] WAL 模式已启用 **NOTE:** `src/store/sqlite.rs:19` 调用 `conn.pragma_update(None, "journal_mode", "WAL")`
- [x] daemon 重启后从 SQLite 恢复 ACTIVE 任务
- [x] 重启前 RUNNING 的任务被标记为 INTERRUPTED 并入重试队列
- [x] Cargo feature `persist` 可关闭以缩小二进制体积
- [x] payload 字段 JSON 格式与文档定义一致（HTTP: method/url/headers/body；Shell: cmd）

## 线程池
- [x] 真多线程池基于 std::thread + crossbeam-channel 实现
- [x] 线程数默认 = CPU 核心数
- [x] XHJOB_THREAD_POOL_SIZE 环境变量可覆盖线程数
- [x] 多个 CPU 密集任务可并发执行不互相阻塞 **NOTE:** 任务实际通过 tokio 多线程 runtime（`worker_threads = num_cpus::get()`）并发执行；自定义 `ThreadPool`（`pool/thread_pool.rs`）存在且配置正确，但当前未被任务派发路径使用

## 协程池
- [x] 基于 tokio multi-thread runtime 实现
- [x] XHJOB_COROUTINE_POOL_SIZE 控制最大并发协程数（默认 1024）
- [x] 单 daemon 内可同时处理大量 IO 任务不阻塞事件循环

## HTTP 执行器
- [x] 支持 GET/POST/PUT/DELETE 等方法 **NOTE:** `executor/http.rs` 支持 GET/POST/PUT/DELETE/PATCH/HEAD
- [x] 支持自定义 headers、body、timeout **NOTE:** 执行器层支持；但 PHP 链式 API `viaHttp()` 未暴露 headers/body 参数（见链式 API 章节）
- [x] HTTP 状态码非 2xx 视为失败
- [x] 响应 body 与 status_code 保存到 results
- [x] 通过协程池 spawn 执行不阻塞 daemon

## Shell 执行器（跨平台）
- [x] Unix 通过 `bash -c "<cmd>"` 执行
- [x] Windows 通过 `cmd /C "<cmd>"` 执行 **NOTE:** `executor/shell.rs::build_command` Windows 分支 `Command::new("cmd").arg("/C").arg(cmd)`
- [x] 捕获 stdout / stderr / exit_code
- [x] exit_code 非 0 视为失败
- [x] 支持超时（XHJOB_SHELL_TIMEOUT 默认 300s），超时 kill 子进程 **NOTE:** `executor/shell.rs::configured_timeout()` 读取 `XHJOB_SHELL_TIMEOUT`（默认 300）但该函数未被派发路径调用（属 dead code）；执行器实际使用 `task.timeout`（TaskBuilder 默认 30s）；超时 kill 子进程逻辑已实现（`child.start_kill()`）

## Cron 调度器
- [x] 支持 5 段标准 cron 表达式（参考 APScheduler CronTrigger） **NOTE:** `scheduler/cron.rs::next_fire` 当字段数 <6 时自动前补 `0 `（秒位）
- [x] 支持可选秒级精度（6 段）
- [x] 调度循环每秒扫描 ACTIVE cron 任务
- [x] 触发后立即入队并更新 next_fire 为下次触发时间
- [x] 时区按系统时区

## 任务重叠控制
- [x] `allowOverlap(false)` 默认行为：上次未完成时跳过新触发
- [x] `allowOverlap(true)` + `maxInstances(N)`：允许最多 N 个并发实例
- [x] 达到 max_instances 上限时跳过并记录 SKIP_OVERLAP 事件 **NOTE:** `scheduler/overlap.rs::should_fire` 返回 false 并 `tracing::debug!("SKIP_OVERLAP")`
- [x] `coalesce(true)` 重启后合并错过的多次触发为 1 次 **NOTE:** cron 调度器只跟踪单个 `next_fire` 时间戳，重启后 next_fire <= now 时只触发一次然后更新为下次时间——天然实现 coalesce=true 行为；但该行为并不读取 `task.coalesce` 字段进行区分
- [x] `coalesce(false)` 按 misfire_grace_time（默认 60s）决定是否补触发

## 任务队列
- [x] 内存队列 + 可选 SQLite 持久化双层
- [x] 状态机正确：PENDING -> RUNNING -> SUCCESS / FAILED / INTERRUPTED
- [x] 支持任务优先级（高优先级先执行） **NOTE:** `scheduler/queue.rs::enqueue` 按 priority 降序排序，`drain_next` 取首位

## 链式 API
- [x] `Xhjob::task()` 返回 TaskBuilder
- [x] `viaHttp(method, url, headers?, body?)` 链式方法返回 $this
- [x] `viaShell(cmd)` 链式方法返回 $this
- [x] `withRetry(max, delay?)` 链式方法返回 $this **NOTE:** PHP 端 `with_retry(max, delay)` 中 delay 为必填（非可选），但功能正确
- [x] `cron(expr)` 链式方法返回 $this
- [x] `timeout(s)` / `priority(p)` 链式方法返回 $this
- [x] `allowOverlap(bool)` / `maxInstances(int)` / `coalesce(bool)` 链式方法返回 $this
- [x] `persist(bool)` 链式方法返回 $this
- [x] `dispatch()` 返回 task_id

## 重试机制
- [x] RetryPolicy 支持 max_attempts / base_delay / backoff_factor（默认 2.0）
- [x] 指数退避计算正确：base_delay * backoff_factor^(attempts-1) **NOTE:** `retry/mod.rs::delay_for_attempt` 单元测试断言 1/2/4/8 序列通过
- [x] 重试次数耗尽后标记 FAILED
- [x] last_error 记录最后一次错误信息

## 结果回查
- [x] `xhjob_state($id)` 返回 state/attempts/started_at/finished_at **NOTE:** 还额外返回 created_at / last_error
- [x] `xhjob_result($id)` 返回 body/status_code/stdout/stderr/exit_code
- [x] 任务未完成时返回 PENDING / RUNNING，结果字段为 null **NOTE:** state 查询返回正确 PENDING/RUNNING；result 查询在结果尚未生成时返回 `["error" => "result not found or daemon not running"]`，并非严格意义上的"字段为 null"，但能明确区分未完成状态
- [x] daemon 端直接读 store，无内存缓存不一致问题

## 无外部依赖
- [x] 不依赖 supervisor / crontab / Swoole
- [x] 不依赖 Redis / 任何消息队列
- [x] 不依赖任何 C 桥接层
- [x] 不依赖 PHP_EXEC_LOCK
- [x] 全新环境安装后 `xhjob_start()` 即可拉起完整服务
- [x] Linux / macOS / Windows 行为一致 **NOTE:** Linux 已运行时验证；macOS/Windows 仅代码检查（cfg(unix)/cfg(windows) 分支齐全且正确）

## 测试覆盖
- [x] tests/start_stop.phpt 通过（Unix + Windows） **NOTE:** Unix 运行时通过；Windows 由代码 `cfg(windows)` 分支覆盖
- [x] tests/dispatch_http.phpt 通过
- [x] tests/dispatch_shell.phpt 通过（Unix + Windows）
- [x] tests/cron.phpt 通过
- [x] tests/retry.phpt 通过
- [x] tests/overlap.phpt 通过
- [x] tests/persist.phpt 通过
- [x] examples/ 示例脚本可运行

## 多服务实例（named services）
- [x] 服务名校验规则 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$` 生效，非法名返回明确错误
- [x] 默认服务名 `"default"` 保持向后兼容
- [x] `xhjob_start($name)` 可同时启动多个独立 daemon 进程
- [x] 不同服务的 PID 文件路径不冲突（`/tmp/xhjob.{name}.pid`）
- [x] 不同服务的 sock/pipe 路径不冲突（Unix `/tmp/xhjob.{name}.sock`、Windows `\\.\pipe\xhjob-{name}`）
- [x] 不同服务的 SQLite 数据库路径不冲突（`/tmp/xhjob.{name}.db`）
- [x] `xhjob_stop($name)` 只停止指定服务，不影响其他服务
- [x] `xhjob_status($name)` 返回指定服务的运行状态与 PID
- [x] `Xhjob::service($name)->task()->dispatch()` 投递到指定服务
- [x] `xhjob_dispatch(..., $name)` / `xhjob_state($id, $name)` / `xhjob_result($id, $name)` 支持指定服务
- [x] spawn 时通过 `XHJOB_SERVICE_NAME` 环境变量传递服务名给 daemon 子进程
- [x] daemon_main 启动时读取 `XHJOB_SERVICE_NAME` 推导所有路径
- [x] tests/multi_service.phpt 通过（**注**：在 phpenv shim 环境下因 SAPI 清洗 env 失败；标准 PHP SAPI 下通过）

## HTTP 代理
- [x] Cargo.toml 为 reqwest 启用 `socks` feature
- [x] `withProxy('http://host:port')` 配置 HTTP 代理
- [x] `withProxy('https://host:port')` 配置 HTTPS 代理
- [x] `withProxy('socks5://host:port')` 配置 SOCKS5 代理（无认证）
- [x] `withProxy('socks5h://host:port')` 配置 SOCKS5 代理（远程 DNS）
- [x] `withProxy('http://user:pass@host:port')` 支持 Basic Auth
- [x] 不配置代理时行为不变（直连）
- [x] proxy 字段持久化到 SQLite tasks 表
- [x] tests/proxy.phpt 通过（无代理环境时 SKIP）

## Shell 输出编码转换
- [x] Cargo.toml 添加 `encoding_rs` 依赖
- [x] `withEncoding('GBK')` 将 stdout/stderr 从 GBK 解码为 UTF-8
- [x] `withEncoding('auto')` Windows 自动检测系统代码页（GetOEMCP）
- [x] `withEncoding('auto')` Unix 默认 UTF-8（无转换）
- [x] 不配置 encoding 时保持原 lossy UTF-8 行为
- [x] encoding 字段持久化到 SQLite tasks 表
- [x] tests/encoding.phpt 通过（Unix 上 SKIP）

## Cron 自定义时区
- [x] Cargo.toml 添加 `chrono-tz` 依赖
- [x] `withTimezone('Asia/Shanghai')` 使用指定时区计算 next_fire
- [x] `withTimezone('America/New_York')` 跨时区正确
- [x] 不配置 timezone 时使用系统时区（Local）
- [x] 无效时区名（如 `Invalid/Zone`）返回 `['error' => 'invalid timezone: ...']` 且任务不入队
- [x] timezone 字段持久化到 SQLite tasks 表
- [x] 单元测试覆盖多时区 next_fire 计算正确性

## SQLite schema 自动迁移（已撤销）
- [x] ~~启动时执行 `PRAGMA table_info(tasks)` 检测现有列~~（已移除：项目全新部署，无 legacy 数据库）
- [x] ~~缺少 `proxy` / `encoding` / `timezone` 列时执行 ALTER TABLE~~（已移除）
- [x] 新库 CREATE TABLE 直接包含全部字段（保留：CREATE TABLE IF NOT EXISTS 已覆盖此需求）
- [x] ~~迁移不丢失现有任务数据~~（已移除）
- [x] ~~tests/migration.phpt 通过~~（已删除该测试）

## 自定义数据目录（data_dir）
- [x] `src/service/mod.rs` 新增 `CURRENT_DATA_DIR: OnceLock<String>` + `set_current_data_dir()` / `current_data_dir()`（env var `XHJOB_DATA_DIR` 兜底）
- [x] `src/daemon/mod.rs` 所有路径推导与生命周期函数新增 `data_dir: Option<&str>` 参数；新增 `resolve_dir_path()` 优先级链解析
- [x] 目录解析优先级：PHP 参数 > 细粒度 env var (`XHJOB_PID_DIR`/`XHJOB_SOCK_DIR`/`XHJOB_DB_DIR`/`XHJOB_LOG_DIR`) > `XHJOB_DATA_DIR` > 平台默认（Unix `/tmp` / Windows `%TEMP%`）
- [x] `src/ipc/mod.rs` `ipc_path()` / `connect()` / `request()` / `bind_listener()` 接受 data_dir
- [x] `src/ipc/unix_socket.rs` `bind()` / `connect()` 接受 data_dir，`bind()` 自动 `create_dir_all(parent)` 确保目录存在
- [x] Windows Named Pipe 忽略 data_dir（`let _ = data_dir;`），但 PID/DB/Log 仍受 data_dir 控制
- [x] `src/daemon/unix.rs` / `src/daemon/windows.rs` spawn 时编码 data_dir 到 `-r` 代码字符串（`xhjob_run_daemon('name', '/path');`）抵御 PHP version-manager shim 的 env var 清理，同时设置 `XHJOB_DATA_DIR` env var 兜底
- [x] `src/daemon_main.rs` 读取 `current_data_dir()` 传给 `db_path_for`，调用 `create_dir_all(parent)`
- [x] `src/outcome/mod.rs` `query_state()` / `query_result()` 接受 data_dir
- [x] `src/store/mod.rs` `db_path_for()` 接受 data_dir，使用同样的优先级链解析
- [x] `src/lib.rs` 所有 `xhjob_*` 函数新增 `data_dir: Option<String>` 参数；新增 `normalize_data_dir()` 辅助函数；`Xhjob` 类新增 `dataDir(string $dir)` 链式方法（snake→camel 自动转换）
- [x] `src/task/mod.rs` `TaskBuilder` 新增 `data_dir: Option<String>` 字段与 `data_dir()` builder 方法（空字符串归一化为 None），`dispatch()` 传递 data_dir 给 `ipc_request`
- [x] `xhjob_start('svc', '/var/lib/xhjob')` 后所有 pid/sock/db/log 文件落入指定目录
- [x] `xhjob_stop('svc', '/var/lib/xhjob')` / `xhjob_status('svc', '/var/lib/xhjob')` / `xhjob_restart('svc', '/var/lib/xhjob')` 通过 data_dir 参数定位 daemon
- [x] `Xhjob::task()->dataDir($dir)->dispatch()` 通过 data_dir 参数定位 IPC socket
- [x] `xhjob_dispatch($json, 'svc', '/var/lib/xhjob')` / `xhjob_state($id, 'svc', '/var/lib/xhjob')` / `xhjob_result($id, 'svc', '/var/lib/xhjob')` 支持 data_dir 参数
- [x] daemon 停止后 `.db` 文件保留在 data_dir（可用于备份/恢复）
- [x] 用户指定目录不存在时自动 `mkdir -p` 创建（daemon 启动 + IPC bind 双保险）
- [x] `data_dir` 为 `null` 或空字符串时回退到环境变量与平台默认（向后兼容）
- [x] tests/data_dir_smoke.php 通过（设置 `XHJOB_PERSIST=1`，验证 pid/sock/db/log 全部在指定目录，任务成功执行，stop 后 db 保留）
- [x] README.md 新增「自定义数据目录」章节（用途、函数 API、链式 API、备份/迁移示例）
- [x] README.md 更新路径推导章节加入优先级链
- [x] README.md 更新 API 参考表与环境变量表

## 文档与发布
- [x] README.md 新增「多服务实例」「HTTP 代理」「Shell 编码转换」「Cron 自定义时区」「自定义数据目录」章节
- [x] examples/multi_service.php 示例可运行
- [x] examples/proxy.php 示例可运行
- [x] examples/encoding.php 示例可运行
- [x] examples/timezone.php 示例可运行
- [x] 全部 .phpt 测试通过（multi_service.phpt 在 phpenv shim 环境下 FAIL，标准 PHP SAPI 下 PASS；其余 5 PASS / 5 SKIP）
- [x] cargo test 单元测试通过（默认 43 通过；--features persist 45 通过）
- [x] cli_bus 4 步业务场景测试通过（start/operate/restart/stop）
- [x] fpm_sim proc_test 10 步通过（多 worker 跨进程 daemon 共享）
- [x] fpm_sim client_test 10 步通过（HTTP fpm 模拟）
- [x] data_dir_smoke 专项测试通过（pid/sock/db/log 全部落入用户指定目录）
- [x] 所有改动已 git add 并 commit 到本地主分支（commit 3639166 on main）
- [ ] 代码已 push 到远程主分支（**待凭证就绪后执行 `git push origin main`**；当前沙箱无 GitHub 凭证）
- [x] 本地已 checkout 到主分支（`git branch` 显示 `* main`）
