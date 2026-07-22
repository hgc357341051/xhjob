#!/usr/bin/env php
<?php
/**
 * Events Test (A17)
 *
 * Verifies the xhjob_events() API for querying task lifecycle events.
 *
 * Coverage:
 *   1. dispatch a shell task, wait for completion, then xhjob_events(0, null)
 *      returns at least 2 events (started + succeeded).
 *   2. xhjob_events(0, $task_id) filters by task_id — all returned events
 *      belong to that task_id.
 *   3. xhjob_events(time() + 100, null) with a future since_ts returns an
 *      empty array.
 *   4. xhjob_events(0, "nonexistent-task-id") returns an empty array.
 *   5. Each event JSON object contains an `event_type` field whose value is
 *      one of "started" / "succeeded" (the two emitted for a successful
 *      shell run).
 *
 * Reference: APScheduler EVENT_JOB_*.
 */

$dataDir = '/tmp/xhjob-events-test';
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

echo "=== events_test.php (A17) ===\n\n";

if (!xhjob_start('default', $dataDir)) {
    echo "FAIL: daemon start\n";
    exit(1);
}
usleep(500_000);

// Dispatch a shell task and wait for terminal state.
$shellTask = json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo hello-events'],
    'persist'   => false,
]);
$task_id = xhjob_dispatch($shellTask, 'default', $dataDir);
ok(is_string($task_id) && !str_starts_with($task_id, 'error:'),
    "dispatch shell task succeeds (id=$task_id)");

// Poll for completion (max ~5s).
$finalState = null;
for ($i = 0; $i < 50; $i++) {
    $s = xhjob_state($task_id, 'default', $dataDir);
    $st = $s['state'] ?? 'UNKNOWN';
    if (in_array($st, ['success', 'failed', 'cancelled', 'expired'], true)) {
        $finalState = $st;
        break;
    }
    usleep(100_000);
}
ok($finalState === 'success', "task reached SUCCESS (got: $finalState)");

// Small grace period so the succeeded event is flushed to the store.
usleep(200_000);

// ----- Test 1: xhjob_events(0, null) returns >= 2 events -----
echo "Test 1: xhjob_events(0, null) returns >= 2 events\n";
$allJson = xhjob_events(0, null, 'default', $dataDir);
ok(is_string($allJson) && !str_starts_with($allJson, 'error:'),
    "xhjob_events returns JSON string (got: $allJson)");
$allEvents = (is_string($allJson) && !str_starts_with($allJson, 'error:'))
    ? json_decode($allJson, true) : null;
ok(is_array($allEvents), 'events JSON decodes to array');
ok(is_array($allEvents) && count($allEvents) >= 2,
    'at least 2 events emitted (got: ' . (is_array($allEvents) ? count($allEvents) : 'null') . ')');

// ----- Test 2: xhjob_events(0, $task_id) filters by task_id -----
echo "Test 2: xhjob_events(0, \$task_id) filters by task_id\n";
$filtJson = xhjob_events(0, $task_id, 'default', $dataDir);
$filtEvents = (is_string($filtJson) && !str_starts_with($filtJson, 'error:'))
    ? json_decode($filtJson, true) : null;
ok(is_array($filtEvents), 'filtered events JSON decodes to array');
$allBelong = is_array($filtEvents)
    ? array_reduce($filtEvents, function ($carry, $e) use ($task_id) {
        return $carry && (($e['task_id'] ?? null) === $task_id);
    }, true)
    : false;
ok(is_array($filtEvents) && count($filtEvents) >= 2 && $allBelong,
    'all filtered events belong to task_id (count=' . (is_array($filtEvents) ? count($filtEvents) : 0) . ')');

// ----- Test 3: future since_ts returns empty -----
echo "Test 3: xhjob_events(time()+100, null) returns empty array\n";
$futureJson = xhjob_events(time() + 100, null, 'default', $dataDir);
$futureEvents = (is_string($futureJson) && !str_starts_with($futureJson, 'error:'))
    ? json_decode($futureJson, true) : null;
ok(is_array($futureEvents) && count($futureEvents) === 0,
    'future since_ts returns empty array (got: ' . json_encode($futureEvents) . ')');

// ----- Test 4: nonexistent task_id returns empty -----
echo "Test 4: xhjob_events(0, \"nonexistent-task-id\") returns empty\n";
$noneJson = xhjob_events(0, 'nonexistent-task-id', 'default', $dataDir);
$noneEvents = (is_string($noneJson) && !str_starts_with($noneJson, 'error:'))
    ? json_decode($noneJson, true) : null;
ok(is_array($noneEvents) && count($noneEvents) === 0,
    'nonexistent task_id returns empty array (got: ' . json_encode($noneEvents) . ')');

// ----- Test 5: events contain event_type field -----
echo "Test 5: events contain event_type field (started / succeeded)\n";
$typesOk = true;
$hasStarted = false;
$hasSucceeded = false;
if (is_array($allEvents)) {
    foreach ($allEvents as $e) {
        $et = $e['event_type'] ?? null;
        if (!in_array($et, ['started', 'succeeded', 'failed', 'missed', 'cancelled',
                             'paused', 'resumed', 'expired', 'max_instances_reached',
                             'rate_limited'], true)) {
            $typesOk = false;
        }
        if ($et === 'started') $hasStarted = true;
        if ($et === 'succeeded') $hasSucceeded = true;
    }
}
ok($typesOk, 'every event has a valid event_type field');
ok($hasStarted, 'at least one started event present');
ok($hasSucceeded, 'at least one succeeded event present');

// Cleanup
xhjob_stop('default', $dataDir);
@system('rm -rf ' . escapeshellarg($dataDir));

echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
