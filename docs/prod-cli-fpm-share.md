# CLI 与 FPM 共用服务连接

> 生产实战篇 · 同一 daemon 多客户端接入 · 路径解析优先级 · 多服务隔离 · PID 复用防护 · IPC 超时兜底

Xhjob 的 daemon 是一个独立的 Rust 长驻进程，PHP 侧无论是 CLI 还是 FPM 都只是 IPC 客户端。只要 CLI 与 FPM 指向**同一个 service_name + data_dir**，它们就会连到同一个 Unix socket、同一个 daemon、同一份 SQLite DB，从而实现「FPM 请求派发任务、CLI 查询状态 / 运维操作」的协作模式。本文档说明共用原理、路径解析、多服务隔离、PID 复用防护与 IPC 超时兜底。

本文档按 **架构说明 → 完整可运行代码 → 注意事项 → 生产建议** 的结构展开。

---

## 架构说明

### 共用原理

```
                    ┌─────────────────┐
                    │   FPM worker    │
                    │  (Web 请求)     │
                    └────────┬────────┘
                             │ dispatch / state / result
                             │ (IPC 客户端)
                             ▼
   service_name=default      ┌──────────────────────┐      service_name=billing
   data_dir=/var/lib/xhjob   │   Unix socket        │      data_dir=/var/lib/xhjob
   ┌──────────────────┐      │   /run/xhjob/        │      ┌──────────────────┐
   │   CLI 运维脚本   │ ═══► │   default.sock       │ ◄═══ │   CLI worker     │
   │  (cron-ops.php)  │      │                      │      │  (result-worker) │
   └──────────────────┘      │   ┌──────────────┐   │      └──────────────────┘
                             │   │  daemon      │   │
                             │   │  (Rust, 单例)│   │
                             │   │  pid=default │   │
                             │   └──────┬───────┘   │
                             │          │           │
                             │   ┌──────▼───────┐   │
                             │   │ SQLite DB    │   │
                             │   │ default.db   │   │
                             │   └──────────────┘   │
                             └──────────────────────┘
```

核心要点：

- **daemon 是独立进程**：不是 FPM 的子进程，也不是 CLI 的子进程。它由 `xhjob_start` 通过 spawn + re-exec 拉起后脱离调用方，独立常驻。
- **CLI 与 FPM 都是 IPC 客户端**：两者通过同一个 Unix socket 连同一个 daemon，调用同一套 `xhjob_*` 函数。daemon 不区分请求来自 CLI 还是 FPM。
- **同一 service_name + data_dir → 同一 socket / pid / db**：这是「共用」的唯一前提。socket 文件名为 `<name>.sock`，PID 文件 `<name>.pid`，DB 文件 `<name>.db`，都由 service_name 命名空间化。

### 启动 daemon 的两种方式

| 方式 | 触发者 | 适用场景 | 推荐度 |
|------|--------|----------|--------|
| **CLI `xhjob_start` 长驻** | systemd unit / 部署脚本调用 `xhjob_start($name, $dataDir)` | 生产环境常驻 | ⭐ 推荐 |
| **FPM 请求内 `ensureRunning` 拉起** | FPM worker 首次派发前惰性调用 `XhjobService::ensureRunning()` | 开发 / 测试 / 无 systemd 环境 | 不推荐生产 |

`xhjob_start` 以 spawn + re-exec 方式拉起独立守护进程，调用方（FPM / CLI 短生命周期进程）会**立即返回**，不会阻塞。若 daemon 已在运行（PID 存活且 starttime 匹配），直接返回 `true`，不重复启动。

> 生产环境**强烈建议**用 systemd 托管 daemon（方式一），FPM 请求只做 IPC 客户端。FPM 请求内 `ensureRunning` 拉起在 daemon 死锁时会让 FPM worker 阻塞在 spawn 上，且无法保证 daemon 随机器启动。

### 路径解析优先级

daemon 的 socket / pid / log / db 文件位置由「显式 data_dir 参数 → 环境变量 → 平台默认」三级解析。**注意 sock_dir 与 pid/log/db 的 fallback 链不同**：

