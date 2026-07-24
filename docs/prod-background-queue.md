# 后台任务队列

> 生产实战篇 · FPM 非阻塞派发 · daemon 队列执行 · CLI worker 轮询结果 · 进度上报 · 失败重试

Web 场景下，PHP-FPM 请求的生命周期通常只有几百毫秒到几秒，无法承载耗时几分钟的报表生成、视频转码、批量推送等任务。Xhjob 的后台任务队列模式把「派发」与「执行」解耦：FPM 请求只负责把任务投递到 daemon 队列后立即返回 task_id，真正的执行在独立 Rust daemon 内异步完成；需要结果的业务方再由 CLI worker 轮询拉取。

本文档按 **架构说明 → 完整可运行代码 → 注意事项 → 生产建议** 的结构展开。

---

## 架构说明

```
┌────────────────┐   dispatch (非阻塞 IPC)   ┌─────────────────────────────────────────────┐
│  FPM 请求      │ ════════════════════════► │              daemon (Rust)                  │
│  (毫秒级)      │ ◄════════════════════════  │                                             │
│  返回 task_id  │   立即回 task_id           │  ┌───────────┐   ┌───────────┐   ┌────────┐ │
└────────────────┘                           │  │ scheduler │──►│  pool     │──►│executor│ │
                                             │  │ (队列)    │   │ async/    │   │shell/  │ │
┌────────────────┐   state / result 轮询     │  └───────────┘   │ thread    │   │http    │ │
│  CLI worker    │ ════════════════════════► │                  └───────────┘   └───┬────┘ │
│  (长驻轮询)    │ ◄════════════════════════  │                                    │      │
│  拉取结果/状态  │   state / result JSON     │  ┌─────────────────────────────────▼────┐ │
└────────────────┘                           │  │  store (SQLite WAL / InMemory)        │ │
                                             │  │  任务定义 · 状态机 · 结果 · 事件流     │ │
                                             │  └──────────────────────────────────────┘ │
                                             └─────────────────────────────────────────────┘
```

数据流分三段：

1. **派发（FPM → daemon）**：FPM worker 调用 `TaskBuilder::shell(...)->dispatch()`，扩展经 Unix socket 把任务 JSON 发给 daemon 后**立即返回 task_id**，IPC 耗时通常在毫秒级。FPM 请求即可结束，不等待任务执行。
2. **执行（daemon 内部）**：daemon scheduler 从队列取出任务，交给 pool（async 协程池或 thread 线程池），executor 执行 shell/http，结果与状态写入 store。
3. **消费（CLI worker ← daemon）**：独立的 CLI 长驻进程通过 `xhjob_state` / `xhjob_result` 轮询任务状态与结果，进入终态后取回结果并落库或回调业务。

> PHP 进程（无论 FPM 还是 CLI）都是**瘦 IPC 客户端**，不持有队列。重启 PHP-FPM 不影响 daemon 中正在执行的任务。

---

## 完整可运行代码

### 1. FPM 侧：非阻塞派发

FPM 请求内构建任务并派发，立即返回 task_id 给前端。对不需要回读结果的 fire-and-forget 任务，启用 `ignoreResult(true)` 节省 DB 写入；用 `rateLimit` 防止瞬时打爆下游；用 `withRetry` + `retryBackoff` 应对临时失败。

