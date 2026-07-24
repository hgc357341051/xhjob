---
title: 生产实战：CLI 与 FPM 共用服务连接
parent: 生产实战
nav_order: 53
---

# 生产实战：CLI 与 FPM 共用服务连接

本篇解答一个在生产部署中最常见的问题：**Web 请求（PHP-FPM）和命令行脚本（CLI）如何连到同一个 xhjob daemon？** 答案是——只要二者使用相同的 `service_name` + `data_dir`，就会解析到同一组 socket / pid / db / log 文件，从而连到同一个 daemon 进程。daemon 本身是一个**独立的 Rust 进程**（Unix 下 double-fork + setsid 脱离父进程），CLI 和 FPM 都只是它的 **IPC 客户端**。

## 共用原理

```
                    ┌──────────────────────────────────────────────────┐
                    │  独立 Rust daemon 进程（double-fork + setsid）    │
                    │  service_name='app-svc'  data_dir='/var/lib/xhjob'│
                    │  ┌────────────┐  ┌────────────┐  ┌────────────┐  │
                    │  │ scheduler  │  │ executor   │  │ store      │  │
                    │  │ (cron/...) │  │ (pool)     │  │ (SQLite)   │  │
                    │  └─────┬──────┘  └────────────┘  └────────────┘  │
                    │        │                                         │
                    │  ┌─────▼──────────────────────────┐              │
                    │  │ IPC listener                   │              │
                    │  │ /var/lib/xhjob/xhjob.app-svc.sock              │
                    │  └─────▲──────────────────────────┘              │
                    └────────┼─────────────────────────────────────────┘
                             │ Unix domain socket（短连接，length-prefixed JSON）
            ┌────────────────┼────────────────┐
            │                │                │
   ① FPM 请求（dispatch）  ② CLI 查状态      ③ CLI worker（轮询 result）
   xhjob_dispatch(...)      xhjob_state(...)  xhjob_result(...)
   同名 + 同 data_dir        同名 + 同 data_dir 同名 + 同 data_dir
```

关键事实：

- daemon 是**独立进程**，不依附于拉起它的 PHP 进程。即使拉起它的 FPM 请求 / CLI 脚本退出，daemon 仍继续运行。
- 所有 `xhjob_*` PHP 函数（`dispatch` / `state` / `result` / `start` / `stop` …）都是 **IPC 客户端**：通过 Unix domain socket（Windows 为 Named Pipe）向 daemon 发一次短连接请求。
- 客户端解析 socket 路径的依据**只有两个**：`service_name` 与 `data_dir`。CLI 和 FPM 传入相同的值，就会解析到同一个 socket 文件，连到同一个 daemon。

源码依据（`src/ipc/mod.rs`）：

```rust
/// Unix: `<dir>/xhjob.{name}.sock`
/// Windows: `\\.\pipe\xhjob-{name}`（data_dir 在 Windows 上被忽略）
pub fn ipc_path(service_name: &str, data_dir: Option<&str>) -> String { ... }
```

## daemon 是独立进程：double-fork + setsid

daemon 不是 PHP 进程内的线程，而是一个**全新拉起的 PHP 子进程**（重新 re-exec 当前 PHP binary），其内部通过扩展启动钩子直接进入 `daemon_main()`。这样做的核心原因：PHP 进程可能已初始化 tokio runtime（来自此前的 `xhjob_dispatch` / `xhjob_state` 调用），而 tokio runtime 状态**不是 fork-safe** 的，所以不能在进程内 `fork + setsid`。

启动流程（`src/daemon/unix.rs::spawn_via_double_fork`）：

1. 定位当前 PHP binary（`current_exe` → `$_` → PATH `php`）。
2. 构造 `-r` 代码字符串：`xhjob_run_daemon('<service_name>', '<data_dir>');`。**service_name 与 data_dir 通过命令行参数传递**，不依赖 env var——因为某些 PHP SAPI / 版本管理器（如 phpenv）shim 在 re-exec 时会清理 `Command::env()` 注入的环境变量，命令行参数则会被保留。
3. 通过 `pre_exec` 钩子调用 `setsid()`，使子进程成为新会话组长，脱离任何 tty。
4. 子进程以 `-d extension=xhjob.so -r <code>` 启动，扩展启动时检测到 `XHJOB_DAEMON_MODE=1`，直接调用 `daemon_main()`，并写入 PID 文件（双行格式，见下文）。

