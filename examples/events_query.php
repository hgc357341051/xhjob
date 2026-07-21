<?php
/**
 * Example: Event listener API 任务执行事件流（A17）.
 *
 * 演示 xhjob_events($since_ts, $task_id) 查询任务执行事件流，
 * 包括 started / succeeded / failed / cancelled 等事件类型。
 *
 * Reference: APScheduler add_listener + EVENT_JOB_* (A17)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/events_query.php
 */

$data_dir = '/tmp/xhjob-ex-events';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== Event listener API demo (A17) ===\n\n";

// 记录开始时间，用于查询事件
$start_ts = time();

// dispatch 3 个任务：成功、失败、取消
$id_ok = Xhjob::task()
    ->viaShell('echo event-ok')
    ->dispatch();
echo "Dispatched task 1 (should succeed): {$id_ok}\n";

$id_fail = Xhjob::task()
    ->viaShell('exit 1')  // 故意失败
    ->withRetry(0, 1)
    ->dispatch();
echo "Dispatched task 2 (should fail): {$id_fail}\n";

$id_cancel = Xhjob::task()
    ->viaShell('sleep 10')  // 长任务，用于演示取消
    ->dispatch();
echo "Dispatched task 3 (will cancel): {$id_cancel}\n";

usleep(500_000);
xhjob_cancel($id_cancel);
echo "Cancelled task 3\n";

// 等待任务结束
usleep(1_000_000);

// 查询过去 1 小时内的全部事件
$since = time() - 3600;
echo "\n=== All events since " . date('Y-m-d H:i:s', $since) . " ===\n";
$json = xhjob_events($since);
$events = json_decode($json, true);
if (!is_array($events)) {
    echo "Failed to decode events JSON: {$json}\n";
} else {
    echo "Total events: " . count($events) . "\n\n";
    foreach ($events as $e) {
        $ts = date('H:i:s', $e['ts']);
        $type = $e['event_type'];
        $task = $e['task_id'];
        $payload = $e['payload'] ?? '';
        echo "  [{$ts}] task=" . substr($task, 0, 8) . "... type={$type}";
        if (!empty($payload)) echo " payload={$payload}";
        echo "\n";
    }
}

// 按任务 id 过滤事件
echo "\n=== Events for task 1 ({$id_ok}) ===\n";
$task_events = json_decode(xhjob_events($since, $id_ok), true);
echo "Count: " . count($task_events) . "\n";
foreach ($task_events as $e) {
    echo "  type={$e['event_type']} ts=" . date('H:i:s', $e['ts']) . "\n";
}

// 统计事件类型
echo "\n=== Event type summary ===\n";
$counts = [];
foreach ($events as $e) {
    $type = $e['event_type'];
    $counts[$type] = ($counts[$type] ?? 0) + 1;
}
foreach ($counts as $type => $count) {
    echo "  {$type}: {$count}\n";
}

// 清理
xhjob_stop('default', $data_dir);
echo "\nDone.\n";
