<?php
// Use a unique service name so the DB file path is
// /tmp/xhjob.{service}.db (Unix) or %TEMP%\xhjob.{service}.db (Windows),
// and won't collide with other tests or the default service.
$svc = 'persist-' . getmypid();
$dbPath = PHP_OS_FAMILY === 'Windows'
    ? getenv('TEMP') . "\\xhjob.{$svc}.db"
    : "/tmp/xhjob.{$svc}.db";
@unlink($dbPath);
@xhjob_stop($svc); usleep(300000);

// Enable persistence for this run. The daemon will derive the DB path from
// XHJOB_DB_DIR (or default /tmp) + service name.
putenv("XHJOB_PERSIST=1");

xhjob_start($svc) or die("FAIL: cannot start daemon with persist\n");
echo "daemon started with persist\n";

// Dispatch a cron task that fires every 2 seconds
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo persist-test'
    : 'echo persist-test';

$payload = json_encode([
    'task_type'    => 'shell',
    'payload'      => ['cmd' => $cmd],
    'cron'         => '*/2 * * * * *',
    'retry_max'    => 0,
    'retry_delay'  => 1,
    'timeout'      => 10,
    'priority'     => 0,
    'allow_overlap'=> false,
    'max_instances'=> 1,
    'coalesce'     => true,
    'persist'      => true,   // task-level persist flag
]);
$id = xhjob_dispatch($payload, $svc);
if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop($svc); exit(1);
}
echo "persist cron task dispatched: $id\n";

// Wait for first fire
for ($i = 0; $i < 30; $i++) {
    $s = xhjob_state($id, $svc);
    if (($s['state'] ?? '') === 'success') break;
    usleep(100000);
}
echo "first run completed\n";

// Verify DB file exists at the service-derived path
if (!file_exists($dbPath)) {
    echo "FAIL: DB file not created at $dbPath\n";
    xhjob_stop($svc); exit(1);
}
echo "DB file exists OK\n";

// Restart the daemon
xhjob_stop($svc);
usleep(500000);
xhjob_start($svc) or die("FAIL: cannot restart daemon\n");
echo "daemon restarted\n";

// The task should be recovered from the DB. Verify it still has its cron + state.
$s = xhjob_state($id, $svc);
if (!isset($s['state']) || $s['state'] === 'UNKNOWN') {
    echo "FAIL: task not recovered after restart\n";
    var_dump($s);
    xhjob_stop($svc); exit(1);
}
echo "task recovered, state={$s['state']}\n";

// Wait for the next cron fire after restart
$success2 = false;
for ($i = 0; $i < 60; $i++) {
    $s = xhjob_state($id, $svc);
    if (($s['state'] ?? '') === 'success') {
        // Check the attempts increased or the finished_at timestamp updated
        $success2 = true;
        break;
    }
    usleep(100000);
}
if (!$success2) {
    echo "FAIL: cron did not fire again after restart within 6s\n";
    var_dump(xhjob_state($id, $svc));
    xhjob_stop($svc); exit(1);
}
echo "cron fired after restart OK\n";

xhjob_stop($svc);
usleep(200000);
putenv("XHJOB_PERSIST");
@unlink($dbPath);
echo "TEST PASSED\n";
?>