```rust
// 关键：service_name + data_dir 编码进 -r 命令行参数，跨 re-exec 存活
let code = format!("xhjob_run_daemon('{}', '{}');", escaped_name, escaped_dir);
let mut cmd = Command::new(&exe);
cmd.arg("-d").arg("extension=xhjob.so");
cmd.arg("-r").arg(&code);
cmd.env("XHJOB_DAEMON_MODE", "1");          // env 仅作向后兼容兜底
unsafe {
    cmd.pre_exec(|| { /* setsid() 脱离 tty */ });
}
let _child = cmd.spawn()?;
```

> 这就是为什么 `xhjob_server.php` 启动 daemon 后可以 `exit(0)` 而 daemon 依然存活——daemon 是 detached grandchild，与启动它的 PHP 进程已无父子关系。

## 启动 daemon 的两种方式

### 方式一：CLI 长驻启动（推荐生产用法）

用一个独立 CLI 脚本 / systemd unit 启动 daemon，FPM 与 CLI 都只做客户端调用。这是**最干净的生产拓扑**：daemon 生命周期与 Web 流量解耦，FPM 重启不影响 daemon。

```php
<?php
// === /opt/xhjob/bin/start-daemon.php ===
// 由 systemd 调用：php /opt/xhjob/bin/start-daemon.php
use Xhjob\XhjobService;

$svc = new XhjobService('app-svc', '/var/lib/xhjob');
$svc->ensureRunning();          // 幂等：已运行则直接返回
$status = $svc->status();
echo "daemon ready, pid={$status['pid']}\n";
```

`XhjobService::ensureRunning()` 内部先查 `status()`，未运行才 `start()`；`start()` 调 `xhjob_start` 后轮询等待 daemon 真正进入 running 状态（含 IPC socket 就绪），再返回 PID。

### 方式二：FPM 请求内拉起（首次访问自举）

FPM 请求内调用 `ensureRunning()`，若 daemon 未运行则由 FPM worker 拉起。daemon 拉起后即脱离 FPM，FPM 请求可继续派发任务。适用于不想单独维护 daemon 启动入口的小型部署。

```php
<?php
// === FPM 请求内 ===
use Xhjob\XhjobService;
use Xhjob\TaskBuilder;

$svc = new XhjobService('app-svc', '/var/lib/xhjob');
$svc->ensureRunning();          // 未运行则拉起，已运行则跳过

$taskId = TaskBuilder::shell('php /app/jobs/process.php ' . escapeshellarg($payload))
    ->timeout(300)
    ->dispatch('app-svc', '/var/lib/xhjob');   // 客户端 IPC，立即返回 task_id
```

> 方式二的风险：FPM worker 拉起 daemon 会增加首次请求延迟（百毫秒级），且若 FPM 被批量重启，多个 worker 可能并发尝试拉起同一 daemon。`daemon::start` 内部用 PID 文件 + `DaemonAlreadyRunning` 做互斥，但仍建议生产用方式一。

### 跨进程模式：`xhjob_server.php`

仓库提供的 `tp/xhjob_server.php` 演示了**跨进程**启动：脚本启动 daemon → 等待健康检查 → 打印 `READY` → `exit(0)`，daemon 必须在脚本退出后仍独立运行。这是验证 daemon 独立性的最小用例。

```bash
# 启动（脚本退出后 daemon 仍运行）
php -d extension=xhjob.so tp/xhjob_server.php --service=app-svc --data-dir=/var/lib/xhjob
# 输出：READY pid=12345 service=app-svc data_dir=/var/lib/xhjob

# 另一个进程查状态（连到同一个 daemon）
php -d extension=xhjob.so tp/xhjob_server.php --service=app-svc --data-dir=/var/lib/xhjob --status

# 停止
php -d extension=xhjob.so tp/xhjob_server.php --service=app-svc --data-dir=/var/lib/xhjob --stop
```

## 路径解析优先级

CLI 与 FPM 要连同一个 daemon，**必须解析到同一个 socket 文件**。socket / pid / db / log 文件的目录按以下优先级解析（从高到低）：

