#!/usr/bin/env php
<?php
/**
 * maxExecutions Test
 *
 * Verifies cron task auto-stops after N executions.
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

echo "=== max_executions.php ===\n\n";

xhjob_start("max-exec-svc");

// Test 1: maxExecutions(3) - should stop after 3 cron triggers
echo "Test 1: maxExecutions(3) stops after 3 executions\n";
$id = Xhjob::task()
    ->service('max-exec-svc')
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')  // every 1 second
    ->maxExecutions(3)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait for execution_count to reach 3 and state=SUCCESS
$start = time();
$lastCount = 0;
while (time() - $start < 30) {
    $state = xhjob_state($id, "max-exec-svc");
    $lastCount = $state['execution_count'] ?? 0;
    if (($state['state'] ?? '') === 'success') break;
    usleep(500_000);
}

$state = xhjob_state($id, "max-exec-svc");
check("state == SUCCESS after max executions",
    ($state['state'] ?? '') === 'success',
    "state=" . ($state['state'] ?? ''));
check("execution_count == 3",
    ($state['execution_count'] ?? 0) == 3,
    "count=" . ($state['execution_count'] ?? 0));

// Test 2: Verify no more triggers after SUCCESS
echo "Test 2: no more triggers after SUCCESS\n";
$countAfterSuccess = $state['execution_count'] ?? 0;
sleep(3);
$stateAfter = xhjob_state($id, "max-exec-svc");
check("execution_count unchanged after SUCCESS",
    ($stateAfter['execution_count'] ?? 0) == $countAfterSuccess,
    "before={$countAfterSuccess} after=" . ($stateAfter['execution_count'] ?? 0));

// Test 3: maxExecutions(0) = unlimited (verify at least 2 triggers in 3 seconds)
echo "Test 3: maxExecutions(0) = unlimited (default)\n";
$id2 = Xhjob::task()
    ->service('max-exec-svc')
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->maxExecutions(0)  // explicit unlimited
    ->dispatch();

check("dispatched unlimited task", is_string($id2) && !str_starts_with($id2, "error:"), "id=$id2");

$start = time();
while (time() - $start < 5) {
    $state = xhjob_state($id2, "max-exec-svc");
    if (($state['execution_count'] ?? 0) >= 2) break;
    usleep(500_000);
}
$state = xhjob_state($id2, "max-exec-svc");
check("unlimited task triggered at least 2 times in 5s",
    ($state['execution_count'] ?? 0) >= 2,
    "count=" . ($state['execution_count'] ?? 0));

// Cleanup
xhjob_remove($id, "max-exec-svc");
xhjob_remove($id2, "max-exec-svc");
xhjob_stop("max-exec-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
