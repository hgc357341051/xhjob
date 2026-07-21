<?php
/**
 * Example: Jitter 随机抖动.
 *
 * 演示 jitter(int $secs) 在 cron 任务的 next_fire 上叠加随机偏移，
 * 让相同 cron 表达式的多个任务触发时刻分散，避免惊群效应。
 * Reference: APScheduler jitter (A9)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_jitter.php
 */

$data_dir = '/tmp/xhjob-ex-jitter';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== Jitter 随机抖动 demo (A9) ===\n\n";

// 投递 5 个相同 cron（每分钟）的任务，使用 jitter(30)
// next_fire 应分散在接下来的 30 秒内
$ids = [];
for ($i = 0; $i < 5; $i++) {
    $id = Xhjob::task()
        ->viaShell("echo jitter-task-{$i}")
        ->cron('* * * * *')  // 每分钟整点触发
        ->jitter(30)          // next_fire 随机延迟 0~30 秒
        ->dispatch();
    $ids[] = $id;
    echo "Dispatched task #{$i}: {$id}\n";
}

// 查询每个任务的 next_fire（通过 xhjob_get 拿完整 Task JSON）
echo "\nNext fire timestamps (should be spread within 30s after the next minute):\n";
$fires = [];
foreach ($ids as $i => $id) {
    $task = json_decode(xhjob_get($id), true);
    $nf = $task['next_fire'] ?? null;
    $jit = $task['jitter'] ?? 0;
    echo "  task #{$i} ({$id}): next_fire=" . ($nf ?? 'null') . " jitter={$jit}\n";
    if ($nf !== null) $fires[] = (int)$nf;
}

if (count($fires) >= 2) {
    $spread = max($fires) - min($fires);
    echo "\nSpread between earliest and latest next_fire: {$spread}s (jitter=30s)\n";
    if ($spread > 0) {
        echo "✓ Tasks are distributed across time, no thundering herd\n";
    } else {
        echo "(Note: random sampling may produce zero spread; rerun if needed)\n";
    }
}

// 清理
xhjob_stop('default', $data_dir);
echo "Done.\n";
