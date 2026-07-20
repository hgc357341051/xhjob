<?php
/**
 * Example: Cron 自定义时区。
 *
 * 演示如何通过 withTimezone() 让 cron 表达式按指定 IANA 时区求值，
 * 而非系统本地时区。
 *
 * 用法：
 *   php -d extension=target/release/libxhjob.so examples/timezone.php
 */

if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// 1. 每天 09:00 北京时间触发
$id1 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?tz=Asia/Shanghai')
    ->cron('0 9 * * *')
    ->withTimezone('Asia/Shanghai')
    ->persist(false)
    ->timeout(30)
    ->dispatch();
echo "Cron task with Asia/Shanghai dispatched: {$id1}\n";

// 2. 每天 09:00 纽约时间触发
$id2 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?tz=America/New_York')
    ->cron('0 9 * * *')
    ->withTimezone('America/New_York')
    ->persist(false)
    ->timeout(30)
    ->dispatch();
echo "Cron task with America/New_York dispatched: {$id2}\n";

// 3. 每天 00:00 UTC 触发
$id3 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?tz=UTC')
    ->cron('0 0 * * *')
    ->withTimezone('UTC')
    ->persist(false)
    ->timeout(30)
    ->dispatch();
echo "Cron task with UTC dispatched: {$id3}\n";

// 非法时区示例：dispatch 应当返回 "error: invalid timezone: ..."
$idBad = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get')
    ->cron('* * * * *')
    ->withTimezone('Not/A_Real_Tz')
    ->dispatch();
echo "Invalid timezone dispatch returned: {$idBad}\n";

// 查看任务状态（cron 任务通常处于 PENDING，等待下一次触发）
foreach ([$id1, $id2, $id3] as $id) {
    $s = xhjob_state($id);
    echo "Task {$id}: state=" . ($s['state'] ?? 'UNKNOWN') . "\n";
}

xhjob_stop();
echo "Done.\n";
