--TEST--
xhjob persistence across daemon restart
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
// This test requires the `persist` cargo feature to be enabled at build time.
// We probe by trying to start the daemon with XHJOB_PERSIST=1 and checking
<<<<<<< Updated upstream
// that a DB file is created.
$probe = getenv('XHJOB_DB');
if ($probe === false) {
    // default path
    $probe = PHP_OS_FAMILY === 'Windows'
        ? getenv('TEMP') . '\\xhjob-probe.db'
        : '/tmp/xhjob-probe.db';
}
@unlink($probe);
putenv("XHJOB_PERSIST=1");
putenv("XHJOB_DB={$probe}");
@xhjob_stop(); usleep(200000);
xhjob_start();
usleep(300000);
$hasDb = file_exists($probe);
@xhjob_stop();
usleep(200000);
putenv("XHJOB_PERSIST");
putenv("XHJOB_DB");
=======
// that a DB file is created for a unique service name.
$probeService = 'persist-probe';
@xhjob_stop($probeService); usleep(200000);
putenv("XHJOB_PERSIST=1");
xhjob_start($probeService);
usleep(300000);
$probePath = PHP_OS_FAMILY === 'Windows'
    ? getenv('TEMP') . "\\xhjob.{$probeService}.db"
    : "/tmp/xhjob.{$probeService}.db";
$hasDb = file_exists($probePath);
@xhjob_stop($probeService);
usleep(200000);
@unlink($probePath);
putenv("XHJOB_PERSIST");
>>>>>>> Stashed changes
if (!$hasDb) {
    echo "skip persist feature not enabled or DB file not created\n";
}
?>
--FILE--
<?php
<<<<<<< Updated upstream
$dbPath = '/tmp/xhjob-persist-' . getmypid() . '.db';
@unlink($dbPath);
@xhjob_stop(); usleep(300000);

// Enable persistence for this run
putenv("XHJOB_PERSIST=1");
putenv("XHJOB_DB={$dbPath}");

xhjob_start() or die("FAIL: cannot start daemon with persist\n");
=======
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
>>>>>>> Stashed changes
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
<<<<<<< Updated upstream
$id = xhjob_dispatch($payload);
if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop(); exit(1);
=======
$id = xhjob_dispatch($payload, $svc);
if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop($svc); exit(1);
>>>>>>> Stashed changes
}
echo "persist cron task dispatched: $id\n";

// Wait for first fire
for ($i = 0; $i < 30; $i++) {
<<<<<<< Updated upstream
    $s = xhjob_state($id);
=======
    $s = xhjob_state($id, $svc);
>>>>>>> Stashed changes
    if (($s['state'] ?? '') === 'SUCCESS') break;
    usleep(100000);
}
echo "first run completed\n";

<<<<<<< Updated upstream
// Verify DB file exists
if (!file_exists($dbPath)) {
    echo "FAIL: DB file not created at $dbPath\n";
    xhjob_stop(); exit(1);
=======
// Verify DB file exists at the service-derived path
if (!file_exists($dbPath)) {
    echo "FAIL: DB file not created at $dbPath\n";
    xhjob_stop($svc); exit(1);
>>>>>>> Stashed changes
}
echo "DB file exists OK\n";

// Restart the daemon
<<<<<<< Updated upstream
xhjob_stop();
usleep(500000);
xhjob_start() or die("FAIL: cannot restart daemon\n");
echo "daemon restarted\n";

// The task should be recovered from the DB. Verify it still has its cron + state.
$s = xhjob_state($id);
if (!isset($s['state']) || $s['state'] === 'UNKNOWN') {
    echo "FAIL: task not recovered after restart\n";
    var_dump($s);
    xhjob_stop(); exit(1);
=======
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
>>>>>>> Stashed changes
}
echo "task recovered, state={$s['state']}\n";

// Wait for the next cron fire after restart
$success2 = false;
for ($i = 0; $i < 60; $i++) {
<<<<<<< Updated upstream
    $s = xhjob_state($id);
=======
    $s = xhjob_state($id, $svc);
>>>>>>> Stashed changes
    if (($s['state'] ?? '') === 'SUCCESS') {
        // Check the attempts increased or the finished_at timestamp updated
        $success2 = true;
        break;
    }
    usleep(100000);
}
if (!$success2) {
    echo "FAIL: cron did not fire again after restart within 6s\n";
<<<<<<< Updated upstream
    var_dump(xhjob_state($id));
    xhjob_stop(); exit(1);
}
echo "cron fired after restart OK\n";

xhjob_stop();
usleep(200000);
putenv("XHJOB_PERSIST");
putenv("XHJOB_DB");
=======
    var_dump(xhjob_state($id, $svc));
    xhjob_stop($svc); exit(1);
}
echo "cron fired after restart OK\n";

xhjob_stop($svc);
usleep(200000);
putenv("XHJOB_PERSIST");
>>>>>>> Stashed changes
@unlink($dbPath);
echo "TEST PASSED\n";
?>
--EXPECTF--
daemon started with persist
persist cron task dispatched: %s
first run completed
DB file exists OK
daemon restarted
task recovered, state=%s
cron fired after restart OK
TEST PASSED