| 目录 | 解析优先级（从高到低） |
|------|----------------------|
| sock_dir | 显式 `data_dir` 参数 → `XHJOB_SOCK_DIR` → `XHJOB_DATA_DIR` → `/run/xhjob`（若 /run 存在）→ `/var/run/xhjob`（若 /var/run 存在）→ `/tmp`（兜底） |
| pid_dir | 显式 `data_dir` 参数 → `XHJOB_PID_DIR` → `XHJOB_DATA_DIR` → `/tmp`（**不走 /run 链**） |
| log_dir | 显式 `data_dir` 参数 → `XHJOB_LOG_DIR` → `XHJOB_DATA_DIR` → `/tmp`（**不走 /run 链**） |
| db_dir | 显式 `data_dir` 参数 → `XHJOB_DB_DIR` → `XHJOB_DATA_DIR` → `/tmp`（**不走 /run 链**） |

> 关键差异：仅 sock_dir 走 `/run` → `/var/run` → `/tmp` 三级链；pid / log / db 找不到时只 fallback 到 `/tmp`。生产环境应**显式设置各分项目录**避免歧义。

### 多服务隔离

不同的 `service_name` 完全独立：各自有独立的 daemon 进程、独立的 socket / pid / db 文件。服务名决定文件命名空间（`<name>.sock` / `<name>.pid` / `<name>.log` / `<name>.db`），互不干扰。

```
service_name=cron-svc    → cron-svc.sock / cron-svc.pid / cron-svc.db   (daemon A)
service_name=billing     → billing.sock    / billing.pid    / billing.db (daemon B)
service_name=report      → report.sock     / report.pid     / report.db  (daemon C)
```

服务名校验规则：`^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`（首字符字母，长度 1-32，含字母 / 数字 / 下划线 / 连字符）。非法值**静默回退到 `default`**，不报错。

---

## 完整可运行代码

### 1. systemd 托管 daemon（生产推荐）

```ini
# /etc/systemd/system/xhjob-daemon.service
[Unit]
Description=Xhjob task queue daemon (default service)
After=network.target

[Service]
Type=forking
# daemon 通过 xhjob_start 拉起后写 PID 文件，systemd 据此跟踪
PIDFile=/run/xhjob/default.pid

# 显式设置各路径，避免 fallback 歧义
Environment=XHJOB_SERVICE_NAME=default
Environment=XHJOB_DATA_DIR=/var/lib/xhjob
Environment=XHJOB_SOCK_DIR=/run/xhjob
Environment=XHJOB_PID_DIR=/run/xhjob
Environment=XHJOB_LOG_DIR=/var/log/xhjob
Environment=XHJOB_DB_DIR=/var/lib/xhjob
Environment=XHJOB_PERSIST=true
Environment=XHJOB_POOL_MODE=async
Environment=XHJOB_API_TOKEN=long-random-token

# 通过 PHP 扩展的 xhjob_start 拉起 daemon（spawn + re-exec，立即返回）
ExecStart=/usr/bin/php -d extension=/usr/lib/php/xhjob.so \
    -r 'xhjob_start(getenv("XHJOB_SERVICE_NAME"), getenv("XHJOB_DATA_DIR")) or exit(1);'

# 停止：发 SIGTERM，daemon 排空当前任务后退出
ExecStop=/usr/bin/php -d extension=/usr/lib/php/xhjob.so \
    -r 'xhjob_stop(getenv("XHJOB_SERVICE_NAME"), getenv("XHJOB_DATA_DIR"));'

Restart=on-failure
RestartSec=2s

# 运行用户（与 FPM 同用户，避免 socket 权限问题）
User=www-data
Group=www-data
RuntimeDirectory=xhjob
RuntimeDirectoryMode=0755

[Install]
WantedBy=multi-user.target
```

```bash
# 启用并启动
sudo systemctl daemon-reload
sudo systemctl enable --now xhjob-daemon.service

# 验证 daemon 运行
sudo systemctl status xhjob-daemon.service
# PID 文件双行格式
cat /run/xhjob/default.pid
# 12345
# 8394832   ← starttime（/proc/{pid}/stat 字段 22）
```