| 优先级 | 来源 | 适用文件 | 说明 |
|--------|------|---------|------|
| 1 | 显式参数 | 全部 | `xhjob_*($name, $dataDir)` 或 `new XhjobService($name, $dataDir)` 传入的 `data_dir` |
| 2 | 细分 env（Unix） | sock / pid / log | `XHJOB_SOCK_DIR` / `XHJOB_PID_DIR` / `XHJOB_LOG_DIR`，分别覆盖对应文件类型 |
| 3 | `XHJOB_DATA_DIR` | 全部 | 统一数据目录，设置后所有运行时文件置于其下 |
| 4 | 平台默认 | 全部 | Unix：`/run/xhjob` > `/var/run/xhjob` > `/tmp`；Windows：`%TEMP%` |

源码依据（`src/ipc/mod.rs::ipc_path` 与 `src/daemon/mod.rs::resolve_dir_path`）：

```rust
// ipc_path 的目录解析顺序
let dir = if let Some(d) = data_dir { ... }            // ① 显式参数
          else if let Ok(d) = env::var("XHJOB_SOCK_DIR") { ... }  // ② 细分 env
          else if let Ok(d) = env::var("XHJOB_DATA_DIR") { ... }  // ③ 统一 env
          else { fallback_sock_dir() };                // ④ 平台默认

// fallback_sock_dir：/run/xhjob > /var/run/xhjob > /tmp
if Path::new("/run").exists() { "/run/xhjob" }
else if Path::new("/var/run").exists() { "/var/run/xhjob" }
else { "/tmp" }
```

文件命名规则：

| 文件 | Unix 路径 | Windows 路径 |
|------|----------|-------------|
| IPC socket | `<dir>/xhjob.<name>.sock` | `\\.\pipe\xhjob-<name>` |
| PID 文件 | `<dir>/xhjob.<name>.pid` | `%TEMP%\xhjob.<name>.pid` |
| 日志文件 | `<dir>/xhjob.<name>.log` | `%TEMP%\xhjob.<name>.log` |
| SQLite DB | `<dir>/xhjob.<name>.db` | `%TEMP%\xhjob.<name>.db` |

> **`/run/xhjob` 与 `/var/run/xhjob` 在创建时设为 `0o700` 权限**，防止 symlink 攻击与同名占位。`/tmp` 是全局可写且带 sticky bit，存在 symlink / 名称占位风险，仅作最后兜底。**生产环境务必显式指定 `data_dir`，不要落到 `/tmp`。**

## 多服务隔离

不同 `service_name` 完全独立——每个服务名对应独立的 daemon 进程、独立的 socket / pid / db / log 文件。同一台机器可同时运行多个互不干扰的 daemon。

服务名校验规则（`src/service/mod.rs::validate`）：`^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`——字母开头，仅含字母 / 数字 / 下划线 / 短横线，最长 32 字符。

```php
<?php
// 两个独立服务：业务队列与定时任务互不干扰
$queueSvc = new XhjobService('queue-svc', '/var/lib/xhjob');
$cronSvc  = new XhjobService('cron-svc',  '/var/lib/xhjob');

$queueSvc->ensureRunning();      // queue-svc daemon
$cronSvc->ensureRunning();       // cron-svc daemon（独立进程、独立 DB）

// 派发到各自的服务
TaskBuilder::shell('php /app/jobs/process.php')->dispatch('queue-svc', '/var/lib/xhjob');
TaskBuilder::shell('php /app/jobs/report.php')->cron('0 9 * * *')->dispatch('cron-svc', '/var/lib/xhjob');
```

对应的运行时文件：

| 服务 | socket | pid | db |
|------|--------|-----|-----|
| `queue-svc` | `/var/lib/xhjob/xhjob.queue-svc.sock` | `xhjob.queue-svc.pid` | `xhjob.queue-svc.db` |
| `cron-svc` | `/var/lib/xhjob/xhjob.cron-svc.sock` | `xhjob.cron-svc.pid` | `xhjob.cron-svc.db` |

## PID 文件双行格式：防 PID 复用

daemon 启动时写 PID 文件，采用**双行格式**：第一行 PID，第二行 starttime（Linux `/proc/<pid>/stat` 第 22 字段，单位 clock ticks）。

```
12345
4567890
```

读取 PID 文件时（`src/daemon/mod.rs::read_pid`）：

