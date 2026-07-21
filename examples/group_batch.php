<?php
/**
 * Example: Task group 并行批处理（C16）.
 *
 * 演示 xhjob_group 创建 3 任务并行批处理，并查询 group_state 完成率。
 *
 * Reference: Celery group (C16)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/group_batch.php
 */

$data_dir = '/tmp/xhjob-ex-group';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== Task group 并行批处理 demo (C16) ===\n\n";

// 3 任务并行批处理：批量执行独立 shell 命令
$tasks = [
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo batch-A && sleep 1']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo batch-B && sleep 1']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo batch-C && sleep 1']],
];

$group_id = xhjob_group(json_encode($tasks));
echo "Group dispatched: {$group_id}\n";

if (str_starts_with($group_id, 'error:')) {
    fwrite(STDERR, "Failed to create group: {$group_id}\n");
    xhjob_stop('default', $data_dir);
    exit(1);
}

// 轮询组完成率：pending → running → succeeded / partial_failed / failed
echo "\nPolling group state:\n";
$final_state = null;
for ($i = 0; $i < 30; $i++) {
    $json = xhjob_group_state($group_id);
    if ($json === null) {
        echo "  tick {$i}: group_state returned null\n";
        sleep(1);
        continue;
    }
    $state = json_decode($json, true);
    $s = $state['state'];
    $summary = $state['summary'] ?? ['total' => 0, 'succeeded' => 0, 'failed' => 0, 'pending' => 0];
    echo "  tick {$i}: state={$s}";
    echo " total={$summary['total']} ok={$summary['succeeded']} fail={$summary['failed']} pend={$summary['pending']}\n";
    $final_state = $s;
    if (in_array($s, ['succeeded', 'failed', 'partial_failed'])) break;
    sleep(1);
}

echo "\nFinal group state: {$final_state}\n";

// 验证最终完成情况
$final_json = xhjob_group_state($group_id);
$final = json_decode($final_json, true);
$summary = $final['summary'] ?? [];
echo "Final summary: total=" . ($summary['total'] ?? 0);
echo " succeeded=" . ($summary['succeeded'] ?? 0);
echo " failed=" . ($summary['failed'] ?? 0);
echo " pending=" . ($summary['pending'] ?? 0) . "\n";

if (($summary['succeeded'] ?? 0) === 3) {
    echo "\n✓ All 3 batch tasks succeeded in parallel\n";
}

// 清理 daemon
xhjob_stop('default', $data_dir);
echo "\nDone.\n";
