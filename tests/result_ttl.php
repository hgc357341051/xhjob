#!/usr/bin/env php
<?php
/**
 * resultTtl Test
 *
 * Verifies result auto-cleanup after TTL expires:
 * - Immediately after SUCCESS: result has stdout
 * - After TTL: result is empty but task remains
 * Reference: Celery result_expires
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

echo "=== result_ttl.php ===\n\n";

xhjob_start("result-ttl-svc");

// Test 1: resultTtl(2) - result cleared after 2s
echo "Test 1: resultTtl(2) clears result after 2s\n";
$id = Xhjob::task()
    ->service('result-ttl-svc')
    ->viaShell('echo hello-ttl')
    ->resultTtl(2)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait for SUCCESS
$start = time();
while (time() - $start < 10) {
    $state = xhjob_state($id, "result-ttl-svc");
    if (($state['state'] ?? '') === 'SUCCESS') break;
    usleep(500_000);
}
$state = xhjob_state($id, "result-ttl-svc");
check("state == SUCCESS", ($state['state'] ?? '') === 'SUCCESS', "state=" . ($state['state'] ?? ''));

// Immediately fetch result - should have stdout
$result = xhjob_result($id, "result-ttl-svc");
check("result has stdout immediately after SUCCESS",
    !empty($result['stdout']),
    "stdout=" . var_export($result['stdout'] ?? null, true));

// Wait for TTL to expire + daemon cleanup cycle (60s throttle, but in-memory cleanup may happen on next scan_once)
// Wait 5 seconds to ensure TTL=2s is exceeded and cleanup runs
echo "  waiting 5s for TTL expiry + cleanup...\n";
sleep(5);

// Manually trigger another task to force scan_once (which runs cleanup_expired_results)
$triggerId = Xhjob::task()
    ->service('result-ttl-svc')
    ->viaShell('echo trigger')
    ->dispatch();
usleep(500_000);

// Now check result - should be cleaned up
$result = xhjob_result($id, "result-ttl-svc");
$cleaned = empty($result['stdout']) && empty($result['body']) && empty($result['exit_code']);
check("result cleared after TTL",
    $cleaned,
    "stdout=" . var_export($result['stdout'] ?? null, true) . " exit=" . var_export($result['exit_code'] ?? null, true));

// Verify task is still queryable
$state = xhjob_state($id, "result-ttl-svc");
check("task state still queryable after result cleanup",
    isset($state['state']),
    "state=" . var_export($state, true));

// Cleanup
xhjob_remove($id, "result-ttl-svc");
xhjob_remove($triggerId, "result-ttl-svc");
xhjob_stop("result-ttl-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
