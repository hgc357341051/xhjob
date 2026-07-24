---
title: 生产实战：后台任务队列
parent: 生产实战
nav_order: 51
---

# 生产实战：后台任务队列

本篇演示如何用 xhjob 把**耗时的后台处理**从 FPM Web 请求中剥离：Web 请求只做"非阻塞派发 + 立即返回 task_id"，真正的执行交给独立 daemon，前端再通过 CLI worker 或轮询接口拿结果与进度。这是 APScheduler / Celery 在单机 PHP 场景下的等价做法，无需引入 Redis / RabbitMQ / Supervisor。

## 架构

```
┌──────────────┐   ① xhjob_dispatch（非阻塞，立即返回 task_id）   ┌──────────────────────┐
│  FPM Web 请求 │ ──────────────────────────────────────────────▶ │  Rust daemon         │
│  (PHP)       │                                                 │  ┌────────────────┐  │
└──────────────┘                                                 │  │ 队列调度器      │  │
       ▲                                                         │  │ (rate_limit/    │  │
       │ ⑤ HTTP 返回 task_id 给前端                                │  │  retry/coalesce)│  │
       │                                                         │  └───────┬────────┘  │
       │                                                         │          ▼           │
       │                                                         │  ┌────────────────┐  │
       │                                                         │  │ executor        │  │
       │                                                         │  │ (shell/http)    │  │
       │                                                         │  └───────┬────────┘  │
       │                                                         │          ▼           │
       │                                                         │  ┌────────────────┐  │
       │                                                         │  │ store           │  │
       │                                                         │  │ (InMemory/SQLite)│ │
       │                                                         │  └───────▲────────┘  │
       │                                                         └──────────┼───────────┘
       │                                                                    │
       │ ④ xhjob_result($id) / xhjob_state($id)                             │
┌──────┴─────────┐   ② while 轮询状态/结果 + 超时处理                          │
│  CLI worker    │ ◀─────────────────────────────────────────────────────────┘
│  (php 进程)    │   ③ xhjob_report_progress($id, $pct, $meta)
└────────────────┘      （任务脚本内部上报进度，写入 store）
```

数据流：

1. **FPM 侧**调用 `xhjob_dispatch`（或 `TaskBuilder::...->dispatch()`）非阻塞派发，立即拿到 `task_id`。
2. **CLI worker**（或前端轮询）通过 `xhjob_state($id)` / `xhjob_result($id)` 查询进度与结果。
3. **任务脚本**在执行过程中调用 `xhjob_report_progress($id, $pct, $meta)` 上报进度，进度写入 store。
4. CLI worker 调 `xhjob_result($id)` 取最终 stdout / stderr / exit_code。
5. FPM 把 `task_id` 返回给前端，前端用该 id 轮询状态接口。

## FPM 侧：非阻塞派发

FPM worker 的核心原则是**派发即返回，绝不阻塞等待结果**。`xhjob_dispatch` 本质是一次本地 Unix socket IPC（默认 5s 超时保护），提交后立即返回 `task_id`。

```php
<?php
// === FPM Web 请求内：派发后台任务 ===
use Xhjob\TaskBuilder;
use Xhjob\XhjobService;

// 确保 daemon 已启动（幂等，已运行则直接返回）
$svc = new XhjobService('queue-svc', '/var/lib/xhjob');
$svc->ensureRunning();

// 业务数据（来自 HTTP 请求）
$data = $_POST['payload'] ?? '';
$jobArg = escapeshellarg($data);

// 非阻塞派发：立即返回 task_id
$taskId = TaskBuilder::shell('php /app/jobs/process.php ' . $jobArg)
    ->ignoreResult(false)                  // 需要取结果，保留 stdout
    ->rateLimit(100, 60)                   // 限流：60 秒内最多 100 个，防雪崩
    ->withRetry(3, 5)                      // 失败重试 3 次，间隔 5 秒
    ->retryBackoff(true)                   // 指数退避：5,10,20... 上限 5*60
    ->timeout(300)                         // 单次执行硬超时 5 分钟
    ->softTimeout(270)                     // 软超时 4.5 分钟，先 SIGTERM 再 SIGKILL
    ->dispatch('queue-svc', '/var/lib/xhjob');

// 立即把 task_id 返回给前端，前端用它轮询状态
header('Content-Type: application/json');
echo json_encode([
    'task_id' => $taskId,
    'status'  => 'dispatched',
    // 前端轮询：GET /jobs/status?task_id=xxx
    'poll'    => '/jobs/status?task_id=' . urlencode($taskId),
]);
```

