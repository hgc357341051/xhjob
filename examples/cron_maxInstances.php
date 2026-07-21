<?php
/**
 * Example: max_instances 并发实例数（A10）.
 *
 * 演示 maxInstances(int $n) 允许同一任务并发执行 N 个实例。
 * 对比：默认 maxInstances=1 + allowOverlap=false 会串行触发。
 *
 * Reference: APScheduler max_instances (A10)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_maxInstances.php
 */

$data_dir = '/tmp/xhjob-ex-maxinst';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== max_instances 并发实例数 demo (A10) ===\n\n";

// 任务 1：maxInstances(2) 允许 2 个并发实例
// 每 1 秒触发一次，每次 sleep 3 秒 → 长任务运行中可继续触发新实例
$id_multi = Xhjob::task()
    ->viaShell('sleep 3 && echo multi-instance-done')
    ->cron('*/1 * * * * *')
    ->maxInstances(2)
    ->maxExecutions(4)
    ->dispatch();
echo "Dispatched multi-instance task: {$id_multi} (maxInstances=2)\n";

// 任务 2：默认 maxInstances=1，串行触发
$id_serial = Xhjob::task()
    ->viaShell('sleep 3 && echo serial-done')
    ->cron('*/1 * * * * *')
    ->maxInstances(1)
    ->maxExecutions(4)
    ->dispatch();
echo "Dispatched serial task:     {$id_serial} (maxInstances=1)\n";

// 等待几秒后观察 attempts / execution_count
echo "\nPolling both tasks for 6 seconds...\n";
for ($i = 0; $i < 6; $i++) {
    $s1 = xhjob_state($id_multi);
    $s2 = xhjob_state($id_serial);
    echo "  t={$i}s: multi state={$s1['state']} count={$s1['execution_count']} attempts={$s1['attempts']}";
    echo " | serial state={$s2['state']} count={$s2['execution_count']} attempts={$s2['attempts']}\n";
    sleep(1);
}

// 取消两个任务，避免阻塞 daemon 关闭
xhjob_cancel($id_multi);
xhjob_cancel($id_serial);

echo "\nBoth tasks cancelled.\n";

// 清理
xhjob_stop('default', $data_dir);
echo "Done.\n";
