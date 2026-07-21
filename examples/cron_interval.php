<?php
/**
 * Example: Interval 周期触发（every）.
 *
 * 演示 every(int $secs) 让任务以固定秒数周期触发，无需 cron 表达式。
 * Reference: APScheduler IntervalTrigger (A7)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_interval.php
 */

// 用独立 data_dir 隔离测试，避免与其他示例冲突
$data_dir = '/tmp/xhjob-ex-interval';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== Interval 周期触发 demo (A7) ===\n\n";

// 每 2 秒触发一次 shell 任务
$id = Xhjob::task()
    ->viaShell('echo interval-tick')
    ->every(2)
    ->maxExecutions(3)  // 限制 3 次，便于演示退出
    ->dispatch();

echo "Dispatched interval task: {$id}\n";
echo "Waiting for at least 2 executions (2s * 2 = ~4s)...\n";

// 轮询执行次数，至少等到 execution_count >= 2
$expected_min = 2;
for ($i = 0; $i < 30; $i++) {
    $state = xhjob_state($id);
    $count = $state['execution_count'] ?? 0;
    $st = $state['state'] ?? 'UNKNOWN';
    echo "  tick {$i}: state={$st} execution_count={$count}";
    if (isset($state['interval'])) echo " interval={$state['interval']}";
    echo "\n";
    if ($count >= $expected_min) {
        echo "Reached {$expected_min} executions, OK.\n";
        break;
    }
    if ($st === 'SUCCESS') {
        echo "Task reached maxExecutions(3) and terminated.\n";
        break;
    }
    sleep(1);
}

// 最终状态查询
$final = xhjob_state($id);
echo "\nFinal state: state={$final['state']} execution_count={$final['execution_count']}\n";

// 清理
xhjob_stop('default', $data_dir);
echo "Done.\n";
