#!/usr/bin/env php
<?php
/**
 * IntervalTrigger Test (A7)
 *
 * Verifies every(N) periodic triggering.
 * Reference: APScheduler IntervalTrigger.
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

echo "=== interval_trigger.php ===\n\n";

xhjob_start("interval-svc");

// Test 1: every(2) periodic triggering fires at least 2 times in 6 seconds.
echo "Test 1: every(2) triggers at least 2 times\n";
$id = Xhjob::task()
    ->service('interval-svc')
    ->viaShell('echo interval')
    ->every(2)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait up to 8 seconds for execution_count to reach at least 2.
$start = time();
$lastCount = 0;
while (time() - $start < 8) {
    $state = xhjob_state($id, "interval-svc");
    $lastCount = $state['execution_count'] ?? 0;
    if ($lastCount >= 2) break;
    usleep(500_000);
}

$state = xhjob_state($id, "interval-svc");
check("state not terminal (interval keeps firing)",
    in_array(($state['state'] ?? ''), ['PENDING', 'RUNNING']),
    "state=" . ($state['state'] ?? ''));
check("execution_count >= 2 within 8s",
    ($state['execution_count'] ?? 0) >= 2,
    "count=" . ($state['execution_count'] ?? 0));
check("interval == 2 in state info",
    ($state['interval'] ?? '') === '2',
    "interval=" . var_export($state['interval'] ?? null, true));

// Cleanup
xhjob_remove($id, "interval-svc");
xhjob_stop("interval-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
