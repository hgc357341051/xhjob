<?php
/**
 * Test: replace_existing (A14)
 *
 * Verifies the explicit-id + replace_existing semantics:
 *   1. dispatch with id="my-fixed-id" + replace_existing=false → success
 *   2. dispatch same id + replace_existing=false → "error: ..." (id conflict)
 *   3. dispatch same id + replace_existing=true → success (overwrites)
 *   4. xhjob_get("my-fixed-id") returns the latest task JSON with correct payload
 *
 * Reference: APScheduler id / replace_existing.
 */

$dataDir = '/tmp/xhjob-re-test';
@system("rm -rf $dataDir");
@mkdir($dataDir, 0777, true);

$pass = 0; $fail = 0; $skip = 0;
function ok($cond, $msg) {
    global $pass, $fail;
    if ($cond) { echo "PASS: $msg\n"; $pass++; }
    else { echo "FAIL: $msg\n"; $fail++; }
}
function skip($msg) {
    global $skip; echo "SKIP: $msg\n"; $skip++;
}

echo "=== replace_existing_test.php ===\n\n";

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

$fixedId = 'my-fixed-id';

// Test 1: first dispatch with explicit id + replace_existing=false → success
echo "Test 1: first dispatch with id + replace_existing=false\n";
$r1 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo re-original'],
    'id' => $fixedId,
    'replace_existing' => false,
], $dataDir);
ok(is_string($r1) && !str_starts_with($r1, "error:"), "first dispatch succeeds (r=$r1)");
ok($r1 === $fixedId, "returned id equals fixed id (got=$r1)");

// Test 2: second dispatch with same id + replace_existing=false → error
echo "Test 2: second dispatch same id + replace_existing=false → error\n";
$r2 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo re-conflict'],
    'id' => $fixedId,
    'replace_existing' => false,
], $dataDir);
ok(is_string($r2) && str_starts_with($r2, "error:"), "second dispatch returns error (r=$r2)");

// Test 3: third dispatch with same id + replace_existing=true → success (overwrite)
echo "Test 3: third dispatch same id + replace_existing=true → overwrite\n";
$r3 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo re-replaced'],
    'id' => $fixedId,
    'replace_existing' => true,
], $dataDir);
ok(is_string($r3) && !str_starts_with($r3, "error:"), "dispatch with replace_existing=true succeeds (r=$r3)");
ok($r3 === $fixedId, "returned id equals fixed id (got=$r3)");

// Test 4: xhjob_get returns the latest (replaced) task JSON with correct payload
echo "Test 4: xhjob_get returns replaced task with correct payload\n";
$json = xhjob_get($fixedId, "default", $dataDir);
ok(is_string($json) && !empty($json), "xhjob_get returns JSON string");
$task = is_string($json) ? json_decode($json, true) : null;
ok(is_array($task), "xhjob_get JSON decodes to array");
if (is_array($task)) {
    ok(($task['id'] ?? '') === $fixedId, "task id matches fixed id (got=" . ($task['id'] ?? '') . ")");
    ok(($task['task_type'] ?? '') === 'shell', "task_type=shell (got=" . ($task['task_type'] ?? '') . ")");
    ok(($task['payload']['cmd'] ?? '') === 'echo re-replaced',
        "payload.cmd is the replaced value (got=" . var_export($task['payload']['cmd'] ?? null, true) . ")");
    ok(($task['replace_existing'] ?? null) === true,
        "replace_existing=true in task JSON (got=" . var_export($task['replace_existing'] ?? null, true) . ")");
} else {
    skip("payload verification skipped (xhjob_get did not return valid JSON)");
}

// Cleanup
@xhjob_remove($fixedId, "default", $dataDir);
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
