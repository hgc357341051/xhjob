<?php
/**
 * Example: tags 作业分组（A15）.
 *
 * 演示 tag(string $tag) 链式调用为任务附加标签，
 * 并通过 xhjob_list + 客户端 array_filter 按 tag 过滤。
 *
 * Reference: APScheduler tags (A15)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_tags.php
 */

$data_dir = '/tmp/xhjob-ex-tags';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== tags 作业分组 demo (A15) ===\n\n";

// 监控类任务（多 tag）
$monitor1 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?monitor=api')
    ->cron('*/30 * * * * *')
    ->tag('monitor')->tag('critical')->tag('prod')
    ->dispatch();
echo "Dispatched monitor task 1: {$monitor1}\n";

$monitor2 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?monitor=db')
    ->cron('*/30 * * * * *')
    ->tag('monitor')->tag('warning')->tag('prod')
    ->dispatch();
echo "Dispatched monitor task 2: {$monitor2}\n";

// 备份类任务
$backup = Xhjob::task()
    ->viaShell('echo backup-started')
    ->cron('0 2 * * *')
    ->tag('backup')->tag('nightly')
    ->dispatch();
echo "Dispatched backup task: {$backup}\n";

// 报表类任务
$report = Xhjob::task()
    ->viaShell('echo report-generated')
    ->cron('0 6 * * *')
    ->tag('report')->tag('daily')
    ->dispatch();
echo "Dispatched report task: {$report}\n";

// 列出全部任务
echo "\nAll tasks:\n";
$all = json_decode(xhjob_list(), true);
foreach ($all as $t) {
    $tags_str = implode(',', $t['tags'] ?? []);
    echo "  - id={$t['id']} state={$t['state']} tags=[{$tags_str}]\n";
}

// 按 tag 'monitor' 过滤（在客户端）
echo "\nTasks tagged 'monitor':\n";
$monitors = array_filter($all, fn($t) => in_array('monitor', $t['tags'] ?? []));
foreach ($monitors as $t) {
    echo "  - id={$t['id']} tags=[" . implode(',', $t['tags']) . "]\n";
}
echo "Count: " . count($monitors) . "\n";

// 按 tag 'prod' 过滤
echo "\nTasks tagged 'prod':\n";
$prods = array_filter($all, fn($t) => in_array('prod', $t['tags'] ?? []));
foreach ($prods as $t) {
    echo "  - id={$t['id']} tags=[" . implode(',', $t['tags']) . "]\n";
}
echo "Count: " . count($prods) . "\n";

// 通过 xhjob_get 也可查询 tags 字段
$task_full = json_decode(xhjob_get($monitor1), true);
echo "\nFull task via xhjob_get: id={$task_full['id']} tags=[" . implode(',', $task_full['tags']) . "]\n";

// 清理
xhjob_cancel($monitor1);
xhjob_cancel($monitor2);
xhjob_cancel($backup);
xhjob_cancel($report);
xhjob_stop('default', $data_dir);
echo "Done.\n";
