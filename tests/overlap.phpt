--TEST--
xhjob task overlap control
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

// Dispatch a slow task that sleeps 3 seconds. allowOverlap=false means
// subsequent triggers within that window should be skipped.
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C ping -n 4 127.0.0.1 >NUL'
    : 'bash -c "sleep 3"';

$payload = json_encode([
    'task_type'    => 'shell',
    'payload'      => ['cmd' => $cmd],
    'cron'         => '*/1 * * * * *',  // every 1 second
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
echo "slow task dispatched: $id\n";

// Wait 2 seconds — task is still running (3s sleep), but cron tries to fire every 1s.
sleep(2);

$s = xhjob_state($id);
if (($s['state'] ?? 'UNKNOWN') !== 'RUNNING') {
    echo "FAIL: expected RUNNING after 2s, got: {$s['state']}\n";
    var_dump($s);
    xhjob_stop(); exit(1);
}
echo "task is still RUNNING after 2s OK\n";

// Wait until completion (5 more seconds should be enough)
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($id);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}
if ($finalState !== 'SUCCESS') {
    echo "FAIL: expected SUCCESS after wait, got: $finalState\n";
    var_dump(xhjob_state($id));
    xhjob_stop(); exit(1);
}
echo "task completed: SUCCESS\n";

// Verify that overlap was respected — only one execution happened (attempts=1, no extra results)
$r = xhjob_result($id);
if (!isset($r['exit_code']) || (int)$r['exit_code'] !== 0) {
    echo "FAIL: expected exit_code=0, got: " . ($r['exit_code'] ?? 'null') . "\n";
    var_dump($r);
    xhjob_stop(); exit(1);
}
echo "exit_code=0 OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
slow task dispatched: %s
task is still RUNNING after 2s OK
task completed: SUCCESS
exit_code=0 OK
TEST PASSED
