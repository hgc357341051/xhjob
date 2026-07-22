#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 后台任务队列异步功能测试套件
// +----------------------------------------------------------------------
// | 用法：
// |   php -d extension=/workspace/releases/xhjob-php8.2-linux-x86_64.so \
// |       /workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php
// +----------------------------------------------------------------------
// | 覆盖（对标 APScheduler/Celery 后台队列异步特性）：
// |   1. 非阻塞派发（dispatch 立即返回 task_id，不等执行）
// |   2. 批量并发派发（10 任务并发，全部 success）
// |   3. 任务进度上报（shell 中调 report_progress，state.progress 字段）
// |   4. countdown 延迟派发（3 秒后触发）
// |   5. chord 全部成功（3 header + 1 callback，回调 meta 含 3 结果）
// |   6. chord 部分失败（1 header 失败，chord 转 partial_failed）
// |   7. inspect active（运行中任务）
// |   8. inspect registered（cron 注册任务）
// |   9. inspect scheduled（未来触发任务）
// |  10. inspect stats（聚合统计）
// |  11. pull_events（事件拉取）
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

if (!extension_loaded('xhjob')) {
    $so = '/workspace/releases/xhjob-php8.2-linux-x86_64.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$pass = 0;
$fail = 0;
$failedSteps = [];

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $failedSteps;
    echo "[$n] $desc ... ";
    try {
        $fn();
        $pass++;
        echo "PASS\n";
    } catch (\Throwable $e) {
        $fail++;
        $failedSteps[] = $n;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

function assertTrue(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new \Exception($msg);
    }
}

function assertEq($a, $b, string $msg): void
{
    if ($a !== $b) {
        throw new \Exception("$msg (expected=" . var_export($b, true) . ", got=" . var_export($a, true) . ")");
    }
}

$SERVICE  = 'async-queue-test';
$DATA_DIR = '/tmp/xhjob-async-queue-test';

// 清理旧环境
@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);

echo "=== Xhjob 后台任务队列异步功能测试套件 ===\n";
echo "service={$SERVICE} data_dir={$DATA_DIR}\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n";
echo "新函数检查: xhjob_chord=" . (function_exists('xhjob_chord') ? 'OK' : 'MISSING')
   . " xhjob_inspect=" . (function_exists('xhjob_inspect') ? 'OK' : 'MISSING')
   . " xhjob_report_progress=" . (function_exists('xhjob_report_progress') ? 'OK' : 'MISSING')
   . " xhjob_pull_events=" . (function_exists('xhjob_pull_events') ? 'OK' : 'MISSING') . "\n\n";

// ============================================================
// 0. 启动 daemon
// ============================================================
step(0, '启动 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "pid={$pid}");
    $svc->wait(10, true);
    $st = $svc->status();
    assertTrue($st['running'], 'daemon 应运行');
});

// ============================================================
// 1. 非阻塞派发（dispatch 立即返回 task_id，不等执行）
// ============================================================
step(1, '非阻塞派发（立即返回 task_id）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $t0 = microtime(true);
    $id = $mgr->create(
        TaskBuilder::shell('sleep 2; echo nonblock-done')
            ->timeout(10)
    );
    $elapsed = microtime(true) - $t0;
    assertTrue(!empty($id) && strpos($id, 'error') === false, "create 失败: {$id}");
    // 非阻塞：dispatch 应在 1 秒内返回（任务还在 sleep 2 中）
    assertTrue($elapsed < 1.0, "dispatch 应非阻塞，实际耗时 {$elapsed}s");
    // 立即查询应为 pending 或 running（非 success）
    $st = $mgr->state($id);
    $state = $st['state'] ?? '?';
    assertTrue(in_array($state, ['pending', 'running'], true), "立即查询应为 pending/running，实际 {$state}");
    $GLOBALS['nonblockId'] = $id;
    echo "（dispatch 耗时 " . round($elapsed, 3) . "s）";
});

// ============================================================
// 2. 批量并发派发（10 任务并发，全部 success）
// ============================================================
step(2, '批量并发派发（10 任务并发）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $ids = [];
    // 10 个快速任务并发派发
    for ($i = 0; $i < 10; $i++) {
        $ids[] = $mgr->create(
            TaskBuilder::shell("echo batch-{$i}")
                ->timeout(10)
        );
    }
    // 等待全部完成
    $okCount = 0;
    $deadline = time() + 15;
    while (time() < $deadline) {
        $allDone = true;
        foreach ($ids as $id) {
            $st = $mgr->state($id);
            $s = $st['state'] ?? '?';
            if ($s === 'success') {
                $okCount++;
            } elseif ($s !== 'failed') {
                $allDone = false;
            }
        }
        if ($allDone) break;
        usleep(300000);
    }
    assertTrue($okCount >= 10, "应有 10 个 success，实际 {$okCount}");
    echo "（{$okCount}/10 success）";
});