### 2. FPM 请求内派发（连同一 daemon）

FPM worker 通过同一 `service_name=default` + `data_dir=/var/lib/xhjob` 连上 systemd 拉起的 daemon，派发后立即返回。

```php
<?php
// app/controller/Order.php  （FPM 请求内）
namespace app\controller;

use Xhjob\TaskBuilder;

class Order
{
    public function pay(): array
    {
        $orderId = (int) ($_POST['order_id'] ?? 0);

        // 派发到 default 服务（与 systemd daemon 共用同一 socket / db）
        $id = TaskBuilder::shell(
                'php /app/bin/after-pay.php ' . escapeshellarg((string) $orderId)
            )
            ->withId('after-pay-' . $orderId)     // 业务幂等
            ->replaceExisting(true)
            ->withRetry(3, 5)
            ->retryBackoff(true)
            ->timeout(120)
            ->dispatch('default', '/var/lib/xhjob');  // 与 systemd 一致

        if (str_starts_with($id, 'error:')) {
            throw new \RuntimeException('派发失败：' . substr($id, 6));
        }
        return ['task_id' => $id, 'order_id' => $orderId];
    }
}
```

### 3. CLI 查状态 / 运维（连同一 daemon）

CLI 脚本通过同一 `service_name` + `data_dir` 连同一 daemon，查询 FPM 派发的任务状态。

```php
#!/usr/bin/env php
<?php
// bin/check-task.php  （CLI 运维）
// 用法：php check-task.php <task_id>

$taskId  = $argv[1] ?? '';
$name    = 'default';           // 与 FPM / systemd 一致
$dataDir = '/var/lib/xhjob';

if ($taskId === '') {
    fwrite(STDERR, "usage: php check-task.php <task_id>\n");
    exit(1);
}

// 确认 daemon 在运行
$status = xhjob_status($name, $dataDir);
if (($status['running'] ?? 'false') !== 'true') {
    fwrite(STDERR, "daemon 未运行\n");
    exit(1);
}
printf("daemon: running, pid=%s\n", $status['pid'] ?? '-');

// 查任务状态
$state = xhjob_state($taskId, $name, $dataDir);
if (isset($state['error'])) {
    fwrite(STDERR, "state error: {$state['error']}\n");
    exit(1);
}
printf("task %s: state=%s attempts=%s progress=%s%%\n",
    $taskId,
    $state['state'] ?? 'UNKNOWN',
    $state['attempts'] ?? '0',
    $state['progress'] ?? '0'
);

// 终态时取结果
$terminal = ['success', 'failed', 'cancelled', 'expired', 'interrupted'];
if (in_array($state['state'] ?? '', $terminal, true)) {
    $result = xhjob_result($taskId, $name, $dataDir);
    if (!isset($result['error'])) {
        printf("exit_code=%s\n", $result['exit_code'] ?? $result['status_code'] ?? '-');
        printf("stdout=%s\n", substr((string) ($result['stdout'] ?? $result['body'] ?? ''), 0, 500));
    }
}
```

### 4. 多服务隔离演示

不同 `service_name` 完全独立，各自的 daemon + DB 互不干扰。

```php
#!/usr/bin/env php
<?php
// bin/multi-service.php  （演示多服务隔离）

// 服务 A：cron 调度（长驻定时任务）
$aName = 'cron-svc';
$aDir  = '/var/lib/xhjob';

// 服务 B：账单队列（高优先级实时任务）
$bName = 'billing';
$bDir  = '/var/lib/xhjob-billing';

// 启动两个独立 daemon
xhjob_start($aName, $aDir);
xhjob_start($bName, $bDir);

// 各自派发（互不影响）
$aId = TaskBuilder::shell('php /app/bin/report.php')
    ->cron('0 2 * * *')
    ->withTimezone('Asia/Shanghai')
    ->dispatch($aName, $aDir);

$bId = TaskBuilder::shell('php /app/bin/charge.php')
    ->withRetry(5, 10)
    ->timeout(30)
    ->dispatch($bName, $bDir);

printf("cron-svc task: %s\n", $aId);
printf("billing  task: %s\n", $bId);

// 查询各自 daemon 状态（独立 PID）
$aStatus = xhjob_status($aName, $aDir);
$bStatus = xhjob_status($bName, $bDir);
printf("cron-svc daemon: pid=%s\n", $aStatus['pid'] ?? '-');
printf("billing  daemon: pid=%s\n", $bStatus['pid'] ?? '-');

// 停一个不影响另一个
xhjob_stop($aName, $aDir);
printf("after stop cron-svc: billing still running=%s\n",
    xhjob_status($bName, $bDir)['running'] ?? 'false');
```

