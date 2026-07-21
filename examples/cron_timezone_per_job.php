<?php
/**
 * Example: timezone per-job 每作业独立时区（A16）.
 *
 * 演示每个 cron 任务可通过 withTimezone(string $tz) 指定独立的 IANA 时区，
 * next_fire 按该时区计算。区别于全局时区，per-job 时区让同一服务可
 * 调度不同地区的任务。
 *
 * Reference: APScheduler CronTrigger(timezone=...) (A16)
 *
 * 与 examples/timezone.php 的区别：
 *   timezone.php 演示基础 withTimezone 用法；
 *   本示例进一步演示 per-job 时区 + 非法时区错误处理 + 跨国应用场景。
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_timezone_per_job.php
 */

$data_dir = '/tmp/xhjob-ex-tz-perjob';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== timezone per-job demo (A16) ===\n\n";

// 4 个相同 cron 表达式的任务，分别使用不同时区
// 「0 9 * * *」在 4 个时区对应不同 UTC 时刻
$regions = [
    ['name' => 'US East',  'tz' => 'America/New_York'],
    ['name' => 'CN',       'tz' => 'Asia/Shanghai'],
    ['name' => 'UK',       'tz' => 'Europe/London'],
    ['name' => 'UTC',      'tz' => 'UTC'],
];

$ids = [];
foreach ($regions as $r) {
    $id = Xhjob::task()
        ->viaHttp('GET', "https://httpbin.org/get?region=" . urlencode($r['name']))
        ->cron('0 9 * * *')  // 每天 9 点（按 withTimezone 指定时区）
        ->withTimezone($r['tz'])
        ->dispatch();
    $ids[] = $id;
    echo "Dispatched {$r['name']} task (tz={$r['tz']}): {$id}\n";
}

// 查询每个任务的 timezone 配置（通过 xhjob_get）
echo "\nPer-job timezone configuration:\n";
foreach ($ids as $i => $id) {
    $task = json_decode(xhjob_get($id), true);
    $tz = $task['timezone'] ?? 'null';
    $nf = $task['next_fire'] ?? 'null';
    $next_str = $nf !== 'null' ? date('Y-m-d H:i:s', (int)$nf) . ' UTC' : 'null';
    echo "  {$regions[$i]['name']}: tz={$tz} next_fire={$nf} ({$next_str})\n";
}

// 非法时区示例：dispatch 应当返回 "error: invalid timezone: ..."
echo "\nInvalid timezone test:\n";
$bad_id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get')
    ->cron('* * * * *')
    ->withTimezone('Not/A_Real_Tz')
    ->dispatch();
echo "  Result: {$bad_id}\n";
echo "  Started with 'error:': " . (str_starts_with($bad_id, 'error:') ? 'YES (expected)' : 'NO') . "\n";

// 跨国应用场景说明
echo "\n跨国应用场景：\n";
echo "  - 纽约 9:00 报表 → America/New_York\n";
echo "  - 上海 9:00 报表 → Asia/Shanghai\n";
echo "  - 伦敦 9:00 报表 → Europe/London\n";
echo "  - 同一服务调度，无需为每个时区启动独立 daemon\n";

// 清理
foreach ($ids as $id) {
    xhjob_cancel($id);
}
xhjob_stop('default', $data_dir);
echo "\nDone.\n";