```php
<?php
// app/controller/Report.php  （FPM 请求内）
namespace app\controller;

use Xhjob\TaskBuilder;

class Report
{
    /**
     * POST /report/generate
     *
     * 入队一个报表生成任务，立即返回 task_id，前端凭 task_id 轮询进度。
     */
    public function generate(): array
    {
        $userId = (int) ($_POST['user_id'] ?? 0);
        $date   = $_POST['date'] ?? date('Y-m-d');

        // 业务幂等键：同一用户同一天只允许一个在途任务
        $taskId = 'report-' . $userId . '-' . str_replace('-', '', $date);

        // 派发 shell 任务：报表脚本接收 task_id 以便上报进度
        $id = TaskBuilder::shell(
                'php /app/bin/generate-report.php ' . escapeshellarg($taskId)
                    . ' ' . escapeshellarg((string) $userId)
                    . ' ' . escapeshellarg($date)
            )
            ->withId($taskId)              // 业务幂等 ID（TaskBuilder PHP 类用 withId）
            ->replaceExisting(true)        // 同 id 覆盖，防重复派发
            ->ignoreResult(true)           // fire-and-forget：FPM 不读结果，节省 DB
            ->rateLimit(100, 60)           // 60 秒内最多 100 个，防爆队列
            ->withRetry(3, 5)              // 失败重试 3 次，间隔 5 秒
            ->retryBackoff(true)           // 指数退避：5s → 10s → 20s
            ->timeout(600)                 // 单次执行硬超时 10 分钟
            ->softTimeout(540)             // 软超时 9 分钟，给业务优雅退出
            ->maxInstances(1)              // 同任务最多 1 个实例并发
            ->tag('report')
            ->withMeta(json_encode(['user_id' => $userId, 'date' => $date]))
            ->dispatch('default', '/var/lib/xhjob');  // 指定服务名 + 数据目录

        // dispatch 成功返回 task_id；失败返回 "error: ..." 字符串
        if (str_starts_with($id, 'error:')) {
            throw new \RuntimeException('派发失败：' . substr($id, 6));
        }

        // 立即返回，不等任务执行
        return ['task_id' => $id, 'status' => 'queued'];
    }
}
```

### 2. CLI worker：轮询状态与结果

对于需要回读结果的任务（**不要**设 `ignoreResult(true)`），用一个独立 CLI 长驻进程轮询。下面是一个通用 worker，从业务队列拉取待消费的 task_id，轮询 `xhjob_state` 确认终态后用 `xhjob_result` 取回结果。

```php
#!/usr/bin/env php
<?php
// bin/result-worker.php  （CLI 长驻，systemd 托管）
//
// 用途：消费业务侧记录的 task_id 队列，轮询 xhjob 取回结果后回写业务库。
// 启动：php bin/result-worker.php
// 托管：systemd unit（Restart=on-failure）

$name    = 'default';
$dataDir = '/var/lib/xhjob';
$pollInterval = 2;       // 轮询间隔（秒）
$maxWaitSecs  = 1800;    // 单任务最长等待 30 分钟

// 从业务 DB 取出待消费的 task_id 列表（示例用文件代替）
function fetchPendingTaskIds(): array
{
    $file = '/var/run/xhjob-result-queue.json';
    if (!is_file($file)) {
        return [];
    }
    $ids = json_decode((string) file_get_contents($file), true) ?: [];
    return is_array($ids) ? $ids : [];
}

function markConsumed(string $taskId): void
{
    $file = '/var/run/xhjob-result-queue.json';
    $ids  = fetchPendingTaskIds();
    unset($ids[$taskId]);
    file_put_contents($file, json_encode(array_values($ids)), LOCK_EX);
}

function consumeResult(string $taskId, int $maxWaitSecs): void
{
    global $name, $dataDir;
    $deadline = time() + $maxWaitSecs;
    $terminal = ['success', 'failed', 'cancelled', 'expired', 'interrupted'];

    while (time() < $deadline) {
        $state = xhjob_state($taskId, $name, $dataDir);

        if (isset($state['error'])) {
            // daemon 不可达或任务不存在：退避后重试，不放弃
            fwrite(STDERR, "[" . date('c') . "] state error: {$state['error']}\n");
            sleep(5);
            continue;
        }

        $s = $state['state'] ?? 'UNKNOWN';
        $progress = isset($state['progress']) ? (int) $state['progress'] : 0;
        fwrite(STDOUT, "[" . date('c') . "] {$taskId} state={$s} progress={$progress}%\n");

        if (in_array($s, $terminal, true)) {
            // 进入终态，取结果
            $result = xhjob_result($taskId, $name, $dataDir);
            if (isset($result['error'])) {
                fwrite(STDERR, "[" . date('c') . "] {$taskId} no result: {$result['error']}\n");
            } else {
                $exitCode = $result['exit_code'] ?? ($result['status_code'] ?? '-');
                $stdout   = $result['stdout'] ?? $result['body'] ?? '';
                fwrite(STDOUT, "[" . date('c') . "] {$taskId} DONE exit={$exitCode}\n");
                fwrite(STDOUT, "  stdout=" . substr((string) $stdout, 0, 500) . "\n");
                // TODO: 回写业务库 / 触发回调
            }
            return;
        }
        sleep($pollInterval ?? 2);
    }

    // 超时未完成：记录告警，交给人工或重试机制
    fwrite(STDERR, "[" . date('c') . "] {$taskId} TIMEOUT after {$maxWaitSecs}s\n");
}

// 主循环
fwrite(STDOUT, "[" . date('c') . "] result-worker started\n");
while (true) {
    $ids = fetchPendingTaskIds();
    foreach ($ids as $taskId) {
        consumeResult((string) $taskId, $maxWaitSecs);
        markConsumed((string) $taskId);
    }
    sleep($pollInterval);
}
```

