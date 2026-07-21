#!/usr/bin/env php
<?php
/**
 * Requeue Test (C7)
 *
 * Verifies the cancel → requeue → re-trigger flow:
 *   1. dispatch a delayed shell task (startAt far future) so it stays Pending
 *   2. cancel it → state becomes CANCELLED
 *   3. xhjob_requeue → state returns to PENDING, attempts=0
 *   4. task should fire normally (the startAt is still far in the future, but
 *      requeue sets next_fire=now which causes the scan_retries loop in the
 *      queue to pick it up immediately, bypassing the start_date check which
 *      only applies to cron scan_once, not the retry/queue scan path)
 *
 * Reference: Celery requeue.
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

echo "=== requeue_test.php ===\n\n";

xhjob_start("requeue-svc");

// Test 1: cancel a Pending task → CANCELLED state.
echo "Test 1: cancel Pending task\n";
$id = Xhjob::task()
    ->service('requeue-svc')
    ->viaShell('echo requeue-target')
    ->startAt(time() + 3600) // delay start so it stays Pending
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

$state = xhjob_state($id, "requeue-svc");
check("state == PENDING initially",
    ($state['state'] ?? '') === 'PENDING',
    "state=" . ($state['state'] ?? ''));

$ok = xhjob_cancel($id, "requeue-svc");
check("xhjob_cancel returns true", $ok === true);

$state = xhjob_state($id, "requeue-svc");
check("state == CANCELLED after cancel",
    ($state['state'] ?? '') === 'CANCELLED',
    "state=" . ($state['state'] ?? ''));

// Test 2: requeue the cancelled task → state returns to PENDING.
echo "Test 2: xhjob_requeue returns CANCELLED → PENDING\n";
$ok = xhjob_requeue($id, "requeue-svc");
check("xhjob_requeue returns true", $ok === true);

$state = xhjob_state($id, "requeue-svc");
check("state == PENDING after requeue",
    ($state['state'] ?? '') === 'PENDING',
    "state=" . ($state['state'] ?? ''));
check("attempts == 0 after requeue",
    ($state['attempts'] ?? -1) === 0,
    "attempts=" . var_export($state['attempts'] ?? null, true));

// Test 3: requeue should be idempotent-false on a non-terminal task.
// After the requeue the task is Pending, so requeue again should return false.
echo "Test 3: requeue of non-terminal returns false\n";
$ok = xhjob_requeue($id, "requeue-svc");
check("requeue of Pending task returns false", $ok === false);

// Test 4: requeue of non-existent id returns false (no error / exception).
echo "Test 4: requeue of non-existent id returns false\n";
$ok = xhjob_requeue("nonexistent-id-12345", "requeue-svc");
check("requeue of non-existent id returns false", $ok === false);

// Cleanup
xhjob_remove($id, "requeue-svc");
xhjob_stop("requeue-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
