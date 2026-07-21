#!/usr/bin/env php
<?php
/**
 * Boundary Cases Test
 *
 * Verifies error handling for invalid inputs and edge cases:
 * - invalid JSON in dispatch
 * - invalid service name in state/result
 * - daemon not running
 * - invalid cron expression (Task 6 verification)
 * - HTTP 4xx not retried (Task 4 verification, requires network; SKIP if no network)
 */

$pass = 0;
$fail = 0;
$skip = 0;

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

function skip(string $name, string $reason): void {
    global $skip;
    echo "  [SKIP] {$name} - {$reason}\n";
    $skip++;
}

echo "=== boundary_cases.php ===\n\n";

// 1. invalid JSON in dispatch
echo "Test 1: invalid JSON in dispatch\n";
$result = xhjob_dispatch("{not json", "default");
check("returns error string", is_string($result) && str_starts_with($result, "error:"),
    "got: " . var_export($result, true));

// 2. invalid service name in state
echo "Test 2: invalid service name in state\n";
$state = xhjob_state("any-id", "1invalid");
check("returns array", is_array($state), "got: " . gettype($state));
check("state field exists", isset($state['state']), "keys: " . implode(',', array_keys($state ?? [])));
// Should not crash with fatal error; state should be unknown/error indicator
check("state is not success", ($state['state'] ?? '') !== 'SUCCESS', "state={$state['state']}");

// 3. invalid service name in result
echo "Test 3: invalid service name in result\n";
$result = xhjob_result("any-id", "1invalid");
check("returns array", is_array($result), "got: " . gettype($result));

// 4. daemon not running (stop first to ensure clean state)
echo "Test 4: dispatch when daemon not running\n";
// Try stop first (in case daemon running from previous test)
@xhjob_stop("boundary-svc");
$result = xhjob_dispatch(
    json_encode(['task_type' => 'Shell', 'payload' => ['cmd' => 'echo hi'], 'service_name' => 'boundary-svc']),
    "boundary-svc"
);
check("returns error: prefix when daemon not running",
    is_string($result) && str_starts_with($result, "error:"),
    "got: " . var_export($result, true));

// 5. invalid cron expression (Task 6 verification)
echo "Test 5: invalid cron expression in dispatch\n";
// Start daemon first (or use default - if it's already running this is no-op)
xhjob_start("boundary-svc");
$result = Xhjob::task()
    ->service('boundary-svc')
    ->viaShell('echo hi')
    ->cron('not a cron')
    ->dispatch();
check("returns error: prefix for invalid cron",
    is_string($result) && str_starts_with($result, "error:"),
    "got: " . var_export($result, true));
check("error mentions cron parse",
    is_string($result) && str_contains($result, "cron"),
    "got: " . var_export($result, true));
xhjob_stop("boundary-svc");

// 6. HTTP 4xx not retried (Task 4 verification, requires network)
echo "Test 6: HTTP 404 not retried\n";
if (!getenv('XHJOB_NETWORK_TESTS')) {
    skip("HTTP 404 not retried", "set XHJOB_NETWORK_TESTS=1 to enable (requires network)");
} else {
    xhjob_start("boundary-svc");
    $id = Xhjob::task()
        ->service('boundary-svc')
        ->viaHttp('GET', 'https://httpbin.org/status/404')
        ->withRetry(3, 1)
        ->timeout(10)
        ->dispatch();
    
    // Wait for terminal state
    $maxWait = 15;
    $start = time();
    while (time() - $start < $maxWait) {
        $state = xhjob_state($id, "boundary-svc");
        $s = $state['state'] ?? '';
        if (in_array($s, ['SUCCESS', 'FAILED', 'CANCELLED'])) break;
        usleep(500_000);
    }
    $state = xhjob_state($id, "boundary-svc");
    check("HTTP 404 → FAILED", ($state['state'] ?? '') === 'FAILED', "state={$state['state']}");
    check("attempts == 1 (not retried)", ($state['attempts'] ?? 0) == 1, "attempts={$state['attempts']}");
    xhjob_stop("boundary-svc");
}

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
echo "SKIP: {$skip}\n";
exit($fail === 0 ? 0 : 1);