### 3. 进度上报：任务脚本内调用 `xhjob_report_progress`

长任务脚本通过自身的 task_id 上报进度，前端 / worker 可通过 `xhjob_state` 的 `progress` / `progress_meta` 字段读回。

```php
#!/usr/bin/env php
<?php
// bin/generate-report.php  （被 daemon shell executor 拉起的子进程）
//
// 用法：php generate-report.php <task_id> <user_id> <date>
// task_id 由 FPM 派发时传入，用于上报进度。

$taskId = $argv[1] ?? '';
$userId = (int) ($argv[2] ?? 0);
$date   = $argv[3] ?? date('Y-m-d');

if ($taskId === '') {
    fwrite(STDERR, "missing task_id\n");
    exit(1);
}

$steps = [
    'fetch-data'    => '从 DB 拉取原始数据',
    'aggregate'     => '聚合统计',
    'render'        => '渲染模板',
    'upload'        => '上传到对象存储',
    'notify'        => '发送通知',
];

$total = count($steps);
$i = 0;
foreach ($steps as $key => $desc) {
    $i++;
    $percent = (int) round($i / $total * 100);

    // 上报进度：0-100，越界返回 false（不会联系 daemon）
    $ok = xhjob_report_progress(
        $taskId,
        $percent,
        json_encode(['step' => $key, 'desc' => $desc], JSON_UNESCAPED_UNICODE)
    );
    if (!$ok) {
        fwrite(STDERR, "report_progress failed at {$percent}% (step={$key})\n");
        // 进度上报失败不阻断业务，继续执行
    }

    // 执行实际业务（模拟）
    doStep($key, $userId, $date);
}

// stdout 会被 daemon 捕获为结果（除非 ignoreResult=true）
echo json_encode(['task_id' => $taskId, 'url' => '/reports/' . $taskId . '.xlsx']);
exit(0);

function doStep(string $key, int $userId, string $date): void
{
    // ... 实际业务逻辑 ...
    usleep(500000); // 模拟耗时
}
```

### 4. 失败重试配置

`withRetry` 设定最大重试次数与基础间隔，`retryBackoff(true)` 启用指数退避（`min(delay * 2^(attempts-1), delay * 60)`）。两者组合是后台任务对抗临时故障的核心手段。

```php
<?php
use Xhjob\TaskBuilder;

// 调用第三方 HTTP 接口：临时 5xx 自动重试，指数退避避免雪崩
$id = TaskBuilder::http('POST', 'https://api.example.com/notify')
    ->withBody(json_encode(['event' => 'order.paid', 'order_id' => 42]))
    ->withHeaders(['Authorization' => 'Bearer ' . getenv('API_TOKEN')])
    ->idempotent(true)            // 声明幂等，允许 POST 重试
    ->withRetry(5, 10)            // 最多重试 5 次，基础间隔 10 秒
    ->retryBackoff(true)          // 退避：10s → 20s → 40s → 60s(封顶) → 60s
    ->timeout(30)                 // 单次请求 30 秒超时
    ->acksOnFailure(true)         // 失败后确认（不无限重试），最终落 failed 终态
    ->resultTtl(86400)            // 结果保留 1 天供排查
    ->dispatch();

// shell 任务：依赖外部资源（DB / 文件）偶发失败，重试 + 退避
$id = TaskBuilder::shell('php /app/bin/import-csv.php --file=/data/today.csv')
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->softTimeout(50)->timeout(60)
    ->maxInstances(1)             // 防止重叠导入
    ->dispatch();
```

