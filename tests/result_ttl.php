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
    if (($state['state'] ?? '') === 'success') break;
    usleep(500_000);
}
$state = xhjob_state($id, "result-ttl-svc");
check("state == SUCCESS", ($state['state'] ?? '') === 'success', "state=" . ($state['state'] ?? ''));

// Immediately fetch result - should have stdout
$result = xhjob_result($id, "result-ttl-svc");
check("result has stdout immediately after SUCCESS",
    !empty($result['stdout']),
    "stdout=" . var_export($result['stdout'] ?? null, true));

// 等待 TTL 过期 + cleanup 周期。
// 注意：daemon 端 cleanup_expired_results 被 60s 节流（LAST_CLEANUP_TS），
// 首次 scan_once 时会执行一次 cleanup（此时任务尚未完成，无 result 可清），
// 下一次 cleanup 最早要在 60s 后才会再次执行。
// 因此这里轮询最多 70s，等待 result 被清理。
echo "  waiting for TTL expiry + cleanup (throttled to 60s, polling up to 70s)...\n";
$cleaned = false;
$result = xhjob_result($id, "result-ttl-svc");
$waitStart = time();
while (time() - $waitStart < 70) {
    $result = xhjob_result($id, "result-ttl-svc");
    $cleaned = empty($result['stdout']) && empty($result['body']) && empty($result['exit_code']);
    if ($cleaned) break;
    sleep(1);
}

// Now check result - should be cleaned up
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
xhjob_stop("result-ttl-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
