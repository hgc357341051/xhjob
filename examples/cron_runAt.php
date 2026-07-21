<?php
/**
 * Example: DateTrigger 一次性触发（runAt）.
 *
 * 演示 runAt(int $ts) 让任务在指定 Unix 时间戳触发一次后立即进入 SUCCESS 终态。
 * Reference: APScheduler DateTrigger (A8)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_runAt.php
 */

$data_dir = '/tmp/xhjob-ex-runat';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== DateTrigger 一次性触发 demo (A8) ===\n\n";

// 3 秒后触发一次
$fire_at = time() + 3;
$id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?trigger=runAt')
    ->runAt($fire_at)
    ->timeout(15)
    ->dispatch();

echo "Dispatched runAt task: {$id}\n";
echo "Scheduled to fire at Unix ts={$fire_at} (now+" . ($fire_at - time()) . "s)\n";

// 立即查询：应处于 PENDING，next_fire 等于 runAt 设置的时间戳
$initial = xhjob_state($id);
echo "\nInitial state:\n";
echo "  state={$initial['state']}\n";
echo "  run_at={$initial['run_at']}\n";

// 等待触发
echo "\nWaiting 6s for the task to fire...\n";
sleep(6);

// 触发后查询：应处于 SUCCESS 终态，仅触发一次
$after = xhjob_state($id);
echo "\nAfter firing:\n";
echo "  state={$after['state']}\n";
echo "  execution_count={$after['execution_count']}\n";
echo "  attempts={$after['attempts']}\n";

if ($after['state'] === 'SUCCESS' && $after['execution_count'] === '1') {
    echo "\n✓ DateTrigger fired exactly once and transitioned to SUCCESS\n";
} else {
    echo "\n✗ Unexpected state (network may have failed; check last_error)\n";
    if (isset($after['last_error'])) echo "  last_error: {$after['last_error']}\n";
}

// 查询结果
$result = xhjob_result($id);
if (isset($result['status_code'])) {
    echo "  status_code={$result['status_code']}\n";
}

// 清理
xhjob_stop('default', $data_dir);
echo "Done.\n";
