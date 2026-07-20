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

echo "Cron HTTP task dispatched.\n";
echo "Task ID: {$taskId}\n";
echo "The daemon will fire this URL every minute, even after this PHP script exits.\n";
echo "Run `php -d extension=xhjob.so -r 'var_dump(xhjob_state(\"{$taskId}\"));'` to check state.\n";
echo "Run `php -d extension=xhjob.so -r 'xhjob_stop();'` to stop the daemon.\n";
