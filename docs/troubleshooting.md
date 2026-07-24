# 故障排查

> 排障篇 · 8 类高频问题 + ThinkPHP 8 集成排障 + 错误信息对照表

本篇覆盖 xhjob 在加载、启动、派发、调度、持久化、FPM 集成等环节的常见故障。每个问题按 **症状 → 根因 → 解决方法 → 验证** 四段式组织，所有命令与代码均基于真实 API，可直接复制执行。

> 阅读前建议先熟悉 [安装与配置](install-config.md) 中的环境变量全表与服务名校验规则，以及 [PHP 函数参考](api-functions.md) 中的错误契约（`error:` 前缀 / `false` / `isset($r['error'])`）。

---

## 1. 扩展未加载

### 症状

调用任何 `xhjob_*` 函数时 PHP 抛出致命错误：

```
Fatal error: Uncaught Error: Call to undefined function xhjob_status()
```

在命名空间内调用时则表现为带命名空间前缀的形式：

```
Fatal error: Uncaught Error: Call to undefined function Xhjob\xhjob_status()
```

### 根因

xhjob 是 Rust + ext-php-rs 实现的 PHP 扩展（`.so` / `.dll`），必须在 PHP 进程启动时通过 `extension=` 加载。若当前 SAPI（CLI / FPM）未加载该扩展，所有 `xhjob_*` 全局函数均不存在。常见诱因：

- 只在 CLI 的 `conf.d` 放了 ini，FPM 没放（或反之）。
- 用了 `dl()` 加载——多数 SAPI（含 FPM，及 `enable_dl=Off` 的 CLI）下 `dl()` 不可用。
- 扩展路径错误或与 PHP 版本 / ZTS 不匹配，加载时静默失败。

### 解决方法

**方式一：php.ini 永久加载（推荐）**

```ini
; /etc/php/8.2/fpm/conf.d/50-xhjob.ini
; /etc/php/8.2/cli/conf.d/50-xhjob.ini
extension=/path/to/xhjob-php8.2-linux-x86_64.so
```

CLI 与 FPM 各自的 `conf.d` 都要放（或用 `PHP_INI_SCAN_DIR` 共享同一扫描目录）。

**方式二：PHP_INI_SCAN_DIR 共享扫描目录**

```bash
# 把 xhjob 的 ini 放到独立目录，追加到扫描路径
export PHP_INI_SCAN_DIR=/etc/php/8.2/fpm/conf.d:/etc/xhjob-ini
# 在 /etc/xhjob-ini/50-xhjob.ini 放 extension=/path/to/xhjob-php8.2-linux-x86_64.so
```

**方式三：命令行临时加载（调试用）**

```bash
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so your_script.php
```

> `dl()` 在多数 SAPI 下被禁用或不可用，**不可靠**，不要用于加载 xhjob。

### 验证

```bash
# 1. 模块列表中应出现 xhjob
php -m | grep xhjob

# 2. 函数存在性检查
php -r 'echo function_exists("xhjob_dispatch") ? "ok" : "no";'

# 3. 同时确认 CLI 与 FPM 都已加载（路径按实际替换）
php-fpm8.2 -m | grep xhjob
```

输出 `xhjob` 与 `ok` 即加载成功；FPM 修改 ini 后需 `reload` 生效。

---

## 2. daemon 启动失败

### 症状

```php
<?php
if (!xhjob_start('cron-svc', '/var/lib/xhjob')) {
    error_log('xhjob daemon 启动失败');  // 走到这里
}
```

`xhjob_start()` 返回 `false`，或随后 `xhjob_status()` 显示 `running=false`。

### 根因

`xhjob_start` 以 spawn + re-exec 方式拉起独立守护进程，失败原因（写入 `tracing::error!` 与 daemon 日志）主要有三类：

