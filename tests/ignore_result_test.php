#!/usr/bin/env php
<?php
/**
 * ignoreResult Test (C9)
 *
 * Verifies that ignoreResult(true) makes the daemon skip save_result, so
 * xhjob_result() returns null for the task. The task state machine still
 * runs (Pending → Running → Success).
 *
 * Also verifies that the default (ignoreResult not called) still saves
 * the result as before (backward compatibility).
 *
 * Reference: Celery ignore_result.
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

echo "=== ignore_result_test.php ===\n\n";

xhjob_start("ignoreresult-svc");

// Test 1: ignoreResult(true) — task state machine runs but xhjob_result is null.
echo "Test 1: ignoreResult(true) skips save_result\n";
$id = Xhjob::task()
    ->service('ignoreresult-svc')
    ->viaShell('echo ignored-result')
    ->ignoreResult(true)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait briefly for the task to complete.
usleep(500_000);
$state = xhjob_state($id, "ignoreresult-svc");
check("state eventually SUCCESS (machine still ran)",
    ($state['state'] ?? '') === 'success',
    "state=" . ($state['state'] ?? ''));
// 注意：xhjob_state 返回的 ignore_result 是字符串 'true'，不是 bool true
check("ignore_result=true visible in state",
    ($state['ignore_result'] ?? null) === 'true',
    "ignore_result=" . var_export($state['ignore_result'] ?? null, true));

// Result should be null (no row saved).
$result = xhjob_result($id, "ignoreresult-svc");
check("xhjob_result returns error (no row saved)",
        isset($result['error']),
        "result=" . json_encode($result));

// Test 2: default (ignoreResult not set) — result is saved as before.
echo "Test 2: default still saves result\n";
$id2 = Xhjob::task()
    ->service('ignoreresult-svc')
    ->viaShell('echo saved-result')
    ->dispatch();

check("dispatched", is_string($id2) && !str_starts_with($id2, "error:"), "id=$id2");

usleep(500_000);
$state2 = xhjob_state($id2, "ignoreresult-svc");
check("state eventually SUCCESS",
    ($state2['state'] ?? '') === 'success',
    "state=" . ($state2['state'] ?? ''));
// 注意：xhjob_state 返回的 ignore_result 是字符串 'false'，不是 bool false
check("ignore_result=false (default) visible in state",
    ($state2['ignore_result'] ?? null) === 'false',
    "ignore_result=" . var_export($state2['ignore_result'] ?? null, true));

// Result should be present.
$result2 = xhjob_result($id2, "ignoreresult-svc");
check("xhjob_result has stdout",
    isset($result2['stdout']) && str_contains($result2['stdout'], 'saved-result'),
    "result=" . json_encode($result2));
// 注意：xhjob_result 返回的 exit_code 是字符串 '0'，不是 int 0
check("xhjob_result exit_code=0",
    ($result2['exit_code'] ?? -1) === '0',
    "exit_code=" . var_export($result2['exit_code'] ?? null, true));

// Cleanup
xhjob_remove($id, "ignoreresult-svc");
xhjob_remove($id2, "ignoreresult-svc");
xhjob_stop("ignoreresult-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
