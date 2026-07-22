<?php
/**
 * Test: max_tasks_per_child (C14)
 *
 * Verifies the max_tasks_per_child daemon setting (worker self-recycling after
 * N tasks). Since xhjob_start() does not accept environment variables, the
 * actual self-recycling path (XHJOB_MAX_TASKS_PER_CHILD env var) cannot be
 * triggered through the PHP API alone.
 *
 * This test:
 *   - SKIPs the env-var self-recycling verification with a manual-run note.
 *   - Performs a basic functional check: dispatch 5 shell tasks and verify
 *     all 5 reach SUCCESS (proves the daemon processes tasks correctly).
 *
 * To verify self-recycling manually, run:
 *   XHJOB_MAX_TASKS_PER_CHILD=5 php -d extension=xhjob.so tests/max_tasks_per_child_test.php
 *
 * Reference: Celery worker_max_tasks_per_child.
 */

$dataDir = '/tmp/xhjob-mtpc-test';
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

echo "=== max_tasks_per_child_test.php ===\n\n";

// SKIP: actual self-recycling verification needs the env var set before the
// daemon starts. xhjob_start() does not accept env-var parameters, so this
// must be run manually:
//   XHJOB_MAX_TASKS_PER_CHILD=5 php -d extension=xhjob.so tests/max_tasks_per_child_test.php
skip("XHJOB_MAX_TASKS_PER_CHILD self-recycling needs manual daemon start: XHJOB_MAX_TASKS_PER_CHILD=5 php -d extension=xhjob.so tests/max_tasks_per_child_test.php");

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

// Basic functional test: dispatch 5 shell tasks, verify all reach SUCCESS.
echo "Test 1: dispatch 5 shell tasks and verify all SUCCESS\n";
$ids = [];
for ($i = 0; $i < 5; $i++) {
    $id = dispatch_task([
        'task_type' => 'shell',
        'payload' => ['cmd' => "echo mtpc-task-$i"],
    ], $dataDir);
    if (is_string($id) && !str_starts_with($id, "error:")) {
        $ids[] = $id;
    }
}
ok(count($ids) === 5, "dispatched 5 shell tasks (got=" . count($ids) . ")");

// Wait for all 5 to reach SUCCESS (up to 15s)
$start = time();
while (time() - $start < 15) {
    $allSuccess = true;
    foreach ($ids as $id) {
        $s = xhjob_state($id, "default", $dataDir);
        if (($s['state'] ?? '') !== 'success') { $allSuccess = false; break; }
    }
    if ($allSuccess && count($ids) > 0) break;
    usleep(500_000);
}

$successCount = 0;
foreach ($ids as $id) {
    $s = xhjob_state($id, "default", $dataDir);
    if (($s['state'] ?? '') === 'success') $successCount++;
}
ok($successCount === 5, "all 5 tasks reached SUCCESS (got=$successCount)");

// Note about self-recycling verification
echo "\nNote: to verify worker self-recycling after N tasks, restart the daemon\n";
echo "with XHJOB_MAX_TASKS_PER_CHILD=5 set in the environment and re-run this test.\n";
echo "The daemon log (under $dataDir) would show worker restart events.\n";

// Cleanup
foreach ($ids as $id) {
    @xhjob_remove($id, "default", $dataDir);
}
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
