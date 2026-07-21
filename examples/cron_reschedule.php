<?php
/**
 * Example: reschedule 在线修改 cron（A11）.
 *
 * 演示 xhjob_reschedule($id, $cron) 在线修改 cron 任务的 cron 表达式，
 * 保留 state / execution_count / attempts / meta 等运行时状态。
 *
 * Reference: APScheduler reschedule_job (A11)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_reschedule.php
 */

$data_dir = '/tmp/xhjob-ex-resched';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== reschedule 在线修改 cron demo (A11) ===\n\n";

// 初始 cron：每 1 秒触发
$id = Xhjob::task()
    ->viaShell('echo reschedule-demo')
    ->cron('*/1 * * * * *')
    ->persist(true)
    ->dispatch();
echo "Dispatched task with cron='*/1 * * * * *': {$id}\n";

// 用 xhjob_get 查看初始 cron 表达式
$initial = json_decode(xhjob_get($id), true);
echo "Initial cron={$initial['cron']}, execution_count={$initial['execution_count']}\n";

// 等待 3 秒，让 cron 触发几次
echo "\nWaiting 3s for cron to fire...\n";
sleep(3);

$before = json_decode(xhjob_get($id), true);
echo "Before reschedule: cron={$before['cron']}, execution_count={$before['execution_count']}\n";

// 在线修改 cron 为每 5 秒触发
$new_cron = '*/5 * * * * *';
$ok = xhjob_reschedule($id, $new_cron);
echo "\nReschedule to '{$new_cron}': " . ($ok ? "OK" : "FAIL") . "\n";

// 验证：execution_count 保留，cron 已更新
$after = json_decode(xhjob_get($id), true);
echo "After reschedule: cron={$after['cron']}, execution_count={$after['execution_count']}\n";

if ($after['cron'] === $new_cron && $after['execution_count'] === $before['execution_count']) {
    echo "\n✓ cron updated, execution_count preserved\n";
} else {
    echo "\n✗ Verification failed\n";
}

// 测试对非 cron 任务 reschedule 应返回 false
$id_shell = Xhjob::task()
    ->viaShell('echo no-cron')
    ->dispatch();
$bad = xhjob_reschedule($id_shell, '*/2 * * * * *');
echo "\nReschedule a non-cron task: " . ($bad ? "OK (unexpected)" : "FAIL (expected)") . "\n";

// 清理
xhjob_cancel($id);
xhjob_cancel($id_shell);
xhjob_stop('default', $data_dir);
echo "Done.\n";
