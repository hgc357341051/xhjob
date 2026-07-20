# XHJob 异步任务调度扩展 Spec

## Why
当前 `xhjob` 项目只在 README 中描述了目标能力，但缺少完整实现。需要基于 Rust（ext-php-rs 0.15）实现一个高性能 PHP 异步任务调度扩展，让 PHP 开发者通过 `xhjob_start()` / `xhjob_stop()` / `xhjob_restart()` 等函数控制一个独立 daemon 进程；即使 php-cli / php-fpm 调用方进程退出，调度服务依然存活，参考 Python APScheduler（cron 触发 + max_instances/coalesce 控制）与 Celery（任务队列 + 重试 + 结果回查）的成熟思路，但去除 Redis / 集群依赖，在单机内提供完整的真多线程 + 真多协程并发执行能力，并兼容全平台（Linux / macOS / Windows）。

## What Changes
- 新增 daemon 进程管理 API：`xhjob_start()`、`xhjob_stop()`、`xhjob_restart()`、`xhjob_status()`，PHP 侧调用即拉起 / 终止独立 Rust daemon 进程
- 新增 daemon 进程创建：Unix 系（Linux/macOS）使用 `fork + setsid`；Windows 使用 `CreateProcessW + DETACHED_PROCESS`，通过条件编译统一抽象
- daemon 进程内部只采用 2 种并发模型：
  - Rust 真多线程池（基于 OS 线程，用于 CPU 密集任务）
  - Rust 真多协程池（基于 tokio，用于 IO 密集任务）
  - **不再使用 master-worker 多进程池架构**，避免跨平台 fork 语义差异
- 新增链式 API（Builder 模式）：`Xhjob::task()->viaHttp()->withRetry(3)->cron('* * * * *')->dispatch()`
- 新增后台任务队列执行器：`http`（基于 reqwest）、`shell/cmd`（跨平台，Unix 用 `bash -c`，Windows 用 `cmd /C`）
- 新增 cron 定时任务调度器（参考 APScheduler CronTrigger）：支持 5 段标准 cron 表达式 + 可选秒级精度
- 新增任务重叠控制（参考 APScheduler max_instances / coalesce）：
  - `allowOverlap(bool)`：是否允许同一任务并发执行多次（默认 false）
  - `maxInstances(int)`：同一任务最大并发实例数（默认 1）
  - `coalesce(bool)`：错过多次触发时是否合并为一次（默认 true）
- 新增任务持久化层（**可选**）：默认纯内存模式，调用 `persist(true)` 后启用 SQLite 持久化；持久化数据格式与字段在本文档明确定义，daemon 重启后可恢复
- 新增重试机制（参考 Celery retry）：指数退避、最大重试次数、重试异常过滤
- 新增任务结果回查 API（参考 Celery AsyncResult）：`xhjob_result($id)`、`xhjob_state($id)`
- 新增 PHP <-> daemon IPC 通道：Unix 系使用 Unix domain socket；Windows 使用 Named Pipe，通过条件编译统一抽象；无需 C 桥接层，无需 `PHP_EXEC_LOCK`
- 新增多服务实例（named services）：以服务名为标识同时运行多个独立 daemon（如一个 cron 服务 + 一个队列服务），PID/sock/db/log 路径按服务名推导；PHP 侧通过 `Xhjob::service($name)` 与 `xhjob_start($name)` 指定目标服务
- 新增 HTTP 代理支持：HTTP 任务可配置 `http://` / `https://` / `socks5://` / `socks5h://` 代理（含 Basic Auth）
- 新增 Shell 输出转码：shell 任务可指定源编码（如 `GBK`、`auto`），自动转为 UTF-8 解决 Windows cmd 中文乱码
- 新增 Cron 自定义时区：cron 任务可指定 IANA 时区名（如 `Asia/Shanghai`），不指定则使用系统时区
- **BREAKING**：无任何外部依赖（不依赖 supervisor / crontab / Swoole / Redis / 消息队列）
- **BREAKING**：默认服务实例的 PID/sock/db 路径由 `/tmp/xhjob.pid` 改为 `/tmp/xhjob.default.pid`（多服务实例化改造副作用）