{: .warning }
> **绝对不要在 FPM 请求内调用 `waitForResult` / `waitForState`**。这两个方法会轮询 IPC，把 FPM worker 钉死在请求上，迅速耗尽 worker 池导致 502/504。需要同步等结果时改用 CLI worker 或独立轮询服务。

## CLI worker：轮询结果

CLI worker 是一个常驻 `php` 进程，负责消费 `task_id` 队列、轮询状态、取结果、处理超时。它和 FPM 共用同一个 `service_name` + `data_dir`，因此连的是同一个 daemon。

```php
<?php
// === CLI worker：worker.php ===
// 运行：nice -n 10 php /app/worker.php
use Xhjob\TaskManager;

$mgr = new TaskManager('queue-svc', '/var/lib/xhjob');

// 待处理的 task_id 队列（实际从 DB / Redis / 文件 / 消息队列读取）
$pendingIds = fetchPendingTaskIds();

foreach ($pendingIds as $id) {
    $timeoutSec = 600;          // 单任务最长等待 10 分钟
    $deadline   = time() + $timeoutSec;

    while (time() < $deadline) {
        try {
            $state = $mgr->state($id);           // 查状态
        } catch (\Throwable $e) {
            // daemon 重启中等临时错误，退避后重试
            usleep(500000);
            continue;
        }

        $s = $state['state'] ?? 'unknown';
        if (in_array($s, ['success', 'failed', 'cancelled', 'expired', 'interrupted'], true)) {
            // 终态：取结果并退出
            $result = $mgr->result($id);
            handleTerminal($id, $s, $result);     // 业务处理：写 DB / 通知前端
            break;
        }

        // 非终态：打印进度，继续轮询
        $pct = $state['progress_percent'] ?? null;
        if ($pct !== null) {
            echo "[{$id}] 进度 {$pct}%\n";
        }
        usleep(300000);                          // 300ms 轮询
    }

    if (time() >= $deadline) {
        // 超时未完成：取消任务并标记
        $mgr->stop($id);                          // 取消（xhjob_cancel）
        markTimeout($id);
    }
}
```

CLI worker 进入终态后退出本轮循环，可由 systemd / supervisor 拉起下一轮，或循环消费新 id。

## 进度上报

长任务应向前端反馈进度。任务脚本内部调用 `xhjob_report_progress` 上报百分比与任意 JSON 元数据，daemon 将其写入 store，`xhjob_state` 的返回值即携带进度。

```php
<?php
// === 任务脚本：/app/jobs/process.php ===
// 由 daemon 以 shell 任务拉起，$argv[1] 是业务数据
$taskId = $argv[1] ?? '';
$total  = 1000;

for ($i = 1; $i <= $total; $i++) {
    processOneItem($i);                          // 实际业务处理

    // 每 100 条上报一次进度（避免 IPC 过频）
    if ($i % 100 === 0) {
        $pct = (int) floor($i / $total * 100);
        $meta = json_encode([
            'processed' => $i,
            'total'     => $total,
            'items'     => $i,
        ]);
        // 上报进度：task_id、百分比、元数据 JSON
        xhjob_report_progress($taskId, $pct, $meta);
    }
}

// 正常退出，exit_code=0 → daemon 标记 Success
exit(0);
```