1. **socket 目录权限不足**：`XHJOB_SOCK_DIR` 指向的目录当前用户无写权限，无法创建 `<name>.sock`。
2. **端口 / socket 占用**：同名服务的 socket 文件已存在且 daemon 仍在运行，或残留 socket 未清理。
3. **服务名非法**：`$name` 不匹配 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`（首字符必须是字母，长度 1–32）。注意：非法服务名在部分路径下会**回退到 `default`**而非报错，但在 `xhjob_start` 中会写入错误日志后返回 `false`。

### 解决方法

**1. 检查 socket 目录权限**

```bash
# 查看 sock_dir 解析结果（默认 /run/xhjob > /var/run/xhjob > /tmp）
echo "$XHJOB_SOCK_DIR"
ls -ld /run/xhjob /var/run/xhjob 2>/dev/null

# 确保运行用户可写（以 www-data 为例）
sudo mkdir -p /run/xhjob
sudo chown www-data:www-data /run/xhjob
sudo chmod 755 /run/xhjob
```

生产环境建议用 systemd 的 `RuntimeDirectory=xhjob` 自动创建 `/run/xhjob`（tmpfs，重启清空）。

**2. 清理残留 socket / 确认无重复 daemon**

```bash
# 查看 socket 文件
ls -l ${XHJOB_SOCK_DIR:-/run/xhjob}/

# 确认无残留 daemon 进程
ps aux | grep xhjob
```

若 daemon 已在运行（PID 存活且非僵尸），`xhjob_start` 会直接返回 `true` 不会重复启动；若 socket 残留但进程已死，删除残留 socket 后重试。

**3. 确认服务名合法**

```bash
# 服务名规则：^[a-zA-Z][a-zA-Z0-9_-]{0,31}$
# 合法：default  my_service  svc-1  A_b-C
# 非法：1svc  svc.1  服务
```

**4. 查看 daemon 日志**

```bash
# 日志目录默认 fallback /tmp，生产应显式设置 XHJOB_LOG_DIR
tail -n 100 ${XHJOB_LOG_DIR:-/tmp}/cron-svc.log
```

### 验证

```php
<?php
$s = xhjob_status('cron-svc', '/var/lib/xhjob');
if (isset($s['error'])) {
    throw new RuntimeException("状态查询失败：{$s['error']}");
}
if ($s['running'] === 'true') {
    printf("daemon 运行中，pid=%s\n", $s['pid'] ?? '-');
}
```

`xhjob_status()` 返回 `running=true`（含 `pid`）即启动成功。注意 `running` 是字符串 `"true"`/`"false"`，不是布尔值。

---

## 3. xhjob_dispatch 返回 error: 前缀

### 症状

```php
<?php
$r = xhjob_dispatch($taskJson, 'cron-svc');
// $r 形如："error: service name invalid: 1svc" 或 "error: ipc timeout"
```

返回字符串以 `error:` 开头，而非 task_id。

### 根因

`xhjob_dispatch` 的错误契约是：失败以 `error:` 前缀返回。失败原因包括：

- **服务名校验失败**：`$name` 不匹配 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`。
- **task_json 非法**：JSON 解析失败或字段结构与 Rust `TaskBuilder` 不一致。
- **daemon 不可达**：IPC 超时（`XHJOB_IPC_TIMEOUT_SECS`，默认 5 秒）或 daemon 未启动。

### 解决方法

**1. 用 `str_starts_with` 检测并打印具体错误**

```php
<?php
$r = xhjob_dispatch($taskJson, 'cron-svc');
if (str_starts_with($r, 'error:')) {
    // 错误信息在 'error:' 之后（substr 偏移 6）
    throw new RuntimeException('派发失败：' . substr($r, 6));
}
$taskId = $r;
```

> 兼容 PHP 7.x 时用 `strncmp($r, 'error:', 6) === 0`。**永远不要把 `error: xxx` 字符串当作 task_id 使用。**

**2. 确认 daemon 已启动**

```php
<?php
$s = xhjob_status('cron-svc', '/var/lib/xhjob');
if ($s['running'] !== 'true') {
    // 先启动 daemon
    xhjob_start('cron-svc', '/var/lib/xhjob');
}
```

**3. 确认 task_json 结构正确**

优先用 `TaskBuilder::toJson()` 产物或 `Xhjob` 链式构建器，避免手拼 JSON 触发 serde 反序列化失败：

