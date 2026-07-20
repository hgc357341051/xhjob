--TEST--
xhjob retry mechanism on failure
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

// Dispatch a task that always fails (exit code != 0) and configure 3 retries.
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C exit 7'
    : 'bash -c "exit 7"';

$payload = json_encode([
    'task_type'    => 'shell',
    'payload'      => ['cmd' => $cmd],
    'retry_max'    => 3,
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
echo "retry task dispatched: $id\n";

// Poll for terminal state — needs up to (3 retries * 1s base + 2 + 4 + 8 backoff cap) ~ 15s
$finalState = null;
for ($i = 0; $i < 300; $i++) {  // up to 30s
    $s = xhjob_state($id);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}

if ($finalState !== 'FAILED') {
    echo "FAIL: expected FAILED, got: $finalState\n";
    var_dump(xhjob_state($id));
    xhjob_stop(); exit(1);
}
echo "final state: FAILED\n";

$s = xhjob_state($id);
$attempts = (int)($s['attempts'] ?? '0');
// After 3 retries (1 initial + 3 retries = 4 total attempts), the task is marked FAILED.
// attempts counter reflects how many times the task has been attempted.
if ($attempts < 3) {
    echo "FAIL: expected at least 3 attempts, got: $attempts\n";
    var_dump($s);
    xhjob_stop(); exit(1);
}
echo "attempts: {$attempts} OK\n";

if (!isset($s['last_error']) || empty($s['last_error'])) {
    echo "FAIL: expected non-empty last_error\n";
    var_dump($s);
    xhjob_stop(); exit(1);
}
echo "last_error populated OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
retry task dispatched: %s
final state: FAILED
attempts: %d OK
last_error populated OK
TEST PASSED