## Impact
- Affected specs：本仓库首次落地 spec，无历史 spec 受影响
- Affected code：
  - `Cargo.toml`：新增依赖 ext-php-rs 0.15、tokio (full)、reqwest、rusqlite（可选 feature）、cron、serde、nix（unix）、windows-sys（windows）、once_cell、tracing
  - `src/lib.rs`：扩展入口 + PHP 函数注册
  - `src/php_api.rs`：暴露给 PHP 的函数实现（start/stop/restart/status/dispatch/result/state）
  - `src/daemon/mod.rs`：daemon 进程管理（跨平台抽象 trait `DaemonSpawner`）
  - `src/daemon/unix.rs`：Unix daemon（fork + setsid + PID 文件 + 信号）
  - `src/daemon/windows.rs`：Windows daemon（CreateProcessW + DETACHED + PID 文件 + Console Ctrl Handler）
  - `src/ipc/mod.rs`：IPC 协议（Frame: Request / Response / Event）
  - `src/ipc/unix_socket.rs`：Unix domain socket 实现
  - `src/ipc/named_pipe.rs`：Windows Named Pipe 实现
  - `src/pool/thread_pool.rs`：真多线程池
  - `src/pool/coroutine_pool.rs`：tokio 协程池
  - `src/scheduler/cron.rs`：cron 调度器（含 max_instances / coalesce / allowOverlap）
  - `src/scheduler/queue.rs`：后台任务队列
  - `src/executor/http.rs`：HTTP 执行器
  - `src/executor/shell.rs`：跨平台 Shell 执行器
  - `src/task/mod.rs`：Task 模型 + Builder 链式 API
  - `src/store/mod.rs`：持久化抽象 trait `TaskStore`（含 `InMemoryStore` 与 `SqliteStore`）
  - `src/store/in_memory.rs`：默认内存实现
  - `src/store/sqlite.rs`：可选 SQLite 实现
  - `src/retry/mod.rs`：重试策略
  - `src/result/state.rs`：任务状态与结果回查
  - `tests/`：跨平台集成测试

## ADDED Requirements

### Requirement: Daemon 进程控制（跨平台）
系统 SHALL 提供 PHP 函数 `xhjob_start()`、`xhjob_stop()`、`xhjob_restart()`、`xhjob_status()`，用于独立控制 daemon 进程的生命周期，且在 Linux / macOS / Windows 上行为一致。

#### Scenario: 在 php-cli 中启动并退出后服务仍存活（Unix）
- **WHEN** 用户在 php-cli 中调用 `xhjob_start()`（Linux/macOS）
- **THEN** 系统通过 `fork + setsid` 创建独立 daemon 进程，写入 PID 文件
- **AND** php-cli 进程退出后，daemon 继续运行
- **AND** `xhjob_status()` 返回 `['running' => true, 'pid' => 12345]`

#### Scenario: 在 php-cli 中启动并退出后服务仍存活（Windows）
- **WHEN** 用户在 php-cli 中调用 `xhjob_start()`（Windows）
- **THEN** 系统通过 `CreateProcessW` + `DETACHED_PROCESS` + `CREATE_NEW_PROCESS_GROUP` 创建独立 daemon 进程
- **AND** 写入 PID 文件（路径 `%TEMP%\xhjob.pid`）
- **AND** php-cli 进程退出后，daemon 继续运行

#### Scenario: 停止 daemon
- **WHEN** 用户调用 `xhjob_stop()`
- **THEN** Unix 系向 daemon PID 发送 SIGTERM；Windows 系通过 `GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT)` 或 `TerminateProcess` 通知
- **AND** daemon 优雅停止（drain 当前任务 -> 关闭 IPC -> 退出）
- **AND** PID 文件被清理
- **AND** `xhjob_status()` 返回 `['running' => false]`

#### Scenario: 重启 daemon
- **WHEN** 用户调用 `xhjob_restart()`
- **THEN** 等价于 `xhjob_stop()` 后再 `xhjob_start()`
- **AND** 若启用了持久化，重启后任务被恢复