```php
<?php
// 推荐：用 Xhjob 链式构建器
$r = Xhjob::task()
    ->service('cron-svc')
    ->viaShell('echo hello')
    ->cron('0 * * * *')
    ->dispatch();
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('dispatch 失败：' . substr($r, 6));
}
```

### 验证

```php
<?php
$r = xhjob_dispatch($taskJson, 'cron-svc');
assert(!str_starts_with($r, 'error:'), "dispatch 仍失败：$r");
echo "task_id = $r\n";

// 进一步确认任务已进入调度
$st = xhjob_state($r, 'cron-svc');
echo "state = " . ($st['state'] ?? 'UNKNOWN') . "\n";
```

dispatch 返回不以 `error:` 开头的 task_id，且 `xhjob_state` 能查到 `state` 字段（非 `UNKNOWN`）即成功。

---

## 4. 任务卡 Running

### 症状

```php
<?php
// 长时间轮询，state 始终为 running
for ($i = 0; $i < 60; $i++) {
    $s = xhjob_state($taskId, 'default');
    echo $s['state'] . "\n";   // 一直输出 running
    sleep(5);
}
```

任务长时间停留在 `state=running`，既不成功也不失败。

### 根因

1. **watchdog 未启用或 factor 太大**：`XHJOB_WATCHDOG_INTERVAL=0` 禁用了 watchdog，或 `XHJOB_WATCHDOG_FACTOR` 设得过大，假死任务未被及时中断。
2. **任务真正假死**：子进程卡在 IO（fifo / NFS / DNS 阻塞 read），tokio `timeout` future 未触发。
3. **worker_pid 已死但 lease 未清理**：worker 进程异常退出，但 lease 记录残留，任务未被重置。

### 解决方法

**1. 检查 watchdog 配置**

watchdog 每 `XHJOB_WATCHDOG_INTERVAL` 秒（默认 5，`0`=禁用）扫描所有 Running 任务，当任务运行时长超过 `timeout * XHJOB_WATCHDOG_FACTOR`（默认 2）时，发送取消信号并标记 `Interrupted`，记录 `hung_detected` 事件。

```bash
# 查看当前配置
echo "interval=${XHJOB_WATCHDOG_INTERVAL:-5} factor=${XHJOB_WATCHDOG_FACTOR:-2}"

# 确认未禁用（0 会完全关闭 watchdog）
# 生产建议保持默认 interval=5, factor=2
```

任务必须配置了 `timeout`（非 0），否则 watchdog 无法判断"假死"基线，会被跳过。

**2. 手动强制取消**

```php
<?php
// 强制取消卡住的任务
if (!xhjob_cancel($taskId, 'default')) {
    error_log("取消失败：任务可能已终态");
}
```

`xhjob_cancel` 对 Pending 任务转 `cancelled` 终态不再触发；对 Running 任务发送取消信号并转 `cancelled`。

**3. 检查 lease 状态**

```php
<?php
$st = xhjob_state($taskId, 'default');
if (isset($st['worker_pid'])) {
    printf("worker_pid=%s worker_starttime=%s\n", $st['worker_pid'], $st['worker_starttime'] ?? '-');
    // 若 worker_pid 对应进程已不存在，但 state 仍为 running，则为 lease 残留
}
```

### 验证

```php
<?php
$st = xhjob_state($taskId, 'default');
echo $st['state'] . "\n";
```

- watchdog 自动中断：`state=interrupted`（同时可在事件流中查到 `hung_detected`）。
- 手动取消：`state=cancelled`。

```php
<?php
// 确认事件流中有 hung_detected 事件（watchdog 介入）
$r = xhjob_events(time() - 3600, $taskId, 'default');
if (!str_starts_with($r, 'error:')) {
    foreach (json_decode($r, true) as $ev) {
        if ($ev['event_type'] === 'hung_detected') {
            echo "watchdog 已检测到假死\n";
        }
    }
}
```

---

## 5. SQLite 损坏

### 症状

daemon 启动时日志出现告警：

```
ERROR xhjob::store] integrity check failed: <具体错误>
```

