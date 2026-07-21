#!/usr/bin/env php
<?php
/**
 * Group Test (C16)
 *
 * Verifies the xhjob_group() / xhjob_group_state() API for parallel task
 * batches (Celery `group(t1, t2, t3)` semantics).
 *
 * Coverage:
 *   1. Create a 3-task group (echo g1 / g2 / g3), wait for completion,
 *      verify group_state.state="succeeded" and summary={total:3,
 *      succeeded:3, failed:0, pending:0}.
 *   2. xhjob_events(0, null) contains at least 6 events (3 started +
 *      3 succeeded) for the group's tasks.
 *   3. partial_failed test: create another group with 1 `exit 1` task +
 *      2 success tasks, verify group_state.state="partial_failed" and
 *      summary.succeeded=2, summary.failed=1.
 *
 * Reference: Celery `group`.
 */

$dataDir = '/tmp/xhjob-group-test';
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

echo "=== group_test.php (C16) ===\n\n";

if (!xhjob_start('default', $dataDir)) {
    echo "FAIL: daemon start\n";
    exit(1);
}
usleep(500_000);

// Helper: shell task config.
function shellTask(string $cmd): array {
    return [
        'task_type' => 'shell',
        'payload'   => ['cmd' => $cmd],
        'persist'   => false,
    ];
}

// ----- Test 1: 3-task group, all succeed -----
echo "Test 1: 3-task group (g1/g2/g3) all succeed\n";
$tasks = json_encode([
    shellTask('echo "g1"'),
    shellTask('echo "g2"'),
    shellTask('echo "g3"'),
]);
$group_id = xhjob_group($tasks, 'default', $dataDir);
ok(is_string($group_id) && !str_starts_with($group_id, 'error:'),
    "xhjob_group returns group_id (got: $group_id)");

// Wait up to 5s for the group to complete.
$groupFinal = null;
for ($i = 0; $i < 50; $i++) {
    $json = xhjob_group_state($group_id, 'default', $dataDir);
    if (!is_string($json)) { $groupFinal = null; break; }
    $rec = json_decode($json, true);
    $st = $rec['state'] ?? 'unknown';
    if (in_array($st, ['succeeded', 'partial_failed', 'failed'], true)) {
        $groupFinal = $rec;
        break;
    }
    usleep(100_000);
}
ok(is_array($groupFinal), 'group_state returned a record');
ok(is_array($groupFinal) && ($groupFinal['state'] ?? '') === 'succeeded',
    'group state=succeeded (got: ' . (is_array($groupFinal) ? $groupFinal['state'] ?? '' : 'null') . ')');

$summary = is_array($groupFinal) ? ($groupFinal['summary'] ?? null) : null;
ok(is_array($summary), 'group_state contains summary object');
ok(is_array($summary) && ($summary['total'] ?? -1) === 3,
    'summary.total=3 (got: ' . var_export(is_array($summary) ? $summary['total'] ?? null : null, true) . ')');
ok(is_array($summary) && ($summary['succeeded'] ?? -1) === 3,
    'summary.succeeded=3 (got: ' . var_export(is_array($summary) ? $summary['succeeded'] ?? null : null, true) . ')');
ok(is_array($summary) && ($summary['failed'] ?? -1) === 0,
    'summary.failed=0 (got: ' . var_export(is_array($summary) ? $summary['failed'] ?? null : null, true) . ')');
ok(is_array($summary) && ($summary['pending'] ?? -1) === 0,
    'summary.pending=0 (got: ' . var_export(is_array($summary) ? $summary['pending'] ?? null : null, true) . ')');

// ----- Test 2: events count -----
echo "Test 2: xhjob_events(0, null) contains >= 6 events\n";
usleep(200_000);
$eventsJson = xhjob_events(0, null, 'default', $dataDir);
$events = (is_string($eventsJson) && !str_starts_with($eventsJson, 'error:'))
    ? json_decode($eventsJson, true) : null;
ok(is_array($events), 'events JSON decodes to array');
ok(is_array($events) && count($events) >= 6,
    'at least 6 events emitted (3 started + 3 succeeded) (got: ' . (is_array($events) ? count($events) : 0) . ')');

// ----- Test 3: partial_failed group -----
echo "Test 3: group with 1 failing task + 2 success -> partial_failed\n";
$pfTasks = json_encode([
    shellTask('echo "pf-ok-1"'),
    shellTask('exit 1'),
    shellTask('echo "pf-ok-2"'),
]);
$pf_group_id = xhjob_group($pfTasks, 'default', $dataDir);
ok(is_string($pf_group_id) && !str_starts_with($pf_group_id, 'error:'),
    "xhjob_group returns group_id for partial_failed group (got: $pf_group_id)");

// Wait up to 5s for the partial_failed group to settle.
$pfFinal = null;
for ($i = 0; $i < 50; $i++) {
    $json = xhjob_group_state($pf_group_id, 'default', $dataDir);
    if (!is_string($json)) { $pfFinal = null; break; }
    $rec = json_decode($json, true);
    $st = $rec['state'] ?? 'unknown';
    if (in_array($st, ['succeeded', 'partial_failed', 'failed'], true)) {
        $pfFinal = $rec;
        break;
    }
    usleep(100_000);
}
ok(is_array($pfFinal), 'partial_failed group_state returned a record');
ok(is_array($pfFinal) && ($pfFinal['state'] ?? '') === 'partial_failed',
    'partial_failed group state=partial_failed (got: ' . (is_array($pfFinal) ? $pfFinal['state'] ?? '' : 'null') . ')');

$pfSummary = is_array($pfFinal) ? ($pfFinal['summary'] ?? null) : null;
ok(is_array($pfSummary) && ($pfSummary['succeeded'] ?? -1) === 2,
    'partial_failed summary.succeeded=2 (got: ' . var_export(is_array($pfSummary) ? $pfSummary['succeeded'] ?? null : null, true) . ')');
ok(is_array($pfSummary) && ($pfSummary['failed'] ?? -1) === 1,
    'partial_failed summary.failed=1 (got: ' . var_export(is_array($pfSummary) ? $pfSummary['failed'] ?? null : null, true) . ')');

// Cleanup
xhjob_stop('default', $dataDir);
@system('rm -rf ' . escapeshellarg($dataDir));

echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
