--TEST--
xhjob daemon start/stop lifecycle
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
?>
--FILE--
<?php
// Ensure clean slate
@xhjob_stop();
usleep(300000);

// 1. status should report running=false
$s = xhjob_status();
if ($s['running'] !== 'false') {
    echo "FAIL: expected running=false initially, got: ";
    var_dump($s);
    exit(1);
}
echo "initial status: running=false OK\n";

// 2. start daemon
$r = xhjob_start();
if ($r !== true) { echo "FAIL: xhjob_start returned: "; var_dump($r); exit(1); }
echo "start: true OK\n";

// 3. status should now report running=true with a pid
$s = xhjob_status();
if ($s['running'] !== 'true' || !isset($s['pid']) || (int)$s['pid'] <= 0) {
    echo "FAIL: unexpected status after start: ";
    var_dump($s);
    exit(1);
}
$pid = (int)$s['pid'];
echo "status: running=true, pid={$pid} OK\n";

// 4. calling start again should be a no-op (return true, same pid)
$r = xhjob_start();
if ($r !== true) { echo "FAIL: second start returned: "; var_dump($r); exit(1); }
$s = xhjob_status();
if ((int)$s['pid'] !== $pid) {
    echo "FAIL: expected same pid=$pid after duplicate start, got: ";
    var_dump($s);
    exit(1);
}
echo "duplicate start: no-op OK\n";

// 5. stop daemon
$r = xhjob_stop();
if ($r !== true) { echo "FAIL: xhjob_stop returned: "; var_dump($r); exit(1); }
echo "stop: true OK\n";

// 6. after stop, status should report running=false
usleep(500000);
$s = xhjob_status();
if ($s['running'] !== 'false') {
    echo "FAIL: expected running=false after stop, got: ";
    var_dump($s);
    exit(1);
}
echo "post-stop status: running=false OK\n";

// 7. calling stop when no daemon should not panic (returns true: idempotent)
$r = xhjob_stop();
var_dump($r);

echo "TEST PASSED\n";
?>
--EXPECTF--
initial status: running=false OK
start: true OK
status: running=true, pid=%d OK
duplicate start: no-op OK
stop: true OK
post-stop status: running=false OK
bool(true)
TEST PASSED
