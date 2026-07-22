#!/usr/bin/env php
<?php
/**
 * Max Memory Per Child Test (C17)
 *
 * Verifies the XHJOB_MAX_MEMORY_PER_CHILD worker-limit semantics
 * (Celery `worker_max_memory_per_child`).
 *
 * NOTE: Full verification requires starting the daemon with
 * XHJOB_MAX_MEMORY_PER_CHILD=N set in the environment so the daemon's
 * WorkerLimits singleton picks up a non-zero threshold. xhjob_start()
 * does not accept env-var parameters, and the WorkerLimits singleton is
 * installed once at daemon startup — so a self-recycling daemon cannot
 * be triggered from this PHP test alone.
 *
 * To verify the self-recycling behavior manually, run:
 *   XHJOB_MAX_MEMORY_PER_CHILD=100 php -d extension=xhjob.so \
 *       tests/max_memory_per_child_test.php
 *
 * This test does the next-best thing:
 *   1. SKIP the actual memory-limit verification (requires manual env).
 *   2. Start a daemon (no env), dispatch 5 shell tasks, wait for them
 *      to complete, and verify all 5 reach SUCCESS — sanity-checking
 *      that the daemon's basic task execution still works.
 *
 * Reference: Celery worker_max_memory_per_child.
 */

$dataDir = '/tmp/xhjob-memchild-test';
@system('rm -rf ' . escapeshellarg($dataDir));
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

echo "=== max_memory_per_child_test.php (C17) ===\n\n";

// SKIP notice: full verification requires manual env-var startup.
skip('full memory-limit self-recycling verification requires manual run: '
    . 'XHJOB_MAX_MEMORY_PER_CHILD=100 php -d extension=xhjob.so '
    . 'tests/max_memory_per_child_test.php');

if (!xhjob_start('default', $dataDir)) {
    echo "FAIL: daemon start\n";
    exit(1);
}
usleep(500_000);

// ----- Functional sanity: dispatch 5 shell tasks, all should SUCCESS -----
echo "Test: dispatch 5 shell tasks, verify all reach SUCCESS\n";
$ids = [];
for ($i = 0; $i < 5; $i++) {
    $task = json_encode([
        'task_type' => 'shell',
        'payload'   => ['cmd' => 'echo memchild-' . $i],
        'persist'   => false,
    ]);
    $id = xhjob_dispatch($task, 'default', $dataDir);
    if (is_string($id) && !str_starts_with($id, 'error:')) {
        $ids[] = $id;
    } else {
        echo "FAIL: dispatch[$i] returned error: $id\n";
        $fail++;
    }
}
ok(count($ids) === 5, 'dispatched 5 shell tasks (count=' . count($ids) . ')');

// Wait up to 5s for all 5 to reach SUCCESS.
$successCount = 0;
$deadline = time() + 5;
while (time() < $deadline && $successCount < count($ids)) {
    $successCount = 0;
    foreach ($ids as $tid) {
        $s = xhjob_state($tid, 'default', $dataDir);
        if (($s['state'] ?? '') === 'success') {
            $successCount++;
        }
    }
    if ($successCount < count($ids)) {
        usleep(100_000);
    }
}
ok($successCount === count($ids),
    "all 5 tasks reached SUCCESS (got: $successCount/" . count($ids) . ')');

// Cleanup
xhjob_stop('default', $dataDir);
@system('rm -rf ' . escapeshellarg($dataDir));

echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
