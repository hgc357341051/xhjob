#!/usr/bin/env php
<?php
/**
 * Retry Backoff Test (C8)
 *
 * Verifies retryBackoff(true) is reflected in task state and that a task with
 * retries configured does retry on shell failure. The exponential delay
 * sequence itself is verified by the Rust unit test
 * `test_exponential_backoff_delay_sequence` (since timing measurements in PHP
 * would be flaky). This end-to-end test focuses on:
 *   1. retryBackoff(true) round-trips through xhjob_state.
 *   2. A shell task that fails (false) with withRetry(3, 1) + retryBackoff(true)
 *      eventually reaches FAILED terminal after 3 retry attempts.
 *
 * Reference: Celery retry_backoff.
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

echo "=== retry_backoff.php ===\n\n";

xhjob_start("backoff-svc");

// Test 1: retryBackoff(true) field round-trip via xhjob_state.
echo "Test 1: retryBackoff(true) field round-trip\n";
$id = Xhjob::task()
    ->service('backoff-svc')
    ->viaShell('false') // exits 1 -> retryable
    ->withRetry(3, 1)
    ->retryBackoff(true)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

$state = xhjob_state($id, "backoff-svc");
check("retry_backoff == true in state info",
    ($state['retry_backoff'] ?? '') === 'true',
    "retry_backoff=" . var_export($state['retry_backoff'] ?? null, true));

// Test 2: a failing shell task with withRetry(3, 1) + retryBackoff(true)
// eventually reaches FAILED terminal after attempts reach 3.
echo "Test 2: failing shell task retries then FAILED\n";
// Wait up to 30 seconds for the task to fail all 3 attempts.
// With retry_delay=1 + retry_backoff=true the delays are: 1, 2, 4 = 7s + exec
// time, so 30s is ample.
$start = time();
$finalState = '';
$finalAttempts = -1;
while (time() - $start < 30) {
    $s = xhjob_state($id, "backoff-svc");
    $finalState = $s['state'] ?? '';
    $finalAttempts = $s['attempts'] ?? -1;
    if ($finalState === 'failed') break;
    usleep(500_000);
}

check("state == FAILED after retries exhausted",
    $finalState === 'failed',
    "state=" . $finalState);
// withRetry(3, 1) 表示 retry_max=3（3 次重试），总执行次数 = 1 次初始 + 3 次重试 = 4
// 最后一次失败时 attempts 从 3 增至 4 并标记为 FAILED
// 注意：xhjob_state 返回的 attempts 是字符串
check("attempts == 4 (retry_max reached)",
    $finalAttempts === '4',
    "attempts=" . var_export($finalAttempts, true));

// Test 3: retryBackoff(false) keeps fixed delay (default behavior).
echo "Test 3: retryBackoff(false) default round-trip\n";
$id2 = Xhjob::task()
    ->service('backoff-svc')
    ->viaShell('echo ok')
    ->withRetry(2, 5)
    ->dispatch(); // no retryBackoff() call -> default false

check("dispatched second task", is_string($id2) && !str_starts_with($id2, "error:"), "id=$id2");
$state2 = xhjob_state($id2, "backoff-svc");
check("retry_backoff == false by default",
    ($state2['retry_backoff'] ?? '') === 'false',
    "retry_backoff=" . var_export($state2['retry_backoff'] ?? null, true));

// Cleanup
xhjob_remove($id, "backoff-svc");
xhjob_remove($id2, "backoff-svc");
xhjob_stop("backoff-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