或表现为持久化数据丢失——daemon 重启后 `xhjob_inspect('registered')` 查不到之前注册的 cron / interval 任务。

### 根因

daemon 启动时对 SQLite 执行 `PRAGMA quick_check`（扫描每个 b-tree 页检测结构损坏）+ `PRAGMA foreign_keys=ON`（强制外键约束）。损坏诱因：

- **磁盘满**：写入时空间耗尽导致页写入不完整。
- **异常断电**：WAL 未正确 checkpoint。
- **文件系统损坏**：底层存储故障。

损坏时 daemon 不会在损坏的 DB 上继续运行（避免进一步损坏），而是告警并阻止启动。

### 解决方法

**1. 备份并删除损坏的 DB 文件**

```bash
# DB 文件名为 xhjob.{name}.db，路径由 data_dir / XHJOB_DB_DIR 决定
DB_PATH=${XHJOB_DB_DIR:-/tmp}/xhjob.cron-svc.db

# 先备份（用于事后分析）
cp "$DB_PATH" "${DB_PATH}.corrupt.$(date +%s).bak"

# 删除 DB 及 WAL/SHM 附属文件，重启后 daemon 会重建
rm -f "$DB_PATH" "$DB_PATH-wal" "$DB_PATH-shm"
```

**2. 确认持久化已启用**

```bash
# XHJOB_PERSIST 默认 true（当编译启用 persist feature 时），显式设 1 更稳妥
export XHJOB_PERSIST=1

# 确认编译启用了 --all-features（含 persist feature）
# 扩展构建：cargo build --release --all-features
```

**3. 重启 daemon**

```php
<?php
xhjob_stop('cron-svc', '/var/lib/xhjob');
// 等待退出
while (xhjob_status('cron-svc', '/var/lib/xhjob')['running'] === 'true') {
    usleep(200_000);
}
xhjob_start('cron-svc', '/var/lib/xhjob');
```

### 验证

```bash
# 1. daemon 启动日志中无 integrity 告警
grep -i "integrity check" ${XHJOB_LOG_DIR:-/tmp}/cron-svc.log
# 期望：无输出
```

```php
<?php
// 2. inspect stats 正常返回（不再 error:）
$r = xhjob_inspect('stats', 'cron-svc');
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('inspect 失败：' . substr($r, 6));
}
$stats = json_decode($r, true);
print_r($stats);
```

daemon 启动无 integrity 告警、`xhjob_inspect('stats')` 正常返回 JSON 即恢复。

---

## 6. FPM worker 阻塞

### 症状

PHP-FPM 请求长时间无响应，且 `max_execution_time` 未中断请求——表现为浏览器/客户端一直转圈，FPM worker 被占满。

### 根因

xhjob 的 IPC 调用（`xhjob_dispatch` / `xhjob_state` / `xhjob_result` 等）在 C 扩展层通过 `block_on` 同步等待 daemon 响应。**`max_execution_time` 只能中断 PHP 解释器层面的执行，无法中断 C 级阻塞调用**——当 IPC 进入 C 级 `recv`/`connect` 阻塞时，`max_execution_time` 计时器不会生效，FPM worker 会一直挂起直到 IPC 自身超时（默认 `XHJOB_IPC_TIMEOUT_SECS=5`）。

当 daemon 健康但响应慢，或 daemon 不可达但 TCP 栈未及时返回错误时，5 秒超时对 FPM 请求而言偏长，易导致 worker 耗尽。

### 解决方法

**1. 调小 IPC 超时**

```bash
# FPM 环境建议设 2-3 秒（默认 5）
export XHJOB_IPC_TIMEOUT_SECS=2
```

或在 FPM 的 `www.conf` / `php-fpm.conf` 的 env 段设置：

```ini
env[XHJOB_IPC_TIMEOUT_SECS] = 2
```

> 注意：FPM 必须通过 `env[...]` 注入环境变量，`putenv()` 在 FPM worker 内设置对子进程 IPC 无效。

**2. 确认 daemon 健康**

```php
<?php
// 在请求入口做一次轻量探活，daemon 不健康时快速失败而非阻塞
$s = xhjob_status('default');
if ($s['running'] !== 'true') {
    http_response_code(503);
    exit('xhjob daemon unavailable');
}
```