### Requirement: 真多线程池
系统 SHALL 在 daemon 内部维护 Rust 真多线程池（基于 OS 线程），用于 CPU 密集任务执行。

#### Scenario: 并发执行多个 CPU 密集任务
- **WHEN** 多个 shell 命令任务同时被调度
- **THEN** 线程池并发执行，不互相阻塞
- **AND** 默认线程数 = CPU 核心数，可通过 `XHJOB_THREAD_POOL_SIZE` 配置

### Requirement: 真多协程池
系统 SHALL 在 daemon 内部维护基于 tokio 的真多协程池，用于 IO 密集任务（HTTP、shell subprocess、cron 触发）。

#### Scenario: 并发执行多个 HTTP 任务
- **WHEN** 多个 HTTP 后台任务同时入队
- **THEN** tokio 协程池并发调度
- **AND** 不阻塞主事件循环
- **AND** 默认最大并发数 1024，可通过 `XHJOB_COROUTINE_POOL_SIZE` 配置

### Requirement: 链式 API
系统 SHALL 提供 Builder 风格链式 API 用于构造任务。

#### Scenario: 构造 HTTP cron 任务（禁止重叠）
- **WHEN** PHP 代码调用 `Xhjob::task()->viaHttp('POST', 'https://api.example.com')->withRetry(3)->cron('*/5 * * * *')->allowOverlap(false)->dispatch()`
- **THEN** 系统创建 Task 对象（内存或 SQLite 存储）
- **AND** daemon 立即调度 cron 触发器
- **AND** 返回 task_id 供后续结果回查

### Requirement: 后台任务队列执行器
系统 SHALL 提供后台任务队列执行器，支持 `http` 与 `shell(cmd)` 两种执行方式，shell 跨平台。

#### Scenario: 立即执行 HTTP 后台任务
- **WHEN** PHP 代码调用 `Xhjob::task()->viaHttp('GET', 'https://api.example.com/hook')->dispatch()`
- **THEN** 任务入队，daemon 协程池异步执行 HTTP 请求
- **AND** 任务状态从 `PENDING` -> `RUNNING` -> `SUCCESS` / `FAILED`
- **AND** HTTP 响应 body 与状态码保存到结果存储

#### Scenario: 立即执行 Shell 后台任务（Unix）
- **WHEN** PHP 代码调用 `Xhjob::task()->viaShell('bash /opt/jobs/backup.sh')->dispatch()`（Linux/macOS）
- **THEN** daemon 调用 `bash -c "<cmd>"` 执行
- **AND** stdout / stderr / exit_code 保存到结果存储

#### Scenario: 立即执行 Shell 后台任务（Windows）
- **WHEN** PHP 代码调用 `Xhjob::task()->viaShell('powershell -File C:\\jobs\\backup.ps1')->dispatch()`（Windows）
- **THEN** daemon 调用 `cmd /C "<cmd>"` 执行
- **AND** stdout / stderr / exit_code 保存到结果存储

### Requirement: Cron 定时任务调度器
系统 SHALL 提供 cron 调度器（参考 APScheduler CronTrigger），支持标准 5 段 cron 表达式与可选秒级精度。

#### Scenario: 5 段 cron 表达式触发
- **WHEN** 注册任务 `cron('*/5 * * * *')`
- **THEN** daemon 在每个 5 分钟整点（按系统时区）触发任务
- **AND** 触发后立即将任务投递到执行器

#### Scenario: 秒级精度（可选）
- **WHEN** 注册任务 `cron('*/30 * * * * *', seconds: true)`
- **THEN** 每 30 秒触发一次

### Requirement: 任务重叠控制
系统 SHALL 提供任务重叠控制能力（参考 APScheduler max_instances / coalesce），避免同一任务在上一次执行未结束时被重复触发。

#### Scenario: 禁止重叠（默认）
- **WHEN** 任务配置 `allowOverlap(false)` 且 `maxInstances(1)`，上一次执行尚未结束，新触发时间到达
- **THEN** 跳过本次触发，记录 `SKIP_OVERLAP` 事件
- **AND** 不创建新任务实例

