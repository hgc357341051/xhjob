--TEST--
xhjob cron task triggering
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
?>
--FILE--
<?php
@xhjob_stop(); usleep(300000);
xhjob_start() or die("FAIL: cannot start daemon\n");

// Use a 6-segment cron expr to fire every 2 seconds (second-precision)
// Format:  "sec min hour dom mon dow"
$cronExpr = '*/2 * * * * *';

$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo cron-fired'
    : 'echo cron-fired';

$payload = json_encode([
    'task_type'    => 'shell',
    'payload'      => ['cmd' => $cmd],
    'cron'         => $cronExpr,
    'retry_max'    => 0,
    'retry_delay'  => 1,
    'timeout'      => 10,
    'priority'     => 0,
    'allow_overlap'=> false,
    'max_instances'=> 1,
    'coalesce'     => true,
    'persist'      => false,
]);
$id = xhjob_dispatch($payload);
if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop(); exit(1);
}
echo "cron task dispatched: $id\n";

// Wait for the cron to fire at least once (need up to ~3s for the first fire)
$successSeen = false;
$attempts = 0;
for ($i = 0; $i < 60; $i++) {  // up to 6 seconds
    $s = xhjob_state($id);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS') {
        $successSeen = true;
        break;
    }
    if ($state === 'FAILED') {
        echo "FAIL: task ended in FAILED state\n";
        var_dump(xhjob_state($id));
        var_dump(xhjob_result($id));
        xhjob_stop(); exit(1);
    }
    $attempts++;
    usleep(100000);
}

if (!$successSeen) {
    echo "FAIL: cron never fired within 6 seconds\n";
    var_dump(xhjob_state($id));
    xhjob_stop(); exit(1);
}
echo "cron fired, state=SUCCESS\n";

// Verify the result
$r = xhjob_result($id);
if (!isset($r['stdout']) || strpos($r['stdout'], 'cron-fired') === false) {
    echo "FAIL: stdout missing marker\n";
    var_dump($r);
    xhjob_stop(); exit(1);
}
echo "stdout contains 'cron-fired' OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
cron task dispatched: %s
cron fired, state=SUCCESS
stdout contains 'cron-fired' OK
TEST PASSED
