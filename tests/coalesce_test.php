#!/usr/bin/env php
<?php
/**
 * Coalesce Test (A18)
 *
 * Verifies that the `coalesce` field is properly accepted, stored, and
 * round-tripped through xhjob_dispatch() / xhjob_get() for cron tasks.
 *
 * Note: Full verification of coalesce=true (collapse missed triggers into
 * one fire) vs coalesce=false (skip missed triggers beyond grace window)
 * requires a complex scenario where the daemon is unavailable for > 60s
 * to produce missed triggers. This test simplifies to verifying:
 *   1. coalesce=true (default) field round-trip via xhjob_get.
 *   2. coalesce=false field round-trip via xhjob_get.
 *   3. Both cron tasks are accepted by dispatch (basic trigger sanity).
 *
 * Reference: APScheduler coalesce.
 */

$dataDir = '/tmp/xhjob-coalesce-test';
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

echo "=== coalesce_test.php (A18) ===\n\n";

if (!xhjob_start('default', $dataDir)) {
    echo "FAIL: daemon start\n";
    exit(1);
}
usleep(500_000);

// ----- Test 1: coalesce=true (default) -----
echo "Test 1: cron task with coalesce=true (default)\n";
$taskTrue = json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo coalesce-true'],
    'cron'      => '*/1 * * * *',
    'coalesce'  => true,
    'persist'   => false,
]);
$id1 = xhjob_dispatch($taskTrue, 'default', $dataDir);
ok(is_string($id1) && !str_starts_with($id1, 'error:'),
    "dispatch cron + coalesce=true succeeds (id=$id1)");

$json1 = xhjob_get($id1, 'default', $dataDir);
$task1 = is_string($json1) ? json_decode($json1, true) : null;
ok(is_array($task1) && ($task1['coalesce'] ?? null) === true,
    'coalesce=true persisted (got: ' . var_export($task1['coalesce'] ?? null, true) . ')');

// ----- Test 2: coalesce=false -----
echo "Test 2: cron task with coalesce=false\n";
$taskFalse = json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo coalesce-false'],
    'cron'      => '*/1 * * * *',
    'coalesce'  => false,
    'persist'   => false,
]);
$id2 = xhjob_dispatch($taskFalse, 'default', $dataDir);
ok(is_string($id2) && !str_starts_with($id2, 'error:'),
    "dispatch cron + coalesce=false succeeds (id=$id2)");

$json2 = xhjob_get($id2, 'default', $dataDir);
$task2 = is_string($json2) ? json_decode($json2, true) : null;
ok(is_array($task2) && ($task2['coalesce'] ?? null) === false,
    'coalesce=false persisted (got: ' . var_export($task2['coalesce'] ?? null, true) . ')');

// ----- Test 3: default coalesce value when omitted -----
// TaskBuilder defaults coalesce to true (via default_coalesce_true).
echo "Test 3: cron task without explicit coalesce defaults to true\n";
$taskDefault = json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo coalesce-default'],
    'cron'      => '*/1 * * * *',
    'persist'   => false,
]);
$id3 = xhjob_dispatch($taskDefault, 'default', $dataDir);
ok(is_string($id3) && !str_starts_with($id3, 'error:'),
    "dispatch cron without explicit coalesce succeeds (id=$id3)");

$json3 = xhjob_get($id3, 'default', $dataDir);
$task3 = is_string($json3) ? json_decode($json3, true) : null;
ok(is_array($task3) && ($task3['coalesce'] ?? null) === true,
    'default coalesce=true when omitted (got: ' . var_export($task3['coalesce'] ?? null, true) . ')');

// Note: Full coalesce behavior verification (missed-trigger collapsing vs
// skipping) requires a >60s daemon-unavailable scenario which is too
// expensive for this test suite. The field round-trip above covers the
// builder → store → xhjob_get path; the runtime misfire logic is
// separately covered by Rust unit tests in src/scheduler/cron.rs.
skip('full coalesce misfire behavior (requires >60s daemon outage)');

// Cleanup
xhjob_stop('default', $dataDir);
@system('rm -rf ' . escapeshellarg($dataDir));

echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
