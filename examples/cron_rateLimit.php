<?php
/**
 * Example: rateLimit 每任务限流（C12）.
 *
 * 演示 rateLimit(int $count, int $windowSecs) 为单个任务设置滑动窗口限流，
 * 适用于调用外部 API 时控制速率（如免费 API 10s 内最多 3 次）。
 *
 * Reference: Celery rate_limit (C12)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_rateLimit.php
 */

$data_dir = '/tmp/xhjob-ex-ratelimit';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== rateLimit 每任务限流 demo (C12) ===\n\n";

// 每 1 秒尝试触发，但 10 秒内最多 3 次
$id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?limited=1')
    ->cron('*/1 * * * * *')  // 每秒尝试触发
    ->rateLimit(3, 10)        // 10s 内最多 3 次
    ->maxExecutions(20)
    ->dispatch();

echo "Dispatched rate-limited task: {$id}\n";
echo "cron='*/1 * * * * *' (try every 1s), rateLimit(3, 10)\n";
echo "Expected: ~3 executions per 10s window\n\n";

// 轮询 15 秒，观察 execution_count 增长
echo "Polling execution_count over 15 seconds:\n";
$last_count = 0;
for ($t = 0; $t <= 15; $t++) {
    $state = xhjob_state($id);
    $count = $state['execution_count'] ?? 0;
    $delta = $count - $last_count;
    echo "  t={$t}s: execution_count={$count}";
    if ($delta > 0) echo " (+" . $delta . " in last second)";
    echo "\n";
    $last_count = $count;
    if ($t < 15) sleep(1);
}

// 最终验证：execution_count 应在 3-6 之间（15s 内 ~3-4 个窗口 × 3 次）
$final = xhjob_state($id);
echo "\nFinal execution_count: {$final['execution_count']}\n";
echo "(Expected: ~4-5 (15s / 10s window × 3 max per window))\n";

// 通过 xhjob_get 查看 rate_limit 配置
$cfg = json_decode(xhjob_get($id), true);
echo "rate_limit_count={$cfg['rate_limit_count']} rate_limit_window={$cfg['rate_limit_window']}\n";

// 清理
xhjob_cancel($id);
xhjob_stop('default', $data_dir);
echo "Done.\n";