### 5. PID 文件双行格式与 PID 复用防护

daemon 启动时写 PID 文件，格式为双行：第一行 PID，第二行 starttime（`/proc/{pid}/stat` 字段 22，进程启动时的时钟 ticks）。

```bash
# 正常运行的 PID 文件
$ cat /run/xhjob/default.pid
12345
8394832

# 模拟 PID 复用：旧 daemon 死后 PID 12345 被 nginx 复用
$ ps -p 12345 -o pid,comm
  PID COMM
12345 nginx

# 新 daemon 启动 → 读 PID 文件 → starttime 不匹配 → 视为 stale → 清理
$ systemctl restart xhjob-daemon
# daemon 日志：
# INFO xhjob::pid] stale pid 12345 (starttime mismatch: file=8394832, actual=9201111), cleaning
# INFO xhjob::daemon] starting fresh daemon, pid=12500
```

```php
<?php
// 读取并校验 PID 文件（运维脚本示例）
function readDaemonPid(string $pidFile): ?array
{
    if (!is_file($pidFile)) {
        return null;
    }
    $lines = file($pidFile, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
    if (count($lines) < 1) {
        return null;
    }
    $pid = (int) $lines[0];
    $starttime = $lines[1] ?? null;

    // 校验 PID 存活
    if (!file_exists("/proc/{$pid}")) {
        return null;  // 进程已死
    }
    // 校验 starttime（防 PID 复用）
    if ($starttime !== null) {
        $stat = file_get_contents("/proc/{$pid}/stat");
        $fields = explode(' ', $stat);
        $actualStarttime = $fields[21] ?? '';  // 字段 22（0-indexed = 21）
        if ($actualStarttime !== $starttime) {
            return null;  // PID 被复用，旧 daemon 已死
        }
    }
    return ['pid' => $pid, 'starttime' => $starttime];
}

$info = readDaemonPid('/run/xhjob/default.pid');
echo $info ? "daemon alive, pid={$info['pid']}" : "daemon dead (stale pid)";
```

### 6. `XHJOB_IPC_TIMEOUT_SECS` 防 FPM worker 阻塞

PHP 的 `max_execution_time` **无法中断** C 级 socket 阻塞（`recv` / `connect`）。若 daemon 死锁，FPM worker 会被永久挂住，直到 IPC 超时。`XHJOB_IPC_TIMEOUT_SECS`（默认 5 秒）是兜底机制。

```ini
; /etc/php/8.2/fpm/pool.d/www.conf  （FPM worker 环境变量）
; 把 IPC 超时设得比 max_execution_time 小，让 FPM worker 快速失败
env[XHJOB_IPC_TIMEOUT_SECS] = 3
```

```php
<?php
// FPM 请求内：捕获 IPC 超时并降级
$id = '';
try {
    $id = TaskBuilder::shell('php /app/bin/job.php')
        ->dispatch('default', '/var/lib/xhjob');
} catch (\Throwable $e) {
    // IPC 超时 / daemon 不可达：降级到本地 fallback 队列
    error_log('xhjob dispatch failed, fallback: ' . $e->getMessage());
    $id = enqueueFallback($_POST['job_payload']);
}
if (str_starts_with($id, 'error:')) {
    $id = enqueueFallback($_POST['job_payload']);
}
return ['task_id' => $id];
```

---

## 注意事项