前端 / CLI worker 查询进度：

```php
$state = $mgr->state($taskId);
// $state 含 state / progress_percent / progress_meta 等字段
echo "状态={$state['state']} 进度={$state['progress_percent']}%\n";
```

## 失败重试配置

`withRetry` + `retryBackoff` 是后台队列的可靠性基石：

```php
TaskBuilder::shell('php /app/jobs/process.php ' . $jobArg)
    ->withRetry(3, 5)          // 最多重试 3 次，基础间隔 5 秒
    ->retryBackoff(true)       // 启用指数退避：5, 10, 20... 上限 retry_delay*60=300s
    ->acksOnFailure(false)     // 失败时不 ack，便于重试（配合 acksLate 使用）
    ->dispatch();
```

- `withRetry(3, 5)`：任务失败（非零退出码 / 超时）后，最多重试 3 次，每次基础间隔 5 秒。
- `retryBackoff(true)`：启用指数退避，重试延迟按 `min(retry_delay * 2^(attempts-1), retry_delay * 60)` 增长，避免短时间内反复冲击失败的下游。
- `acksOnFailure(false)`：失败时不确认，配合 `acksLate(true)` 可在 daemon 崩溃后重投（见[定时任务](prod-cron/)与崩溃恢复文档）。

## 注意事项

| 关注点 | 建议 |
|------|------|
| **FPM 内不要 `waitForResult`** | `waitForResult` / `waitForState` 会轮询 IPC 钉死 FPM worker，迅速耗尽 worker 池。FPM 只做派发，等结果交给 CLI worker 或前端轮询。 |
| **worker 进程用 `nice` 降权** | CLI worker 与 daemon 同机运行，用 `nice -n 10 php worker.php` 降低其 CPU 优先级，避免与 FPM 抢资源。 |
| **`ignoreResult(true)` 省 DB 写入** | 不需要取结果的高吞吐任务（如纯通知、心跳）设 `ignoreResult(true)`，daemon 跳过 `save_result`，减少 SQLite 写入。本篇因需取结果，故设 `false`。 |
| **高并发用 `rateLimit` 防雪崩** | `rateLimit(100, 60)` 限制 60 秒窗口内最多 100 次执行，避免下游被瞬时流量压垮。 |
| **IPC 超时保护** | 所有 `xhjob_*` 通信默认 5s 超时（`XHJOB_IPC_TIMEOUT_SECS`），daemon 死锁时 FPM worker 不会被永久阻塞。 |
| **任务幂等性** | 开启重试 / `acksLate` 后任务可能被重复执行，业务逻辑必须幂等（如用唯一键去重、状态机判断）。 |
| **进度上报频率** | 上报过频会增加 IPC 开销，建议按批次（如每 100 条 / 每 5%）上报一次。 |

## 完整端到端示例

把上述三段拼起来：FPM 派发 → 任务脚本上报进度 → CLI worker 取结果。

```php
<?php
// === 1. FPM 派发（Web 请求内）===
$taskId = TaskBuilder::shell('php /app/jobs/process.php ' . escapeshellarg($payload))
    ->rateLimit(100, 60)
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->timeout(300)
    ->dispatch('queue-svc', '/var/lib/xhjob');
// 返回 task_id 给前端

// === 2. 任务脚本上报进度（/app/jobs/process.php 内）===
xhjob_report_progress($taskId, 50, json_encode(['items' => 500]));

// === 3. CLI worker 取结果（worker.php 内）===
$mgr = new \Xhjob\TaskManager('queue-svc', '/var/lib/xhjob');
if ($mgr->waitForState($taskId, 'success', 600)) {
    $r = $mgr->result($taskId);
    echo "stdout: " . ($r['stdout'] ?? '') . "\n";
    echo "exit_code: " . ($r['exit_code'] ?? -1) . "\n";
}
```

> `waitForState` 仅在 **CLI worker** 中使用，绝不在 FPM 请求内调用。