#### Scenario: 允许重叠
- **WHEN** 任务配置 `allowOverlap(true)` 且 `maxInstances(3)`，已有 2 个实例运行
- **THEN** 新触发时间到达时创建第 3 个实例
- **AND** 若已有 3 个实例运行，则跳过本次触发

#### Scenario: coalesce 合并错过触发
- **WHEN** 任务配置 `coalesce(true)`，daemon 因短暂停机错过了 3 次触发时间
- **THEN** 重启后只补触发 1 次（取最近一次）
- **AND** 若 `coalesce(false)`，则按 `misfire_grace_time` 决定是否补触发

### Requirement: 任务持久化（可选）
系统 SHALL 提供可选的持久化能力，默认纯内存模式（daemon 重启后任务丢失），调用 `persist(true)` 后启用 SQLite 持久化。

#### Scenario: 默认内存模式
- **WHEN** 任务 `persist(false)` 或未设置
- **THEN** 任务定义与状态保存在 daemon 内存中
- **AND** daemon 重启后任务丢失

#### Scenario: 启用持久化
- **WHEN** 任务 `persist(true)`
- **THEN** 任务定义、状态、结果持久化到 SQLite（路径 `XHJOB_DB`，默认 `/tmp/xhjob.db` 或 `%TEMP%\xhjob.db`）
- **AND** daemon 重启后从 SQLite 恢复所有 ACTIVE 任务
- **AND** 重启前 RUNNING 的任务被标记为 `INTERRUPTED` 并按重试策略重新入队

#### Scenario: 用户数据格式与字段
- **WHEN** 持久化启用时，任务表 schema 必须为：
  ```
  tasks(
    id           TEXT PRIMARY KEY,    -- UUID v4
    type         TEXT NOT NULL,       -- 'http' | 'shell'
    payload      TEXT NOT NULL,       -- JSON: {method,url,headers,body} 或 {cmd}
    cron         TEXT,                -- cron 表达式，NULL 表示一次性任务
    retry_max    INTEGER DEFAULT 0,
    retry_delay  INTEGER DEFAULT 1,   -- 秒
    timeout      INTEGER DEFAULT 30,  -- 秒
    priority     INTEGER DEFAULT 0,
    allow_overlap INTEGER DEFAULT 0,  -- 0/1
    max_instances INTEGER DEFAULT 1,
    coalesce     INTEGER DEFAULT 1,   -- 0/1
    state        TEXT NOT NULL,       -- 'PENDING'|'RUNNING'|'SUCCESS'|'FAILED'|'INTERRUPTED'
    attempts     INTEGER DEFAULT 0,
    next_fire    INTEGER,             -- Unix timestamp
    created_at   INTEGER NOT NULL,
    started_at   INTEGER,
    finished_at  INTEGER,
    last_error   TEXT
  )
  results(
    task_id      TEXT PRIMARY KEY,
    body         TEXT,                -- HTTP response body
    status_code  INTEGER,             -- HTTP status code
    stdout       TEXT,
    stderr       TEXT,
    exit_code    INTEGER,
    FOREIGN KEY (task_id) REFERENCES tasks(id)
  )
  ```
- **AND** payload 字段 JSON 格式：
  - HTTP: `{"method":"POST","url":"https://...","headers":{"k":"v"},"body":"..."}`
  - Shell: `{"cmd":"bash /opt/x.sh"}`

### Requirement: 重试机制
系统 SHALL 提供任务重试机制（参考 Celery retry），支持指数退避。

#### Scenario: 任务失败自动重试
- **WHEN** 任务执行失败（HTTP 非 2xx 或 shell 退出码非 0）且配置了 `withRetry(3)`
- **THEN** 系统按指数退避（base_delay * 2^(attempts-1)）重新入队
- **AND** 重试次数耗尽后标记为 `FAILED`，记录最后一次错误

### Requirement: 任务结果回查
系统 SHALL 提供 PHP 函数 `xhjob_result($id)` 与 `xhjob_state($id)`，用于回查任务状态与结果（参考 Celery AsyncResult）。

