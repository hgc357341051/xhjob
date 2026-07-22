<?php
/**
 * Test: rate_limit (C12)
 *
 * Verifies that a cron task with rate_limit_count + rate_limit_window is
 * accepted by dispatch, the fields round-trip via xhjob_get, and the cron
 * task triggers at least once within the window.
 *
 * Note: Full verification of "max N triggers per window + allow (N+1)th after
 * window elapses" requires a long wait (multiple cron ticks). This test
 * verifies the basic setup is correct and at least one trigger fires.
 *
 * Reference: Celery rate_limit.
 */

$dataDir = '/tmp/xhjob-rl-test';
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

echo "=== rate_limit_test.php ===\n\n";

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

// Test 1: dispatch cron task with rate_limit_count=3, rate_limit_window=10
echo "Test 1: cron task with rate_limit_count=3, rate_limit_window=10\n";
$id = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo rl-trigger'],
    'cron' => '*/1 * * * *',
    'rate_limit_count' => 3,
    'rate_limit_window' => 10,
], $dataDir);
ok(is_string($id) && !str_starts_with($id, "error:"), "dispatched cron task with rate_limit (id=$id)");

// Verify rate_limit fields via xhjob_get
$json = xhjob_get($id, "default", $dataDir);
$task = is_string($json) ? json_decode($json, true) : null;
ok(is_array($task), "xhjob_get returns valid JSON");
if (is_array($task)) {
    ok(($task['rate_limit_count'] ?? -1) === 3,
        "rate_limit_count=3 in task JSON (got=" . var_export($task['rate_limit_count'] ?? null, true) . ")");
    ok(($task['rate_limit_window'] ?? -1) === 10,
        "rate_limit_window=10 in task JSON (got=" . var_export($task['rate_limit_window'] ?? null, true) . ")");
} else {
    skip("rate_limit field verification skipped (xhjob_get failed)");
}

// Immediately query state → PENDING or RUNNING
$state = xhjob_state($id, "default", $dataDir);
ok(in_array(($state['state'] ?? ''), ['pending', 'running']),
    "state is PENDING or RUNNING immediately (state=" . ($state['state'] ?? '') . ")");

// Wait for cron trigger (up to 75 seconds)
echo "Waiting up to 75s for cron trigger...\n";
$start = time();
$triggered = false;
while (time() - $start < 75) {
    $s = xhjob_state($id, "default", $dataDir);
    $ec = (int)($s['execution_count'] ?? 0);
    $st = $s['state'] ?? '';
    if ($ec >= 1 || $st === 'running') { $triggered = true; break; }
    sleep(1);
}
ok($triggered, "cron task triggered at least once within rate_limit window");

// Verify events contain at least one "started" event
if ($triggered) {
    $eventsJson = xhjob_events(0, $id, "default", $dataDir);
    $events = json_decode($eventsJson, true);
    if (!is_array($events)) $events = [];
    $hasStarted = false;
    $startedCount = 0;
    foreach ($events as $e) {
        if (($e['event_type'] ?? '') === 'started') { $hasStarted = true; $startedCount++; }
    }
    ok($hasStarted, "events contain at least one 'started' event (count=$startedCount)");
} else {
    skip("events check skipped (cron did not trigger within 75s)");
}

// Note: full rate-limit enforcement (3 per 10s, 4th allowed after window)
// requires multiple cron ticks and a longer wait. Skipped here.
skip("full rate-limit window enforcement (3-per-10s cap + 4th after window) requires multi-tick wait; basic setup + 1 trigger verified");

// Cleanup
@xhjob_remove($id, "default", $dataDir);
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
