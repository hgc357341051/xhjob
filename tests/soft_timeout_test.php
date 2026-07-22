#!/usr/bin/env php
<?php
/**
 * softTimeout Test (C11)
 *
 * Verifies the SIGTERM → SIGKILL escalation path for shell tasks:
 *   1. softTimeout(2)+timeout(5) with a shell script that traps SIGTERM and
 *      exits cleanly → state=SUCCESS, soft_timeout="2" visible in state,
 *      stdout contains the trap marker ("CAUGHT").
 *   2. softTimeout(2)+timeout(4) with a shell script that ignores SIGTERM
 *      → state=FAILED after SIGKILL is sent at the grace-period boundary.
 *   3. softTimeout(0) is treated as None (disabled) → soft_timeout=null.
 *   4. xhjob_get() returns the full Task JSON with soft_timeout field.
 *
 * Reference: Celery soft_time_limit.
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

echo "=== soft_timeout_test.php ===\n\n";

xhjob_start("softtimeout-svc");

// Test 1: softTimeout(2)+timeout(5) + trap SIGTERM → state=SUCCESS.
// The bash script traps SIGTERM, prints CAUGHT, and exits 0; otherwise
// it sleeps in a loop so SIGTERM arrives mid-execution.
echo "Test 1: softTimeout(2)+timeout(5) + trap SIGTERM → SUCCESS\n";
$script1 = "trap 'echo CAUGHT; exit 0' TERM; echo STARTED; for i in $(seq 1 30); do sleep 0.5; done";
$id = Xhjob::task()
    ->service('softtimeout-svc')
    ->viaShell($script1)
    ->timeout(5)
    ->softTimeout(2)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait for SIGTERM (2s) + trap firing + a small buffer.
usleep(3_000_000);
$state = xhjob_state($id, "softtimeout-svc");
check("state eventually SUCCESS (graceful SIGTERM exit)",
    ($state['state'] ?? '') === 'success',
    "state=" . ($state['state'] ?? ''));
check("soft_timeout=2 visible in state",
    ($state['soft_timeout'] ?? null) === "2",
    "soft_timeout=" . var_export($state['soft_timeout'] ?? null, true));

// Verify stdout contains CAUGHT (trap actually fired).
$result = xhjob_result($id, "softtimeout-svc");
check("stdout contains CAUGHT (trap fired)",
    isset($result['stdout']) && str_contains($result['stdout'], 'CAUGHT'),
    "stdout=" . var_export($result['stdout'] ?? null, true));
// 注意：xhjob_result 返回的 exit_code 是字符串 '0'，不是 int 0
check("exit_code=0 (clean exit via trap)",
    ($result['exit_code'] ?? -1) === '0',
    "exit_code=" . var_export($result['exit_code'] ?? null, true));

// Test 2: softTimeout(2)+timeout(4) + ignore SIGTERM → state=FAILED.
// The bash script explicitly ignores SIGTERM (trap '' TERM); SIGKILL is
// required to terminate the child at 4s (soft=2s + grace=2s).
echo "Test 2: softTimeout(2)+timeout(4) + ignore SIGTERM → FAILED (SIGKILL)\n";
$script2 = "trap '' TERM; echo STARTED; for i in $(seq 1 30); do sleep 0.5; done";
$id2 = Xhjob::task()
    ->service('softtimeout-svc')
    ->viaShell($script2)
    ->timeout(4)
    ->softTimeout(2)
    ->dispatch();

check("dispatched", is_string($id2) && !str_starts_with($id2, "error:"), "id2=$id2");

// Wait for full timeout (4s) + SIGKILL + buffer.
usleep(5_000_000);
$state2 = xhjob_state($id2, "softtimeout-svc");
check("state eventually FAILED (SIGKILL after grace period)",
    ($state2['state'] ?? '') === 'failed',
    "state=" . ($state2['state'] ?? ''));
check("last_error mentions SIGKILL",
    str_contains((string)($state2['last_error'] ?? ''), 'SIGKILL'),
    "last_error=" . var_export($state2['last_error'] ?? null, true));

// Test 3: softTimeout(0) is treated as None (disabled).
echo "Test 3: softTimeout(0) is treated as None\n";
$id3 = Xhjob::task()
    ->service('softtimeout-svc')
    ->viaShell('echo no-soft-timeout')
    ->timeout(10)
    ->softTimeout(0)
    ->dispatch();

check("dispatched", is_string($id3) && !str_starts_with($id3, "error:"), "id3=$id3");

usleep(500_000);
$state3 = xhjob_state($id3, "softtimeout-svc");
check("state eventually SUCCESS",
    ($state3['state'] ?? '') === 'success',
    "state=" . ($state3['state'] ?? ''));
check("soft_timeout=null (disabled, treated as None)",
    ($state3['soft_timeout'] ?? 'unset') === 'null',
    "soft_timeout=" . var_export($state3['soft_timeout'] ?? null, true));

// Test 4: xhjob_get returns soft_timeout in Task JSON.
echo "Test 4: xhjob_get returns soft_timeout in Task JSON\n";
$json = xhjob_get($id, "softtimeout-svc");
check("xhjob_get returns JSON string", is_string($json) && !empty($json), "json=" . var_export($json, true));
$task = is_string($json) ? json_decode($json, true) : null;
check("Task JSON has soft_timeout=2",
    is_array($task) && ($task['soft_timeout'] ?? null) === 2,
    "soft_timeout=" . var_export($task['soft_timeout'] ?? null, true));

$json3 = xhjob_get($id3, "softtimeout-svc");
$task3 = is_string($json3) ? json_decode($json3, true) : null;
// 注意：当 soft_timeout 为 None 时，Task JSON 中可能省略该字段（serde skip_serializing_if）
// 或值为 null。两种情况都应视为 "disabled"。使用 ?? null 兜底，让缺失字段也判为 null。
check("Task JSON has soft_timeout=null for Test 3",
    is_array($task3) && ($task3['soft_timeout'] ?? null) === null,
    "soft_timeout=" . var_export($task3['soft_timeout'] ?? 'missing', true));

// Cleanup
xhjob_remove($id, "softtimeout-svc");
xhjob_remove($id2, "softtimeout-svc");
xhjob_remove($id3, "softtimeout-svc");
xhjob_stop("softtimeout-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