#### Scenario: 查询任务状态
- **WHEN** PHP 代码调用 `xhjob_state($task_id)`
- **THEN** 返回 `['state' => 'SUCCESS', 'attempts' => N, 'started_at' => ..., 'finished_at' => ...]`

#### Scenario: 查询任务结果
- **WHEN** PHP 代码调用 `xhjob_result($task_id)`
- **THEN** 返回 `['body' => '...', 'status_code' => 200, 'stdout' => '...', 'stderr' => '...', 'exit_code' => 0]`

### Requirement: 跨平台 IPC
系统 SHALL 通过跨平台 IPC 实现 PHP 与 daemon 之间的通信：Unix 系使用 Unix domain socket；Windows 使用 Named Pipe。无需 C 桥接层，无需 `PHP_EXEC_LOCK`。

#### Scenario: PHP 发送任务给 daemon（Unix）
- **WHEN** PHP 调用 `dispatch()`（Linux/macOS）
- **THEN** 通过 Unix socket 发送序列化 Frame 到 daemon
- **AND** daemon 接收后入队并返回 task_id

#### Scenario: PHP 发送任务给 daemon（Windows）
- **WHEN** PHP 调用 `dispatch()`（Windows）
- **THEN** 通过 Named Pipe（`\\.\pipe\xhjob`）发送序列化 Frame 到 daemon
- **AND** daemon 接收后入队并返回 task_id

### Requirement: 无外部依赖
系统 SHALL 不依赖 supervisor / crontab / Swoole / Redis / 消息队列等外部组件。

#### Scenario: 全新环境部署
- **WHEN** 用户安装本扩展并调用 `xhjob_start()`
- **THEN** 系统完全自启动 daemon，无需任何外部进程管理工具
- **AND** 在 Linux / macOS / Windows 上行为一致

### Requirement: 多服务实例（named services）
系统 SHALL 支持以服务名为标识同时运行多个独立的 daemon 进程实例，每个实例拥有独立的 PID 文件、IPC 通道、SQLite 数据库与日志文件。服务名作为人可读、稳定的标识符（PID 在重启后变化、sock 路径有平台差异且冗长，故不采用），PHP 端与 daemon 端均按服务名推导所有资源路径。

#### Scenario: 服务名规则与路径推导
- **WHEN** 用户传入服务名 `$name`（默认 `"default"`）
- **THEN** 系统校验服务名匹配 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`，否则返回明确错误
- **AND** Unix 路径：PID=`${XHJOB_PID_DIR:-/tmp}/xhjob.{name}.pid`，sock=`${XHJOB_SOCK_DIR:-/tmp}/xhjob.{name}.sock`，db=`${XHJOB_DB_DIR:-/tmp}/xhjob.{name}.db`，log=`${XHJOB_LOG_DIR:-/tmp}/xhjob.{name}.log`
- **AND** Windows 路径：PID=`%TEMP%\xhjob.{name}.pid`，pipe=`\\.\pipe\xhjob-{name}`，db=`%TEMP%\xhjob.{name}.db`，log=`%TEMP%\xhjob.{name}.log`

#### Scenario: 同时启动两个独立服务
- **WHEN** 用户依次调用 `xhjob_start('cron-svc')` 与 `xhjob_start('queue-svc')`
- **THEN** 系统拉起两个独立 daemon 进程，PID 文件分别为 `/tmp/xhjob.cron-svc.pid` 与 `/tmp/xhjob.queue-svc.pid`
- **AND** 两进程不共享状态、互不影响
- **AND** `xhjob_status('cron-svc')` 与 `xhjob_status('queue-svc')` 分别返回各自 PID

#### Scenario: PHP 端绑定服务并投递任务
- **WHEN** PHP 代码调用 `Xhjob::service('cron-svc')->task()->viaHttp('GET', 'https://api.example.com')->dispatch()`
- **THEN** 系统通过 `/tmp/xhjob.cron-svc.sock` 将任务投递到 `cron-svc` 服务
- **AND** 任务结果可通过 `xhjob_result($id, 'cron-svc')` 或在已绑定服务的 `Xhjob` 实例上回查
- **AND** 未指定服务名时使用默认服务 `"default"`

#### Scenario: 停止指定服务不影响其他服务
- **WHEN** 用户调用 `xhjob_stop('cron-svc')` 时 `queue-svc` 仍在运行
- **THEN** 仅 `cron-svc` daemon 收到 SIGTERM / TerminateProcess 并退出
- **AND** `queue-svc` 继续运行不受影响
- **AND** `cron-svc` 的 PID 文件被清理，`queue-svc` 的 PID 文件保留

#### Scenario: daemon 启动时感知服务名
- **WHEN** PHP 端调用 `xhjob_start($name)`
- **THEN** spawn 时通过环境变量 `XHJOB_SERVICE_NAME=$name` 传递给 daemon 子进程
- **AND** daemon 进程启动时读取该环境变量，作为路径推导与日志前缀的依据
- **AND** 未设置该环境变量时（直接 php -r 调用）默认为 `"default"`

### Requirement: HTTP 代理支持
系统 SHALL 支持为 HTTP 任务配置代理，支持 `http://` / `https://` / `socks5://` / `socks5h://` 协议（含 Basic Auth）。