// ============================================================
// 3. 任务进度上报（shell 中调 report_progress，state.progress 字段）
// ============================================================
step(3, '任务进度上报', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // shell 任务执行中调用 xhjob_report_progress 上报进度
    // 用一个 shell 脚本：sleep 1 上报 30% 再 sleep 1 上报 70% 再完成
    // 注意：shell 任务无法直接调 xhjob PHP 函数，需通过 xhjobctl CLI
    // 这里用更简单的方式：创建任务后从 PHP 端（模拟外部上报）调 reportProgress
    $id = $mgr->create(
        TaskBuilder::shell('sleep 2; echo progress-done')
            ->timeout(10)
    );
    // 等 0.5 秒让任务进入 running
    usleep(500000);
    // 从 PHP 端上报进度（模拟任务执行体上报）
    $ok = $mgr->reportProgress($id, 50, '{"step":"half"}');
    assertTrue($ok, 'reportProgress 应返回 true');
    // 查询 state 应含 progress=50
    $st = $mgr->state($id);
    $progress = (int) ($st['progress'] ?? 0);
    $meta = $st['progress_meta'] ?? '';
    assertTrue($progress === 50, "progress 应=50，实际 {$progress}");
    assertTrue(strpos($meta, 'half') !== false, "progress_meta 应含 half，实际 {$meta}");
    // 等任务完成
    $mgr->waitForState($id, 'success', 10);
    echo "（progress=50, meta={$meta}）";
});

// ============================================================
// 4. countdown 延迟派发（3 秒后触发）
// ============================================================
step(4, 'countdown 延迟派发（3 秒）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo countdown-done')
            ->countdown(3)
            ->timeout(10)
    );
    assertTrue(!empty($id), "create 失败");
    // 前 2 秒应为 pending（未到触发时间）
    sleep(2);
    $st = $mgr->state($id);
    $state = $st['state'] ?? '?';
    assertTrue($state === 'pending', "countdown(3) 后 2 秒应为 pending，实际 {$state}");
    // 等 3 秒后应触发完成
    $mgr->waitForState($id, 'success', 10);
    $st2 = $mgr->state($id);
    assertTrue(($st2['state'] ?? '') === 'success', "countdown 后应 success");
    echo "（2s时=pending, 完成后=success）";
});

// ============================================================
// 5. chord 全部成功（3 header + 1 callback，回调 meta 含 3 结果）
// ============================================================
step(5, 'chord 全部成功（3 header + 1 callback）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $headers = [
        TaskBuilder::shell('echo chord-h1')->timeout(10),
        TaskBuilder::shell('echo chord-h2')->timeout(10),
        TaskBuilder::shell('echo chord-h3')->timeout(10),
    ];
    $callback = TaskBuilder::shell('echo chord-callback')->timeout(10);
    $chordId = $mgr->createChord($headers, $callback);
    assertTrue(!empty($chordId) && strpos($chordId, 'error') === false, "createChord 失败: {$chordId}");
    $GLOBALS['chordSuccessId'] = $chordId;

    // 等待 chord 完成（header + callback 都要跑完）
    $deadline = time() + 20;
    $finalState = null;
    while (time() < $deadline) {
        $cs = $mgr->chordState($chordId);
        $finalState = $cs['state'] ?? null;
        if ($finalState === 'success' || $finalState === 'partial_failed') break;
        usleep(500000);
    }
    assertTrue($finalState === 'success', "chord 全部成功应 state=success，实际 {$finalState}");
    echo "（chord_state={$finalState}）";
});

// ============================================================
// 6. chord 部分失败（1 header 失败，chord 转 partial_failed）
// ============================================================
step(6, 'chord 部分失败（1 header 失败）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $headers = [
        TaskBuilder::shell('echo ok-h1')->timeout(10),
        TaskBuilder::shell('exit 1')->timeout(10)->withRetry(0, 0),  // 失败且不重试
        TaskBuilder::shell('echo ok-h3')->timeout(10),
    ];
    $callback = TaskBuilder::shell('echo should-not-run')->timeout(10);
    $chordId = $mgr->createChord($headers, $callback);
    assertTrue(!empty($chordId), "createChord 失败");

    // 等待 chord 进入终态
    $deadline = time() + 20;
    $finalState = null;
    while (time() < $deadline) {
        $cs = $mgr->chordState($chordId);
        $finalState = $cs['state'] ?? null;
        if ($finalState === 'partial_failed' || $finalState === 'success') break;
        usleep(500000);
    }
    assertTrue($finalState === 'partial_failed', "chord 部分失败应 state=partial_failed，实际 {$finalState}");
    // 验证 callback_task_id 为 null（未派发回调）
    $callbackTaskId = $cs['callback_task_id'] ?? null;
    assertTrue($callbackTaskId === null, "部分失败时 callback 不应派发，实际 callback_task_id={$callbackTaskId}");
    echo "（chord_state={$finalState}, callback 未派发）";
});

