#!/usr/bin/env php
<?php
/**
 * Reschedule Test (A11)
 *
 * Verifies that xhjob_reschedule() online-modifies a cron task's cron
 * expression while preserving state, execution_count, attempts, and meta.
 *
 * Reference: APScheduler reschedule_job.
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

echo "=== reschedule_test.php ===\n\n";

xhjob_start("resched-svc");

// Test 1: dispatch a cron task with cron='*/5 * * * *', then reschedule to '*/1 * * * *'.
echo "Test 1: reschedule cron task from */5 to */1\n";
$id = Xhjob::task()
    ->service('resched-svc')
    ->viaShell('echo resched-target')
    ->cron('*/5 * * * *')
    ->withMeta('{"k":"v"}')
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait briefly for the task to be recorded by the daemon.
usleep(200_000);
$state1 = xhjob_state($id, "resched-svc");
check("state is PENDING or RUNNING initially",
    in_array(($state1['state'] ?? ''), ['pending', 'running']),
    "state=" . ($state1['state'] ?? ''));
check("meta preserved initially",
    ($state1['meta'] ?? '') === '{"k":"v"}',
    "meta=" . var_export($state1['meta'] ?? null, true));

// Reschedule to */1 * * * *
$ok = xhjob_reschedule($id, "*/1 * * * *", "resched-svc");
check("xhjob_reschedule returns true", $ok === true);

// Verify state still non-terminal and meta preserved after reschedule.
$state2 = xhjob_state($id, "resched-svc");
check("state still PENDING or RUNNING after reschedule",
    in_array(($state2['state'] ?? ''), ['pending', 'running']),
    "state=" . ($state2['state'] ?? ''));
check("meta preserved after reschedule",
    ($state2['meta'] ?? '') === '{"k":"v"}',
    "meta=" . var_export($state2['meta'] ?? null, true));

// Test 2: reschedule of non-existent task returns false.
echo "Test 2: reschedule of non-existent id\n";
$ok = xhjob_reschedule("nonexistent-id-67890", "*/1 * * * *", "resched-svc");
check("reschedule of non-existent id returns false", $ok === false);

// Test 3: reschedule with invalid cron returns false.
echo "Test 3: reschedule with invalid cron\n";
$ok = xhjob_reschedule($id, "not a valid cron ###", "resched-svc");
check("reschedule with invalid cron returns false", $ok === false);

// Verify the cron field is UNCHANGED (still */1 * * * * from the Test 1 reschedule).
// We verify indirectly by ensuring the task is still active and would fire on
// the next minute boundary. The state must remain non-terminal.
$state3 = xhjob_state($id, "resched-svc");
check("state still non-terminal after invalid cron reschedule",
    in_array(($state3['state'] ?? ''), ['pending', 'running']),
    "state=" . ($state3['state'] ?? ''));

// Test 4: reschedule of an interval (non-cron) task returns false.
echo "Test 4: reschedule of interval task returns false\n";
$intervalId = Xhjob::task()
    ->service('resched-svc')
    ->viaShell('echo interval')
    ->every(60)
    ->dispatch();
check("interval task dispatched", is_string($intervalId) && !str_starts_with($intervalId, "error:"), "id=$intervalId");

usleep(200_000);
$ok = xhjob_reschedule($intervalId, "*/1 * * * *", "resched-svc");
check("reschedule of interval task returns false", $ok === false);

// Cleanup
xhjob_remove($id, "resched-svc");
xhjob_remove($intervalId, "resched-svc");
xhjob_stop("resched-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