1. 解析第一行 PID，第二行 starttime（可选，兼容旧版单行格式）。
2. 用 `is_process_alive_with_starttime(pid, starttime)` 校验：`kill(pid, 0)` 探活 **且** starttime 匹配。
3. 若 PID 已死，或 PID 被复用但 starttime 不匹配 → 判定为 stale，删除 PID 文件并返回 `None`。

```rust
pub fn write_pid(pid: u32, starttime: Option<u64>, ...) -> Result<()> {
    let content = match starttime {
        Some(st) => format!("{}\n{}", pid, st),   // 新格式：pid + starttime
        None => pid.to_string(),                  // 旧格式：仅 pid（向后兼容）
    };
    std::fs::write(&path, content)?;
}
```

**为什么需要 starttime**：PID 是会复用的。daemon 退出后，OS 可能把它的 PID 分配给一个完全无关的新进程。若只看 PID 探活，会把"无关进程"误判为"daemon 还在运行"，导致后续 `stop` 误杀无辜进程（root 下尤其灾难性）。加上 starttime 双校验后，只有"PID 存活 **且** starttime 与记录一致"才认为是同一个 daemon。

`stop` 路径的 SIGKILL 升级也依赖 starttime：仅当 PID 文件是双行格式（`starttime = Some`）时，SIGTERM 超时后才允许升级 SIGKILL；旧版单行格式（`starttime = None`）则**拒绝 SIGKILL**（fail-SAFE），避免杀到被复用的无关 PID。

## IPC 超时：防 FPM worker 永久阻塞

所有从 PHP-FPM 入口发起的 IPC 调用都包裹在 `tokio::time::timeout` 中，超时由 `XHJOB_IPC_TIMEOUT_SECS` 控制（默认 5s）。

```rust
// src/lib.rs::ipc_request —— 所有 FPM 入口的路由都走这里
async fn ipc_request(op: &str, payload: Value, service_name: &str, data_dir: Option<&str>) -> Result<Response> {
    let timeout_secs = ipc::default_ipc_timeout_secs();   // 默认 5s
    match tokio::time::timeout(Duration::from_secs(timeout_secs),
                                ipc::request(op, payload, service_name, data_dir)).await {
        Ok(inner) => inner,
        Err(_) => Err(XhjobError::ipc(format!("request timeout ({}s) for op={}", timeout_secs, op))),
    }
}
```

**为什么必须超时**：`max_execution_time` **不会中断 C 级阻塞调用**。若 daemon 已 accept 连接但随后死锁 / 被 SIGSTOP / 在 accept 后崩溃，PHP 端会阻塞在 `read_exact` 上 indefinitely，FPM worker 被一个个钉死，直到 worker 池耗尽、站点 502/504 且无法自愈。IPC 超时把这次等待限定在 5s 内，让 FPM worker fail-fast 返回错误给 PHP。

```bash
# 调小超时（高并发场景，希望更快 fail-fast）
export XHJOB_IPC_TIMEOUT_SECS=2

# 调大超时（仅当确实有长 IPC 操作，如大批量 list）
export XHJOB_IPC_TIMEOUT_SECS=15
```

> 5s 对任何本地 IPC 操作（SQLite 写 / cron 扫描 / dispatch）都足够。调小可加速 daemon 故障时的 fail-fast，调大需谨慎——它直接决定单个 FPM worker 在 daemon 异常时被占用多久。

## 完整端到端示例：systemd + FPM + CLI

### ① systemd 启 daemon

```ini
# /etc/systemd/system/xhjob-app.service
[Unit]
Description=xhjob daemon (app-svc)
After=network.target

[Service]
Type=oneshot
# daemon 自身是 double-fork 的独立进程，systemd 只负责"拉起并确认就绪"
RemainAfterExit=yes
Environment=XHJOB_DATA_DIR=/var/lib/xhjob
Environment=XHJOB_PERSIST=1
Environment=XHJOB_POOL_MODE=async
Environment=XHJOB_IPC_TIMEOUT_SECS=5
ExecStart=/usr/bin/php -d extension=xhjob.so /opt/xhjob/bin/start-daemon.php
ExecStop=/usr/bin/php -d extension=xhjob.so -r 'xhjob_stop("app-svc", "/var/lib/xhjob");'
User=www-data
RuntimeDirectory=xhjob
RuntimeDirectoryMode=0700

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now xhjob-app.service
# 验证：socket 文件应已生成
ls -l /var/lib/xhjob/xhjob.app-svc.sock /var/lib/xhjob/xhjob.app-svc.pid
```

