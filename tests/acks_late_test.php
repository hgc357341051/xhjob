#!/usr/bin/env php
<?php
/**
 * acksLate Test (C10)
 *
 * Verifies that tasks marked acksLate(true) are visible in task state, and
 * that the daemon's reset_running_to_pending mechanism is wired up correctly
 * (Running + acks_late=true tasks are eligible for crash-recovery reset to
 * Pending on daemon restart).
 *
 * End-to-end coverage of the actual crash-recovery path requires a real
 * daemon kill + restart cycle, which is brittle in CI. Instead this test
 * exercises the user-facing surface:
 *   1. acksLate(true) is accepted by the builder and round-tripped into
 *      xhjob_state() output as acks_late=true.
 *   2. acksLate(false) (default) round-trips as acks_late=false.
 *   3. xhjob_get() returns the full Task JSON including acks_late.
 *
 * Reference: Celery acks_late.
 */

$pass = 0;
$fail = 0;
function check(string $name, bool $ok, string $detail = ''): void {
    global $pass, $fail;
    if ($ok) {
        echo "  [PASS] {$name}\n";
        $pass++;
    } else {
        echo "  [FAIL] {$name}" . ($detail ? " - {$detail}" : "") . "\n";
        $fail++;
    }
}

echo "=== acks_late_test.php ===\n\n";

xhjob_start("ackslate-svc");

// Test 1: acksLate(true) — task is dispatched and acks_late=true visible in state.
echo "Test 1: acksLate(true) round-trips into xhjob_state\n";
$id = Xhjob::task()
    ->service('ackslate-svc')
    ->viaShell('echo ackslate-on')
    ->acksLate(true)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait briefly for the task to complete.
usleep(500_000);
$state = xhjob_state($id, "ackslate-svc");
check("state eventually SUCCESS",
    ($state['state'] ?? '') === 'SUCCESS',
    "state=" . ($state['state'] ?? ''));
check("acks_late=true visible in state",
    ($state['acks_late'] ?? null) === true,
    "acks_late=" . var_export($state['acks_late'] ?? null, true));

// Test 2: default (acksLate not called) — acks_late=false in state.
echo "Test 2: default acks_late=false visible in state\n";
$id2 = Xhjob::task()
    ->service('ackslate-svc')
    ->viaShell('echo ackslate-off')
    ->dispatch();

check("dispatched", is_string($id2) && !str_starts_with($id2, "error:"), "id=$id2");

usleep(500_000);
$state2 = xhjob_state($id2, "ackslate-svc");
check("state eventually SUCCESS",
    ($state2['state'] ?? '') === 'SUCCESS',
    "state=" . ($state2['state'] ?? ''));
check("acks_late=false (default) visible in state",
    ($state2['acks_late'] ?? null) === false,
    "acks_late=" . var_export($state2['acks_late'] ?? null, true));

// Test 3: xhjob_get() returns full Task JSON with acks_late field.
echo "Test 3: xhjob_get returns acks_late in Task JSON\n";
$json = xhjob_get($id, "ackslate-svc");
check("xhjob_get returns JSON string", is_string($json) && !empty($json), "json=" . var_export($json, true));
$task = is_string($json) ? json_decode($json, true) : null;
check("Task JSON has acks_late=true",
    is_array($task) && ($task['acks_late'] ?? null) === true,
    "acks_late=" . var_export($task['acks_late'] ?? null, true));

$json2 = xhjob_get($id2, "ackslate-svc");
$task2 = is_string($json2) ? json_decode($json2, true) : null;
check("Task JSON has acks_late=false (default)",
    is_array($task2) && ($task2['acks_late'] ?? null) === false,
    "acks_late=" . var_export($task2['acks_late'] ?? null, true));

// Cleanup
xhjob_remove($id, "ackslate-svc");
xhjob_remove($id2, "ackslate-svc");
xhjob_stop("ackslate-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