---

## 注意事项

- **FPM 请求内严禁 `waitForResult` / `waitForState` 长轮询**：`TaskManager::waitForResult()` 内部是 `while` 轮询，会占住 FPM worker 直到超时。FPM worker 数量有限（通常 5~50），几个长轮询就能把整个站点拖垮。轮询只在 CLI worker 内做。
- **worker 用 `nice` 降权**：CLI worker 是常驻进程，应 `nice -n 10 php bin/result-worker.php` 降低调度优先级，避免与 FPM / Nginx 抢 CPU。
- **`ignoreResult(true)` 节省 DB**：fire-and-forget 任务不写结果表，减少 SQLite 写入压力。但一旦设置，`xhjob_result` 永远返回 `error`，CLI worker 无法取回输出——只适合「不关心结果」的场景（如发通知）。需要回读结果的任务**不要**设此项。
- **`rateLimit` 防爆**：批量派发时务必设 `rateLimit(count, window)`，否则瞬时大量任务会打爆下游 API 或 daemon 队列。限流是滑动窗口语义，`count=0` 关闭。
- **`maxInstances(1)` 防重叠**：对不允许并发执行的任务（如数据导入），设 `maxInstances(1)`，前一轮未完成时新一轮会被跳过并记 `max_instances_reached` 事件。
- **进度上报频率**：`xhjob_report_progress` 每次都是一次 IPC 往返，建议按 5%~10% 步进上报，不要每个循环都调。越界值（非 0-100）直接返回 `false`，不联系 daemon。
- **task_id 传递**：任务脚本需要知道自己的 task_id 才能上报进度。派发时把 task_id 作为 shell 参数传入（见上方代码）。若用自动生成的 UUID，派发后把返回的 id 写入脚本能读到的位置（如环境变量 `XHJOB_TASK_ID`，由 shell executor 注入）。

---

## 生产建议

- **daemon 用 systemd 托管**：不要让 FPM 请求拉起 daemon（`ensureRunning`）。生产环境 daemon 应由 systemd 长驻（`Restart=on-failure`），FPM / CLI 都只做 IPC 客户端。详见 [CLI 与 FPM 共用服务连接](prod-cli-fpm-share.md)。
- **结果落库后及时 `remove`**：CLI worker 取回结果并写入业务库后，调 `xhjob_remove($taskId)` 清理 daemon 侧的任务记录与结果，避免 SQLite 表无限膨胀。对 `ignoreResult(true)` 的任务也建议定期清理。
- **监控三件套**：用 `xhjob_inspect('stats')` 看队列深度与 worker 负载；用 `xhjob_pull_events(time() - 600, 'failed')` 拉取最近 10 分钟失败事件接告警；用 `xhjob_pull_events(time() - 600, 'max_instances_reached')` 监控被跳过的任务。
- **FPM 侧设 `XHJOB_IPC_TIMEOUT_SECS`**：FPM worker 派发时若 daemon 卡死，IPC 会阻塞。设 `XHJOB_IPC_TIMEOUT_SECS=3`（小于 FPM 的 `max_execution_time`），让 FPM worker 快速失败而非挂死。PHP 的 `max_execution_time` **无法中断** C 级 socket 阻塞，必须靠此环境变量兜底。
- **队列容量**：`XHJOB_MAX_PENDING`（默认 10000）限制待处理任务上限。派发超过上限会返回 `error:`，业务侧应捕获并降级（如写入本地 fallback 队列稍后重投）。
- **进程隔离**：CLI worker 与 FPM 共用同一 daemon（同 `service_name` + `data_dir`），但 worker 进程本身独立。建议 worker 单独部署在非 Web 节点，只通过网络（或共享 socket 目录）连 daemon，避免与 Web 流量争抢 CPU。
