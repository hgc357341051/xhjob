<?php
/**
 * Test: misfire_grace_time (A13)
 *
 * Verifies that the per-job misfire_grace_time field is accepted by dispatch,
 * stored in the task definition (visible via xhjob_get), and that a cron task
 * with a short grace window still triggers normally. Also verifies the default
 * misfire_grace_time=0 (use global 60s) round-trips correctly.
 *
 * Reference: APScheduler misfire_grace_time.
 */

$dataDir = '/tmp/xhjob-mgt-test';
@system("rm -rf $dataDir");
@mkdir($dataDir, 0777, true);

$pass = 0; $fail = 0; $skip = 0;
function ok($cond, $msg) {
    global $pass, $fail;
    if ($cond) { echo "PASS: $msg\n"; $pass++; }
    else { echo "FAIL: $msg\n"; $fail++; }
}
function skip($msg) {
    global $skip; echo "SKIP: $msg\n"; $skip++;
}

echo "=== misfire_grace_time_test.php ===\n\n";

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

// Test 1: cron task with misfire_grace_time=5 (short window)
echo "Test 1: cron task with misfire_grace_time=5\n";
$id1 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo mgt-short'],
    'cron' => '*/1 * * * *',
    'misfire_grace_time' => 5,
], $dataDir);
ok(is_string($id1) && !str_starts_with($id1, "error:"), "dispatched cron task with misfire_grace_time=5 (id=$id1)");

// Immediately query state → PENDING or RUNNING
$state1 = xhjob_state($id1, "default", $dataDir);
ok(in_array(($state1['state'] ?? ''), ['PENDING', 'RUNNING']),
    "state is PENDING or RUNNING immediately (state=" . ($state1['state'] ?? '') . ")");

// Verify misfire_grace_time=5 via xhjob_get
$json1 = xhjob_get($id1, "default", $dataDir);
$task1 = is_string($json1) ? json_decode($json1, true) : null;
ok(is_array($task1) && ($task1['misfire_grace_time'] ?? -1) === 5,
    "xhjob_get shows misfire_grace_time=5 (got=" . var_export($task1['misfire_grace_time'] ?? null, true) . ")");

// Wait for cron trigger (up to 75 seconds)
echo "Waiting up to 75s for cron trigger...\n";
$start = time();
$triggered = false;
while (time() - $start < 75) {
    $s = xhjob_state($id1, "default", $dataDir);
    $ec = (int)($s['execution_count'] ?? 0);
    $st = $s['state'] ?? '';
    if ($ec >= 1 || $st === 'RUNNING') { $triggered = true; break; }
    sleep(1);
}
ok($triggered, "cron task triggered at least once (execution_count>=1 or RUNNING)");

// Verify events show at least one "started" event
if ($triggered) {
    $eventsJson = xhjob_events(0, $id1, "default", $dataDir);
    $events = json_decode($eventsJson, true);
    if (!is_array($events)) $events = [];
    $hasStarted = false;
    foreach ($events as $e) {
        if (($e['event_type'] ?? '') === 'started') { $hasStarted = true; break; }
    }
    ok($hasStarted, "events contain at least one 'started' event");
} else {
    skip("events check skipped (cron did not trigger within 75s)");
}

// Test 2: default misfire_grace_time=0 (uses global 60s)
echo "Test 2: default misfire_grace_time=0 (global 60s)\n";
$id2 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo mgt-default'],
    'cron' => '*/1 * * * *',
    // misfire_grace_time not set → default 0
], $dataDir);
ok(is_string($id2) && !str_starts_with($id2, "error:"), "dispatched cron task with default misfire_grace_time (id=$id2)");

$json2 = xhjob_get($id2, "default", $dataDir);
$task2 = is_string($json2) ? json_decode($json2, true) : null;
ok(is_array($task2) && ($task2['misfire_grace_time'] ?? -1) === 0,
    "xhjob_get shows misfire_grace_time=0 (default, got=" . var_export($task2['misfire_grace_time'] ?? null, true) . ")");

// Cleanup
@xhjob_remove($id1, "default", $dataDir);
@xhjob_remove($id2, "default", $dataDir);
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
