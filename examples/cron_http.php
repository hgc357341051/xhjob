<?php
/**
 * Example: Cron HTTP task.
 *
 * Dispatches a task that fires every minute and hits a URL.
 *
 * Usage (Linux/macOS):
 *   php -d extension=xhjob.so examples/cron_http.php
 *
 * Usage (Windows):
 *   php -d extension=xhjob.so examples\cron_http.php
 */

// Make sure the daemon is running
if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// Build a chainable cron task that hits httpbin.org every minute.
$taskId = Xhjob::task()
    ->viaHttp('POST', 'https://httpbin.org/post')
    ->withRetry(3, 2)
    ->cron('* * * * *')           // every minute
    ->allowOverlap(false)
    ->persist(false)
    ->timeout(30)
    ->dispatch();

if (str_starts_with($taskId, 'error:')) {
    fwrite(STDERR, "Failed to dispatch cron HTTP task: {$taskId}\n");
    xhjob_stop();
    exit(1);
}

echo "Cron HTTP task dispatched.\n";
echo "Task ID: {$taskId}\n";
echo "The daemon will fire this URL every minute, even after this PHP script exits.\n";
echo "Run `php -d extension=xhjob.so -r 'var_dump(xhjob_state(\"{$taskId}\"));'` to check state.\n";
echo "Run `php -d extension=xhjob.so -r 'xhjob_stop();'` to stop the daemon.\n";

// maxExecutions demo: every 1 second, max 3 executions, then auto-stop.
// Reference: APScheduler max_instances, Celery task max_retries.
// 每 1 秒触发，最多执行 3 次
$id2 = Xhjob::task()
    ->viaShell('echo max-executions-demo')
    ->cron('*/1 * * * * *')
    ->maxExecutions(3)
    ->dispatch();
echo "maxExecutions task id: {$id2} (will execute 3 times then auto-stop)\n";

echo "脚本退出后 daemon 仍持续触发 cron，需运行 `xhjob_stop()` 才能停止\n";