### ② FPM 请求内派发（仅做客户端）

```php
<?php
// === FPM Web 请求内：只派发，不负责 daemon 生命周期 ===
use Xhjob\TaskBuilder;

// daemon 已由 systemd 拉起，这里直接派发（IPC 客户端，5s 超时保护）
$taskId = TaskBuilder::shell('php /app/jobs/process.php ' . escapeshellarg($payload))
    ->timeout(300)
    ->withRetry(3, 5)
    ->dispatch('app-svc', '/var/lib/xhjob');

header('Content-Type: application/json');
echo json_encode(['task_id' => $taskId]);
```

> FPM 侧**不需要**调用 `ensureRunning()`——daemon 由 systemd 管理。若担心 daemon 偶发不可用，可加一个轻量探活：`$svc->status()` 不阻塞，未运行时返回 `running=false`，再决定是报错还是尝试 `ensureRunning()`。

### ③ CLI 查状态 / 取结果

```php
<?php
// === CLI 脚本：查任务状态（连同一个 daemon）===
use Xhjob\TaskManager;

$mgr   = new TaskManager('app-svc', '/var/lib/xhjob');   // 同名 + 同 data_dir
$state = $mgr->state($taskId);
print_r($state);

if (in_array($state['state'] ?? '', ['success', 'failed'], true)) {
    $result = $mgr->result($taskId);
    echo "stdout: " . ($result['stdout'] ?? '') . "\n";
    echo "exit_code: " . ($result['exit_code'] ?? -1) . "\n";
}
```

### ④ 命令行快速查询

```bash
# 查 daemon 状态
php -d extension=xhjob.so -r 'print_r(xhjob_status("app-svc", "/var/lib/xhjob"));'

# 查任务状态
php -d extension=xhjob.so -r 'print_r(xhjob_state("TASK_ID", "app-svc", "/var/lib/xhjob"));'

# 查所有注册任务
php -d extension=xhjob.so -r 'print_r(xhjob_inspect("registered", "app-svc", "/var/lib/xhjob"));'
```

## 注意事项

| 关注点 | 说明 |
|------|------|
| **同名 + 同 data_dir 是唯一约束** | CLI 与 FPM 必须传入完全相同的 `service_name` 与 `data_dir`，否则会解析到不同 socket，连到不同 daemon。生产建议把这两个值固化在配置文件 / 环境变量中，所有入口统一读取。 |
| **daemon 由 systemd 管理最佳** | 避免让 FPM worker 拉起 daemon（首次请求延迟、并发拉起竞争）。systemd 负责 daemon 生命周期，FPM / CLI 只做客户端。 |
| **不要落到 `/tmp`** | `/tmp` 全局可写带 sticky bit，有 symlink / 名称占位风险。生产显式指定 `data_dir`（如 `/var/lib/xhjob`），或用 `/run/xhjob`（systemd tmpfs，0o700）。 |
| **data_dir 跨进程一致** | `tp/xhjob_server.php` 默认用 `/tmp/xhjob-cross`，生产替换为实际 `data_dir`，否则 client 连不到 server 拉起的 daemon。 |
| **IPC 超时是 FPM 护栏** | `XHJOB_IPC_TIMEOUT_SECS` 默认 5s，daemon 死锁时保护 FPM worker 不被永久阻塞。不要设为 0 或过大。 |
| **重启 daemon 不丢持久化任务** | `persist(true)` 的任务定义存在 SQLite，daemon 重启后自动恢复；`acksLate(true)` 的 Running 任务会被重置为 Pending 重投。详见[定时任务](prod-cron/)与[持久化与崩溃恢复](../persistence-recovery/)。 |
| **stop/restart 的 SIGKILL 安全** | 仅当 PID 文件是双行格式（含 starttime）时，SIGTERM 超时后才升级 SIGKILL。旧版单行格式拒绝 SIGKILL（fail-SAFE），需手动清理。 |
| **多服务隔离** | 不同 `service_name` 是完全独立的 daemon + DB。业务队列与定时任务建议分服务部署，互不干扰。 |