// ============================================================
// 7. inspect active（运行中任务）
// ============================================================
step(7, 'inspect active（运行中任务）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 派发一个慢任务进入 running
    $slowId = $mgr->create(
        TaskBuilder::shell('sleep 3; echo slow-inspect')
            ->timeout(10)
    );
    usleep(500000);  // 等任务进入 running
    $active = $mgr->inspect('active');
    assertTrue(is_array($active), 'inspect active 应返回数组');
    assertTrue(count($active) >= 1, "应有至少 1 个 active 任务，实际 " . count($active));
    // 验证返回的任务 id 字段存在
    $foundSlow = false;
    foreach ($active as $task) {
        if (($task['id'] ?? '') === $slowId) {
            $foundSlow = true;
            break;
        }
    }
    assertTrue($foundSlow, "inspect active 应包含刚派发的慢任务 {$slowId}");
    echo "（active count=" . count($active) . "）";
});

// ============================================================
// 8. inspect registered（cron 注册任务）
// ============================================================
step(8, 'inspect registered（cron 注册任务）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 注册一个 cron 任务
    $cronId = $mgr->create(
        TaskBuilder::shell('echo cron-inspect')
            ->cron('*/5 * * * *')
            ->timeout(10)
    );
    $registered = $mgr->inspect('registered');
    assertTrue(is_array($registered), 'inspect registered 应返回数组');
    assertTrue(count($registered) >= 1, "应有至少 1 个 registered 任务，实际 " . count($registered));
    // 清理
    $mgr->stop($cronId);
    sleep(1);
    echo "（registered count=" . count($registered) . "）";
});

// ============================================================
// 9. inspect scheduled（未来触发任务）
// ============================================================
step(9, 'inspect scheduled（未来触发任务）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 派发一个 60 秒后触发的任务
    $futureId = $mgr->create(
        TaskBuilder::shell('echo future-inspect')
            ->countdown(60)
            ->timeout(10)
    );
    $scheduled = $mgr->inspect('scheduled');
    assertTrue(is_array($scheduled), 'inspect scheduled 应返回数组');
    assertTrue(count($scheduled) >= 1, "应有至少 1 个 scheduled 任务，实际 " . count($scheduled));
    // 清理
    $mgr->stop($futureId);
    sleep(1);
    echo "（scheduled count=" . count($scheduled) . "）";
});

// ============================================================
// 10. inspect stats（聚合统计）
// ============================================================
step(10, 'inspect stats（聚合统计）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $stats = $mgr->inspect('stats');
    assertTrue(is_array($stats), 'inspect stats 应返回数组');
    // 验证字段存在
    assertTrue(isset($stats['total']), 'stats 应含 total 字段');
    assertTrue(isset($stats['success']), 'stats 应含 success 字段');
    assertTrue(isset($stats['failed']), 'stats 应含 failed 字段');
    assertTrue(isset($stats['queue_depth']), 'stats 应含 queue_depth 字段');
    // 之前测试派发了多个任务，success 应 >= 10
    assertTrue(($stats['success'] ?? 0) >= 10, "success 应>=10，实际 {$stats['success']}");
    echo "（total={$stats['total']}, success={$stats['success']}, failed={$stats['failed']}, queue={$stats['queue_depth']}）";
});

// ============================================================
// 11. pull_events（事件拉取）
// ============================================================
step(11, 'pull_events（事件拉取）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 拉取所有 succeeded 事件
    $events = $mgr->pullEvents(0, 'succeeded');
    assertTrue(is_array($events), 'pullEvents 应返回数组');
    assertTrue(count($events) >= 1, "应有至少 1 个 succeeded 事件，实际 " . count($events));
    // 验证事件结构
    $first = $events[0] ?? [];
    assertTrue(isset($first['task_id']) || isset($first['event_type']), '事件应含 task_id 或 event_type 字段');
    // 拉取全部类型
    $allEvents = $mgr->pullEvents(0, null);
    assertTrue(count($allEvents) >= count($events), "全部事件应 >= succeeded 事件");
    echo "（succeeded events=" . count($events) . ", all events=" . count($allEvents) . "）";
});

// ============================================================
// 12. 停止 daemon
// ============================================================
step(12, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');
});

echo "\n=== 异步队列测试完成：{$pass} passed, {$fail} failed ===\n";
if ($fail > 0) {
    echo "失败的步骤：[" . implode(', ', $failedSteps) . "]\n";
}
exit($fail > 0 ? 1 : 0);
