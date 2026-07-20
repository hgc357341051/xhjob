--TEST--
xhjob multiple named service instances run independently
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
?>
--FILE--
<?php
// Ensure clean slate for both services.
@xhjob_stop('cron-svc');  usleep(200000);
@xhjob_stop('queue-svc'); usleep(200000);
@xhjob_stop();            usleep(200000);

// 1. Start two independent service daemons.
$r1 = xhjob_start('cron-svc');
if ($r1 !== true) { echo "FAIL: xhjob_start('cron-svc') returned: "; var_dump($r1); exit(1); }
echo "start cron-svc: true OK\n";

$r2 = xhjob_start('queue-svc');
if ($r2 !== true) { echo "FAIL: xhjob_start('queue-svc') returned: "; var_dump($r2); exit(1); }
echo "start queue-svc: true OK\n";

// 2. Verify both report running=true with distinct PIDs.
$s1 = xhjob_status('cron-svc');
$s2 = xhjob_status('queue-svc');

if ($s1['running'] !== 'true' || !isset($s1['pid'])) {
    echo "FAIL: cron-svc status unexpected: "; var_dump($s1); exit(1);
}
if ($s2['running'] !== 'true' || !isset($s2['pid'])) {
    echo "FAIL: queue-svc status unexpected: "; var_dump($s2); exit(1);
}

$pid1 = (int)$s1['pid'];
$pid2 = (int)$s2['pid'];
if ($pid1 <= 0 || $pid2 <= 0) {
    echo "FAIL: invalid pids: {$pid1}, {$pid2}\n"; exit(1);
}
if ($pid1 === $pid2) {
    echo "FAIL: expected distinct pids, both = {$pid1}\n"; exit(1);
}
echo "cron-svc pid={$pid1}, queue-svc pid={$pid2} (distinct) OK\n";

// 3. The default service should NOT be running (we only started named services).
$sd = xhjob_status();
if ($sd['running'] !== 'false') {
    echo "FAIL: expected default service to be not running, got: "; var_dump($sd); exit(1);
}
echo "default service still stopped OK\n";

// 4. Invalid service names must be rejected.
$bad = xhjob_status('1invalid');
if ($bad['running'] !== 'false' || !isset($bad['error'])) {
    echo "FAIL: expected error for invalid service name, got: "; var_dump($bad); exit(1);
}
echo "invalid service name rejected: {$bad['error']} OK\n";

$rBad = xhjob_start('bad name!');
if ($rBad === true) {
    echo "FAIL: xhjob_start with invalid name should return false\n"; exit(1);
}
echo "xhjob_start with invalid name returned false OK\n";

// 5. Stop one service — the other must keep running.
$rStop = xhjob_stop('cron-svc');
if ($rStop !== true) { echo "FAIL: xhjob_stop('cron-svc') returned: "; var_dump($rStop); exit(1); }
echo "stop cron-svc: true OK\n";

usleep(500000);

// cron-svc should now be stopped.
$s1After = xhjob_status('cron-svc');
if ($s1After['running'] !== 'false') {
    echo "FAIL: cron-svc should be stopped, got: "; var_dump($s1After); exit(1);
}
echo "cron-svc stopped OK\n";

// queue-svc must still be running with the same PID.
$s2After = xhjob_status('queue-svc');
if ($s2After['running'] !== 'true' || (int)$s2After['pid'] !== $pid2) {
    echo "FAIL: queue-svc should still be running with pid={$pid2}, got: "; var_dump($s2After); exit(1);
}
echo "queue-svc still running with pid={$pid2} after cron-svc stopped OK\n";

// 6. Dispatch a shell task to queue-svc via the chainable API + service().
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo multi-svc'
    : 'echo multi-svc';

$id = Xhjob::task()
    ->service('queue-svc')
    ->viaShell($cmd)
    ->withRetry(0, 1)
    ->timeout(10)
    ->dispatch();

if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID from queue-svc dispatch, got: {$id}\n";
    xhjob_stop('queue-svc'); exit(1);
}
echo "dispatched to queue-svc: {$id}\n";

// Poll for completion.
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($id, 'queue-svc');
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}
if ($finalState !== 'SUCCESS') {
    echo "FAIL: expected SUCCESS on queue-svc, got: {$finalState}\n";
    var_dump(xhjob_state($id, 'queue-svc'));
    var_dump(xhjob_result($id, 'queue-svc'));
    xhjob_stop('queue-svc'); exit(1);
}
echo "queue-svc task completed: SUCCESS OK\n";

// Verify the result is retrievable via the named service.
$r = xhjob_result($id, 'queue-svc');
if (!isset($r['stdout']) || strpos($r['stdout'], 'multi-svc') === false) {
    echo "FAIL: expected stdout to contain 'multi-svc'\n";
    var_dump($r);
    xhjob_stop('queue-svc'); exit(1);
}
echo "queue-svc result stdout contains marker OK\n";

// 7. Querying the same task ID via a different (stopped) service must fail cleanly.
$sWrong = xhjob_state($id, 'cron-svc');
if (($sWrong['state'] ?? '') === 'SUCCESS') {
    echo "FAIL: cron-svc should not have this task\n";
    var_dump($sWrong); exit(1);
}
echo "task not visible on cron-svc (isolated stores) OK\n";

// 8. Cleanup both services.
xhjob_stop('queue-svc');
usleep(200000);
@xhjob_stop('cron-svc');  // already stopped; should be idempotent
@xhjob_stop();

echo "TEST PASSED\n";
?>
--EXPECTF--
start cron-svc: true OK
start queue-svc: true OK
cron-svc pid=%d, queue-svc pid=%d (distinct) OK
default service still stopped OK
invalid service name rejected: %s OK
xhjob_start with invalid name returned false OK
stop cron-svc: true OK
cron-svc stopped OK
queue-svc still running with pid=%d after cron-svc stopped OK
dispatched to queue-svc: %s
queue-svc task completed: SUCCESS OK
queue-svc result stdout contains marker OK
task not visible on cron-svc (isolated stores) OK
TEST PASSED