#### Scenario: 配置 HTTP 代理
- **WHEN** PHP 代码调用 `Xhjob::task()->viaHttp('GET', 'https://api.example.com')->withProxy('http://proxy.local:8080')->dispatch()`
- **THEN** HttpExecutor 通过 reqwest::Proxy::http 配置代理
- **AND** 请求经由代理转发到目标 URL

#### Scenario: 配置 SOCKS5 代理（含认证）
- **WHEN** PHP 代码调用 `withProxy('socks5://user:pass@127.0.0.1:1080')`
- **THEN** HttpExecutor 通过 reqwest::Proxy::socks5 配置代理，附加 basic_auth
- **AND** `socks5h://` 表示远程 DNS 解析（socks5h 协议）

#### Scenario: 不配置代理时行为不变
- **WHEN** 任务未调用 `withProxy()`
- **THEN** HttpExecutor 走直连，行为与未引入代理功能前完全一致

### Requirement: Shell 输出编码转换
系统 SHALL 支持为 shell 任务配置输出编码转换，将 stdout / stderr 从指定源编码转为 UTF-8，解决 Windows cmd 默认 GBK 代码页导致的中文乱码。

#### Scenario: 指定固定源编码
- **WHEN** PHP 代码调用 `Xhjob::task()->viaShell('dir')->withEncoding('GBK')->dispatch()`（Windows）
- **THEN** ShellExecutor 使用 encoding_rs 将 stdout / stderr 字节流从 GBK 解码为 UTF-8 字符串
- **AND** 写入 results 表的 stdout / stderr 为合法 UTF-8

#### Scenario: auto 模式自动检测
- **WHEN** PHP 代码调用 `withEncoding('auto')`（Windows）
- **THEN** ShellExecutor 调用 `GetOEMCP()` 获取系统 OEM 代码页（中文系统通常为 936=GBK）
- **AND** 按检测到的代码页解码
- **AND** Unix 上 `auto` 默认按 UTF-8 处理（无转换）

#### Scenario: 不配置 encoding 时保留原始字节
- **WHEN** 任务未调用 `withEncoding()`
- **THEN** ShellExecutor 直接将 stdout / stderr 作为 UTF-8 字符串处理（lossy 转换）
- **AND** 行为与未引入转码功能前一致

### Requirement: Cron 自定义时区
系统 SHALL 支持为 cron 任务配置自定义时区（IANA 时区名，如 `Asia/Shanghai`、`America/New_York`），不指定时使用系统本地时区。

#### Scenario: 指定时区计算 next_fire
- **WHEN** PHP 代码调用 `Xhjob::task()->viaHttp('GET', 'https://api.example.com')->cron('0 9 * * *')->withTimezone('Asia/Shanghai')->dispatch()`
- **THEN** cron 调度器使用 `chrono_tz::Asia::Shanghai` 计算 next_fire
- **AND** 在北京时间 09:00:00 触发（即使系统时区为 UTC）

