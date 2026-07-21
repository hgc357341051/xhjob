#!/usr/bin/env php
<?php
/**
 * DateTrigger Test (A8)
 *
 * Verifies runAt(ts) one-shot triggering and immediate Success terminal state.
 * Reference: APScheduler DateTrigger.
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

echo "=== run_at_trigger.php ===\n\n";

xhjob_start("runat-svc");

// Test 1: runAt(time()+3) fires once and transitions to SUCCESS terminal.
echo "Test 1: runAt(time()+3) one-shot triggers and reaches SUCCESS\n";
$targetTs = time() + 3;
$id = Xhjob::task()
    ->service('runat-svc')
    ->viaShell('echo runat')
    ->runAt($targetTs)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");
check("run_at set in state info",
    (string)(xhjob_state($id, "runat-svc")['run_at'] ?? '') === (string)$targetTs,
    "run_at=" . var_export(xhjob_state($id, "runat-svc")['run_at'] ?? null, true));

// Wait up to 10 seconds for state to reach SUCCESS.
$start = time();
$finalState = '';
while (time() - $start < 10) {
    $state = xhjob_state($id, "runat-svc");
    $finalState = $state['state'] ?? '';
    if ($finalState === 'SUCCESS') break;
    usleep(500_000);
}

check("state == SUCCESS after runAt fired",
    $finalState === 'SUCCESS',
    "state=$finalState");

// Test 2: After SUCCESS, the task should NOT re-fire (one-shot).
echo "Test 2: no re-fire after SUCCESS (one-shot)\n";
$countAfterSuccess = xhjob_state($id, "runat-svc")['execution_count'] ?? 0;
sleep(3);
$stateAfter = xhjob_state($id, "runat-svc");
check("execution_count unchanged after SUCCESS",
    ($stateAfter['execution_count'] ?? 0) == $countAfterSuccess,
    "before={$countAfterSuccess} after=" . ($stateAfter['execution_count'] ?? 0));

// Cleanup
xhjob_remove($id, "runat-svc");
xhjob_stop("runat-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