**3. 避免在 FPM 请求中做长查询**

派发后不要在请求内同步轮询 `xhjob_state` 等待结果——改为异步：FPM 只负责 `xhjob_dispatch`，结果由前端轮询或回调获取。

### 验证

```php
<?php
// 模拟 daemon 不可达：IPC 应在 2 秒内超时返回 error:，而非长时间阻塞
$st = microtime(true);
$r = xhjob_dispatch($taskJson, 'unreachable-svc');
$elapsed = microtime(true) - $st;

echo "elapsed={$elapsed}s result={$r}\n";
// 期望：elapsed < 3s，且 r 以 'error:' 开头
```

FPM 请求在 IPC 超时后正常返回错误（而非无限挂起）即修复。

---

## 7. persist feature 未启用

### 症状

daemon 启动时日志出现告警：

```
WARN xhjob::daemon_main] XHJOB_PERSIST=1 but `persist` feature not enabled; falling back to InMemoryStore
```

或表现为 daemon 重启后任务全部丢失——之前注册的 cron / interval 任务消失，`xhjob_inspect('registered')` 返回空。

### 根因

持久化是**编译时 feature**，由 `Cargo.toml` 的 `persist = ["rusqlite", "aes-gcm"]` 控制。两种情况触发回退到 InMemoryStore：

1. **编译未启用 `persist` feature**：二进制中根本没有 `SqliteStore`，无论 `XHJOB_PERSIST` 设何值都回退到内存存储。
2. **`XHJOB_PERSIST=0`**：即便编译启用了 feature，显式设 `0` 也会禁用持久化。

InMemoryStore 回退后，daemon 进程一旦退出，**所有未完成任务全部丢失**，`acksLate(true)` 也会失效（无持久化可恢复）。

### 解决方法

**1. 用 `--all-features` 重新编译**

```bash
# 重新编译扩展与 daemon，启用 persist feature（含 rusqlite + aes-gcm）
cargo build --release --all-features
# 或仅启用 persist
cargo build --release --features persist
```

**2. 确认 `XHJOB_PERSIST=1`（或不设）**

```bash
# 编译启用 persist feature 时，XHJOB_PERSIST 默认 true
# 显式设 1 更稳妥，或不设（走默认）
export XHJOB_PERSIST=1

# 确认未被设为 0 / false
echo "XHJOB_PERSIST=${XHJOB_PERSIST:-<unset, defaults true>}"
```

> 逻辑：编译启用 feature 时，`XHJOB_PERSIST` 默认 `true`，仅 `0`/`false` 禁用；编译未启用时，默认 `false`，仅 `1`/`true` 尝试启用（但会告警回退）。

**3. 重启 daemon**

```php
<?php
xhjob_restart('cron-svc', '/var/lib/xhjob');
```

### 验证

```bash
# 1. 启动日志中应显示 "opening SQLite store" 而非回退告警
grep -E "opening SQLite store|InMemoryStore" ${XHJOB_LOG_DIR:-/tmp}/cron-svc.log
# 期望：出现 "opening SQLite store"，无 "falling back to InMemoryStore"
```

```php
<?php
// 2. 注册一个 cron 任务
xhjob_dispatch(json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo hi'],
    'cron'      => '0 * * * *',
    'persist'   => true,
]), 'cron-svc', '/var/lib/xhjob');
```

```bash
# 3. 重启 daemon 后，registered 仍能看到任务定义
php -r '
    $r = xhjob_inspect("registered", "cron-svc", "/var/lib/xhjob");
    if (str_starts_with($r, "error:")) { echo $r; exit; }
    $tasks = json_decode($r, true);
    echo "registered tasks: " . count($tasks) . "\n";
'
```

daemon 重启后 `xhjob_inspect('registered')` 仍能看到任务定义即持久化生效。

---

## 8. zombie 进程残留

### 症状

```bash
$ ps aux | grep xhjob
www-data  12345  0.0  0.0      0     0 ?        Z    10:00   0:00 [xhjob-daemon] <defunct>
```