- **daemon 用 systemd 托管最稳**：systemd 提供 `Restart=on-failure` 自动拉起、`Type=forking` + `PIDFile` 进程跟踪、`RuntimeDirectory` 自动创建 `/run/xhjob`。不要依赖 FPM 请求内 `ensureRunning` 拉起——daemon 死锁时 FPM worker 会阻塞在 spawn 上。
- **FPM worker 设较小的 `XHJOB_IPC_TIMEOUT_SECS`**：默认 5 秒对 FPM 偏长。建议设 `3` 秒（小于 FPM 的 `max_execution_time`），让 FPM worker 在 daemon 不可达时快速失败降级，而非占住 worker。CLI worker 可设大一些（如 10 秒），因为 CLI 不怕短暂阻塞。
- **共用前提是 service_name + data_dir 完全一致**：FPM 用 `dispatch('default', '/var/lib/xhjob')`，CLI 也必须用 `xhjob_state($id, 'default', '/var/lib/xhjob')`。任一参数不一致会连到不同 daemon（或连不上）。封装一个统一配置（环境变量或配置文件）避免硬编码不一致。
- **socket 权限**：daemon 与 FPM 必须以同一用户运行（或 socket 目录对 FPM 用户可读写），否则 FPM 连不上 socket。systemd unit 设 `User=www-data`（与 FPM pool 用户一致），`RuntimeDirectoryMode=0755`。
- **PID 文件双行格式防复用**：不能只校验 PID 存活——Linux 会复用 PID，旧 daemon 死后新进程可能拿到同样 PID。starttime（`/proc/{pid}/stat` 字段 22）单调递增，跨重启必然不同，是可靠的存活证据。旧格式单行 PID 文件向后兼容，但生产环境应确认双行格式已生效。
- **`/run` 是 tmpfs**：`/run/xhjob` 重启清空，适合放 socket / pid（ ephemeral）。`/var/lib/xhjob` 放 SQLite DB（持久），`/var/log/xhjob` 放日志（持久）。不要把 DB 放 `/run`，否则机器重启丢全部任务定义。
- **多服务不要共用 data_dir**：虽然不同 service_name 的 DB 文件名不同（`<name>.db`），但共用同一目录会增加混乱。建议每个服务独立 data_dir（如 `/var/lib/xhjob` 与 `/var/lib/xhjob-billing`），便于备份与迁移。

---

## 生产建议

- **systemd unit 模板化**：用 systemd 模板单元（`xhjob-daemon@.service`）支持多服务，`%i` 实例名映射到 service_name。`systemctl enable xhjob-daemon@billing.service` 即可拉起 billing 服务的独立 daemon。
- **健康检查接入 K8s / 负载均衡**：`xhjob_status($name, $dataDir)['running'] === 'true'` 即视为健康。可写一个简单的 PHP 探活脚本（`php -r 'echo xhjob_status()["running"];'`）供 liveness probe 调用。
- **FPM 与 CLI 共用扩展 ini**：用 `PHP_INI_SCAN_DIR` 让 FPM 与 CLI 共享同一 xhjob 扩展 ini，避免两边版本不一致。CLI 调试时 `php -d extension=... -m | grep xhjob` 确认加载。
- **滚动发布时先停 FPM 再重启 daemon**：发版时若先 `systemctl restart xhjob-daemon`，正在处理中的 FPM 请求会因 IPC 断开失败。正确顺序：`systemctl reload php8.2-fpm`（停止接受新请求）→ 等存量请求排空 → `systemctl restart xhjob-daemon` → `systemctl reload php8.2-fpm`。`persist(true)` + `acksLate(true)` 保证 daemon 重启期间在途任务恢复。
- **监控 daemon PID 变化**：`xhjob_status` 返回的 `pid` 应长期稳定。若 pid 频繁变化，说明 daemon 在反复崩溃重启，应查日志（`/var/log/xhjob/<name>.log`）定位。可配告警：5 分钟内 pid 变化超过 3 次即告警。
- **`XHJOB_IPC_NO_PEERCRED` 谨慎开启**：默认 daemon 用 `SO_PEERCRED` 校验 IPC 连接的发起方 UID（要求同用户）。跨用户访问（如 root daemon + www-data FPM）需设 `XHJOB_IPC_NO_PEERCRED=1` 跳过校验，但这降低安全性。优先方案是把 daemon 与 FPM 设为同一用户。
