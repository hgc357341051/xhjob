<?php
/**
 * Test: acks_on_failure (C13)
 *
 * Verifies the acks_on_failure semantics:
 *   - acks_on_failure=false: a failing task retries indefinitely and never
 *     reaches the FAILED terminal state (stays PENDING/RUNNING). Cancelling
 *     it transitions to CANCELLED.
 *   - acks_on_failure=true (default): a failing task respects retry_max and
 *     reaches the FAILED terminal state after exhausting retries.
 *
 * Reference: Celery acks_on_failure.
 */

$dataDir = '/tmp/xhjob-aof-test';
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

echo "=== acks_on_failure_test.php ===\n\n";

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

// Test 1: acks_on_failure=false → infinite retry, never FAILED terminal
echo "Test 1: acks_on_failure=false → infinite retry (no FAILED terminal)\n";
$id1 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'exit 1'],
    'retry_max' => 1,
    'retry_delay' => 1,
    'acks_on_failure' => false,
], $dataDir);
ok(is_string($id1) && !str_starts_with($id1, "error:"), "dispatched failing task with acks_on_failure=false (id=$id1)");

// Verify acks_on_failure=false via xhjob_get
$json1 = xhjob_get($id1, "default", $dataDir);
$task1 = is_string($json1) ? json_decode($json1, true) : null;
ok(is_array($task1) && ($task1['acks_on_failure'] ?? null) === false,
    "xhjob_get shows acks_on_failure=false (got=" . var_export($task1['acks_on_failure'] ?? null, true) . ")");

// Wait ~10s for retries to cycle, then verify state is NOT FAILED (still retrying)
echo "Waiting 10s for retry cycles...\n";
sleep(10);
$state1 = xhjob_state($id1, "default", $dataDir);
ok(in_array(($state1['state'] ?? ''), ['pending', 'running']),
    "state is PENDING or RUNNING after 10s (infinite retry, not FAILED) (state=" . ($state1['state'] ?? '') . ")");
ok(($state1['state'] ?? '') !== 'failed',
    "state is NOT FAILED (acks_on_failure=false retries indefinitely)");

// Cancel the infinitely-retrying task → CANCELLED
echo "Test 2: cancel the infinitely-retrying task → CANCELLED\n";
$cancelOk = xhjob_cancel($id1, "default", $dataDir);
ok($cancelOk === true, "xhjob_cancel returns true");

// Give the cancel a moment to propagate (task may be RUNNING)
$start = time();
$finalState1 = '';
while (time() - $start < 5) {
    $s = xhjob_state($id1, "default", $dataDir);
    $finalState1 = $s['state'] ?? '';
    if ($finalState1 === 'cancelled') break;
    usleep(500_000);
}
ok($finalState1 === 'cancelled',
    "state is CANCELLED after cancel (state=" . $finalState1 . ")");

// Test 3: acks_on_failure=true (default) → FAILED terminal after retry_max
echo "Test 3: acks_on_failure=true (default) → FAILED terminal after retry_max\n";
$id2 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'exit 1'],
    'retry_max' => 1,
    'retry_delay' => 1,
    'acks_on_failure' => true,
], $dataDir);
ok(is_string($id2) && !str_starts_with($id2, "error:"), "dispatched failing task with acks_on_failure=true (id=$id2)");

// Verify acks_on_failure=true via xhjob_get
$json2 = xhjob_get($id2, "default", $dataDir);
$task2 = is_string($json2) ? json_decode($json2, true) : null;
ok(is_array($task2) && ($task2['acks_on_failure'] ?? null) === true,
    "xhjob_get shows acks_on_failure=true (got=" . var_export($task2['acks_on_failure'] ?? null, true) . ")");

// Wait ~5s for retries to exhaust (retry_max=1, retry_delay=1)
echo "Waiting 5s for retry_max to exhaust...\n";
sleep(5);
$state2 = xhjob_state($id2, "default", $dataDir);
ok(($state2['state'] ?? '') === 'failed',
    "state is FAILED (terminal) after retry_max exhausted with acks_on_failure=true (state=" . ($state2['state'] ?? '') . ")");

// Cleanup
@xhjob_remove($id1, "default", $dataDir);
@xhjob_remove($id2, "default", $dataDir);
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