`ps` 看到 `defunct` / `[<进程名>] <defunct>` 状态的 zombie 进程。测试环境（daemon 作为 PHP 子进程拉起）尤为常见。

### 根因

daemon 由 `xhjob_start` 以 spawn + re-exec 方式拉起为子进程。当 daemon 子进程退出后，其退出状态需要由**父进程 `wait`/`waitpid` reap**才能从进程表清除。若父进程：

- 未调用 `pcntl_waitpid` / `pcntl_wait` reap 子进程；
- 或父进程自身已退出但 init 未及时接管；

子进程就会变成 zombie（`Z` 状态），占用 PID 表项。测试环境用 PHP 脚本拉起 daemon 又不 reap 时最易复现。

### 解决方法

**测试环境（daemon 是 PHP 子进程）**

```bash
# 1. 找到 zombie 的父进程 PID
ps -o pid,ppid,stat,cmd -p <zombie_pid>
# PPID 列即为父进程

# 2. 若父进程是测试脚本，先 kill 父进程让 init 接管 reap
kill <parent_pid>

# 3. 父进程已死但 zombie 仍残留，用 kill -9 清理（仅测试环境）
kill -9 <zombie_pid>
```

在 PHP 测试脚本中主动 reap：

```php
<?php
// 测试脚本退出前 reap 所有子进程
while (($pid = pcntl_waitpid(-1, $status, WNOHANG)) > 0) {
    echo "reaped child pid=$pid\n";
}
```

**生产环境**

生产环境用 systemd 托管 daemon，由 systemd 负责 reap，**不会产生 zombie**：

```ini
# /etc/systemd/system/xhjob.service
[Unit]
Description=Xhjob Daemon
After=network.target

[Service]
Type=simple
User=xhjob
ExecStart=/usr/bin/php -d extension=/path/to/xhjob.so /path/to/daemon-runner.php
RuntimeDirectory=xhjob
Environment=XHJOB_SERVICE_NAME=default
Environment=XHJOB_PERSIST=1
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

> 生产环境**不要**用 FPM / Web 请求内 `xhjob_start` 拉起 daemon——FPM worker 退出时会带走未 reap 的子进程或留下 zombie。daemon 应由进程管理器（systemd / supervisor）独立托管。

### 验证

```bash
# 查看是否还有 zombie 进程
ps aux | grep xhjob | grep -E "defunct|Z"
# 期望：无输出