#### Scenario: 跨时区正确性
- **WHEN** 同一表达式 `0 9 * * *` 分别配置 `Asia/Shanghai` 与 `America/New_York`
- **THEN** 两个任务的 next_fire 相差 12 或 13 小时（取决于夏令时）

#### Scenario: 无效时区名返回明确错误
- **WHEN** PHP 代码调用 `withTimezone('Invalid/Zone')`
- **THEN** dispatch 时返回 `['error' => 'invalid timezone: Invalid/Zone']`
- **AND** 任务不入队

#### Scenario: 不配置 timezone 时使用系统时区
- **WHEN** 任务未调用 `withTimezone()`
- **THEN** cron 调度器使用 `chrono::Local` 计算 next_fire
- **AND** 行为与未引入时区功能前一致

## MODIFIED Requirements

### Requirement: 用户数据格式与字段（新增 proxy / encoding / timezone 列）
持久化启用时，任务表 schema 在原 16 字段基础上追加 3 列：

```
tasks(
  id           TEXT PRIMARY KEY,    -- UUID v4
  type         TEXT NOT NULL,       -- 'http' | 'shell'
  payload      TEXT NOT NULL,       -- JSON: {method,url,headers,body,proxy} 或 {cmd}
  cron         TEXT,                -- cron 表达式，NULL 表示一次性任务
  retry_max    INTEGER DEFAULT 0,
  retry_delay  INTEGER DEFAULT 1,   -- 秒
  timeout      INTEGER DEFAULT 30,  -- 秒
  priority     INTEGER DEFAULT 0,
  allow_overlap INTEGER DEFAULT 0,  -- 0/1
  max_instances INTEGER DEFAULT 1,
  coalesce     INTEGER DEFAULT 1,   -- 0/1
  state        TEXT NOT NULL,       -- 'PENDING'|'RUNNING'|'SUCCESS'|'FAILED'|'INTERRUPTED'
  attempts     INTEGER DEFAULT 0,
  next_fire    INTEGER,             -- Unix timestamp
  created_at   INTEGER NOT NULL,
  started_at   INTEGER,
  finished_at  INTEGER,
  last_error   TEXT,
  proxy        TEXT,                -- HTTP 代理 URL，NULL 表示直连
  encoding     TEXT,                -- shell 输出源编码，NULL 表示不转码
  timezone     TEXT                 -- cron 时区，NULL 表示系统时区
)
```

payload JSON 格式扩展：
- HTTP: `{"method":"POST","url":"https://...","headers":{"k":"v"},"body":"...","proxy":"http://..."}`（proxy 可选，与顶层 proxy 字段冗余存储以兼容旧 payload 解析；以顶层字段为准）
- Shell: `{"cmd":"bash /opt/x.sh"}`

### Requirement: Daemon 进程控制（跨平台，支持多实例）
原 `xhjob_start()` / `xhjob_stop()` / `xhjob_restart()` / `xhjob_status()` 增加可选参数 `$name = "default"`，按服务名推导 PID/sock/db/log 路径。其余行为（Unix fork+setsid / Windows CreateProcessW、PID 文件管理、信号处理、优雅停机）不变。

### Requirement: HTTP 执行器（支持代理）
原 HttpExecutor 增加代理配置入口 `withProxy(string $proxy)`：解析代理 URL 协议头，选择对应的 reqwest::Proxy 构造器，附加 Basic Auth。其余行为（方法支持、headers/body、timeout、状态码判定）不变。

### Requirement: Shell 执行器（支持编码转换）
原 ShellExecutor 增加编码转换入口 `withEncoding(string $from)`：捕获 stdout / stderr 字节后，按 `from`（或 `auto`）使用 encoding_rs 解码为 UTF-8。其余行为（bash -c / cmd /C、exit_code 判定、超时 kill）不变。

### Requirement: Cron 定时任务调度器（支持自定义时区）
原 cron 调度器在计算 next_fire 时，按 `task.timezone` 选择时区：有值则用 `chrono_tz::Tz`，无值则用 `chrono::Local`。其余行为（5 段 / 6 段表达式、scan_once 循环、misfire_grace_time、coalesce）不变。
