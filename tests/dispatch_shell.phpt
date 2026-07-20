--TEST--
xhjob dispatch Shell task and verify stdout/exit_code
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

// Platform-specific shell command
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo hello-from-xhjob'
    : 'echo hello-from-xhjob';

$payload = json_encode([
    'task_type'    => 'shell',
    'payload'      => ['cmd' => $cmd],
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
echo "dispatched: $id\n";

// Poll until terminal state
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
    echo "FAIL: expected SUCCESS, got: $finalState\n";
    var_dump(xhjob_state($id));
    var_dump(xhjob_result($id));
    xhjob_stop(); exit(1);
}
echo "final state: SUCCESS\n";

$r = xhjob_result($id);
if (!isset($r['stdout']) || !isset($r['exit_code'])) {
    echo "FAIL: missing stdout/exit_code in result\n";
    var_dump($r);
    xhjob_stop(); exit(1);
}
if ((int)$r['exit_code'] !== 0) {
    echo "FAIL: expected exit_code=0, got: {$r['exit_code']}\n";
    xhjob_stop(); exit(1);
}
if (strpos($r['stdout'], 'hello-from-xhjob') === false) {
    echo "FAIL: expected stdout to contain 'hello-from-xhjob'\n";
    echo "stdout: {$r['stdout']}\n";
    xhjob_stop(); exit(1);
}
echo "exit_code=0, stdout contains marker OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
dispatched: %s
final state: SUCCESS
exit_code=0, stdout contains marker OK
TEST PASSED