# 或按状态码过滤（Z = zombie）
ps -eo pid,ppid,stat,cmd | awk '$3 ~ /Z/ && $4 ~ /xhjob/'
# 期望：无输出
```

`ps aux | grep xhjob` 无 `defunct` 行即清理完成。

---

## ThinkPHP 8 集成排障

### 安装

```bash
composer require xhjob/thinkphp8-extend
```

该包通过 `extra.think.services` 自动注册 `Xhjob\ServiceProvider`，并通过 `autoload.files` 自动加载 `Xhjob/helper.php`。

### 问题一：api_token 未配置中间件抛 500

**症状**：访问 `/xhjob/*` 路由时返回 HTTP 500：

```
Xhjob API token not configured: set XHJOB_API_TOKEN env var
```

**根因**：`XhjobAuth` 中间件对 `/xhjob/*` 路由强制校验 `X-Xhjob-Token` header。`api_token` 配置项（`config('xhjob.api_token')`）为 `null` 或空串时，中间件 **fail closed**（拒绝所有请求，含 `createShell` 等 RCE 入口），抛 `HttpException(500)`。

**解决方法**：配置 `api_token`：

```bash
# .env
XHJOB_API_TOKEN=long-random-secret-token
```

```php
// config/xhjob.php
return [
    'service_name' => env('XHJOB_SERVICE', 'default'),
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    'api_token'    => env('XHJOB_API_TOKEN', null),
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

> 注意：中间件用 `$expectedToken === null || $expectedToken === ''` 判空（非 `empty()`），字符串 `"0"` 是合法 token。

**验证**：

```bash
# 配置后，带正确 header 访问
curl -H "X-Xhjob-Token: long-random-secret-token" http://localhost/xhjob/task/list
# 期望：200，不再 500
```

### 问题二：Facade 找不到方法

**症状**：调用 `\Xhjob\facade\Xhjob::create(...)` 报：

```
Call to undefined method Xhjob\facade\Xhjob::create()
```

或 Facade 方法返回 `null` / 抛容器异常。

**根因**：Facade 依赖容器中的 `xhjob.manager` 绑定（`TaskManager` 实例），由 `Xhjob\ServiceProvider::boot()` 注册。若 ServiceProvider 未注册，容器中无 `xhjob.manager`，Facade 调用全部失效。

**解决方法**：确认 ServiceProvider 已注册。ThinkPHP 8 通过包的 `extra.think.services` 自动注册，但若手动安装或未走 composer autoload，需在 `app/service.php` 显式注册：

```php
// app/service.php
return [
    \Xhjob\ServiceProvider::class,
    // ... 其他服务
];
```

确认 composer autoload 已重新生成：

```bash
composer dump-autoload
```

**验证**：

```php
<?php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

$id = Xhjob::create(TaskBuilder::shell('echo hi'));
echo $id;  // 期望：返回 task_id
```

### 问题三：helper 函数未定义

**症状**：调用 `xhjob_manager()` / `xhjob_service()` / `xhjob_task()` 报：

```
Call to undefined function xhjob_manager()
```

**根因**：helper 函数定义在 `Xhjob/helper.php`，通过 composer `autoload.files` 自动加载。若 composer autoload 未生成、或手动安装未配置 `files`，helper 不会被加载。

**解决方法**：

```bash
# 1. 确认 composer.json 的 autoload.files 包含 helper.php
# （包自带配置，重新 dump-autoload 即可）
composer dump-autoload

# 2. 若仍不生效，手动在入口文件 require
# require_once __DIR__ . '/vendor/xhjob/thinkphp8-extend/Xhjob/helper.php';
```

> helper 函数定义在 `if (!function_exists(...))` 守卫内，重复 require 不会报错。

**验证**：

```bash
php -r 'echo function_exists("xhjob_manager") ? "ok" : "no";'
php -r 'echo function_exists("xhjob_task") ? "ok" : "no";'
```

输出 `ok` 即 helper 已加载。

---

## 常见错误信息对照表

| 错误信息 | 含义 | 解决方向 |
|----------|------|----------|
| `Call to undefined function xhjob_status()` | 扩展未加载 | 用 `extension=` 加载 `.so`，见 [问题 1](#1-扩展未加载) |
| `Call to undefined function Xhjob\xhjob_status()` | 命名空间内调用但扩展未加载 | 同上，扩展加载后命名空间内调用同样生效 |
| `error: service name invalid: <name>` | 服务名不匹配 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$` | 改用合法服务名（首字符字母，仅字母/数字/下划线/连字符，长度 1–32） |
| `error: ipc timeout` | IPC 请求超时（`XHJOB_IPC_TIMEOUT_SECS`，默认 5 秒） | 确认 daemon 健康（`xhjob_status`）；FPM 调小超时，见 [问题 6](#6-fpm-worker-阻塞) |
| `error: json parse failed` | `task_json` 非法 JSON 或字段结构不符 | 改用 `TaskBuilder::toJson()` 或 `Xhjob` 链式构建器，见 [问题 3](#3-xhjob_dispatch-返回-error-前缀) |
| `error: daemon not running` | daemon 未启动或 socket 不可达 | `xhjob_start` 启动 daemon，见 [问题 2](#2-daemon-启动失败) |
| `error: no result record for this task` | 任务无结果记录（`ignoreResult=true` 或产出前失败） | 先 `xhjob_state` 确认已终态；非 daemon 故障 |
| `integrity check failed: <详情>` | SQLite 结构损坏（`PRAGMA quick_check` 检出） | 备份并删除 DB 重建，见 [问题 5](#5-sqlite-损坏) |
| `XHJOB_PERSIST=1 but persist feature not enabled; falling back to InMemoryStore` | 编译未启用 `persist` feature，回退内存存储 | `--all-features` 重新编译，见 [问题 7](#7-persist-feature-未启用) |
| `Xhjob API token not configured: set XHJOB_API_TOKEN env var` | ThinkPHP 中间件 `api_token` 未配置 | 配置 `XHJOB_API_TOKEN`，见 [ThinkPHP 排障](#thinkphp-8-集成排障) |
| `Unauthorized: invalid or missing Xhjob token` | 请求 `X-Xhjob-Token` header 缺失或不匹配 | 请求带上与 `api_token` 一致的 `X-Xhjob-Token` header |
| `Call to undefined method Xhjob\facade\Xhjob::create()` | ServiceProvider 未注册，Facade 失效 | 注册 `Xhjob\ServiceProvider`，`composer dump-autoload` |
| `Call to undefined function xhjob_manager()` | helper.php 未加载 | `composer dump-autoload` 或手动 `require` helper.php |
| `Call to undefined method Xhjob::withId()` | 调用了不存在的 `withId()`，真实方法名是 `id()` | 改用 `->id('xxx')`，见 [PHP 函数参考](api-functions.md) |

---

## EventType 事件类型速查

排障时通过 `xhjob_events` / `xhjob_pull_events` 查看任务生命周期事件，共 14 种（多词值用下划线）：

| event_type | 含义 |
|------------|------|
| `started` | 任务开始执行 |
| `succeeded` | 任务执行成功 |
| `failed` | 任务执行失败 |
| `missed` | 任务误触发被跳过（coalesce） |
| `cancelled` | 任务被取消 |
| `paused` | 任务被暂停 |
| `resumed` | 任务被恢复 |
| `expired` | 任务过期（Pending 超 `expires`） |
| `max_instances_reached` | 达到 `maxInstances` 上限，新实例被拒 |
| `rate_limited` | 触发 `rateLimit` 限流，本次触发被跳过 |
| `interrupted` | 任务被 watchdog 中断（假死检测） |
| `hung_detected` | watchdog 检测到任务假死 |
| `lease_held` | 任务 lease 续期 |
| `unknown` | 未知事件类型（向前兼容） |

```php
<?php
// 拉取最近 10 分钟的 failed 事件做告警
$r = xhjob_pull_events(time() - 600, 'failed', 'default');
if (!str_starts_with($r, 'error:')) {
    foreach (json_decode($r, true) as $ev) {
        error_log("[fail] task={$ev['task_id']} ts={$ev['ts']}");
    }
}
```

---

## 速查：关键环境变量

排障时最常调整的环境变量（完整列表见 [安装与配置](install-config.md#环境变量全表)）：

| 变量 | 默认值 | 排障场景 |
|------|--------|----------|
| `XHJOB_PERSIST` | `true`(编译启用 persist) / `false` | 任务重启后丢失 → [问题 7](#7-persist-feature-未启用) |
| `XHJOB_IPC_TIMEOUT_SECS` | `5` | FPM 阻塞 → [问题 6](#6-fpm-worker-阻塞)，建议 FPM 设 2–3 |
| `XHJOB_SERVICE_NAME` | `default` | 服务名非法 → [问题 2](#2-daemon-启动失败) |
| `XHJOB_SOCK_DIR` | `/run/xhjob` > `/var/run/xhjob` > `/tmp` | daemon 启动失败 → [问题 2](#2-daemon-启动失败) |
| `XHJOB_PID_DIR` | fallback `/tmp` | PID 文件冲突 |
| `XHJOB_LOG_DIR` | fallback `/tmp` | 所有排障的第一步：看日志 |
| `XHJOB_DATA_DIR` | （无） | 统一数据目录，覆盖各 `XHJOB_*_DIR` |
| `XHJOB_POOL_MODE` | `async` | 线程池模式（`async` / `thread` / `coroutine`） |
| `XHJOB_WATCHDOG_INTERVAL` | `5` | 任务卡 Running → [问题 4](#4-任务卡-running)，`0`=禁用 |
| `XHJOB_WATCHDOG_FACTOR` | `2` | watchdog 倍数因子，运行超 `timeout * factor` 判定假死 |
