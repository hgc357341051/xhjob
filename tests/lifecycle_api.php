#!/usr/bin/env php
<?php
/**
 * Lifecycle API Test
 *
 * Verifies: pause / resume / cancel / remove / list
 * Reference: APScheduler pause_job / resume_job / remove_job, Celery revoke
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

echo "=== lifecycle_api.php ===\n\n";

xhjob_start("lifecycle-svc");

// Test 1: pause stops cron triggers
echo "Test 1: pause stops cron triggers\n";
$id1 = Xhjob::task()
    ->service('lifecycle-svc')
    ->viaShell('echo task1')
    ->cron('*/1 * * * * *')
    ->dispatch();

// Wait for at least 1 trigger
sleep(2);
$stateBefore = xhjob_state($id1, "lifecycle-svc");
$beforeCount = $stateBefore['execution_count'] ?? 0;
check("task triggered at least once before pause", $beforeCount >= 1, "count=$beforeCount");

$ok = xhjob_pause($id1, "lifecycle-svc");
check("xhjob_pause returns true", $ok === true);

sleep(2);
$statePaused = xhjob_state($id1, "lifecycle-svc");
check("paused == true", ($statePaused['paused'] ?? '') === 'true', "paused=" . var_export($statePaused['paused'] ?? null, true));
check("no new triggers while paused",
    ($statePaused['execution_count'] ?? 0) == $beforeCount,
    "before=$beforeCount after=" . ($statePaused['execution_count'] ?? 0));

// Test 2: resume restores cron triggers
echo "Test 2: resume restores cron triggers\n";
$ok = xhjob_resume($id1, "lifecycle-svc");
check("xhjob_resume returns true", $ok === true);

sleep(2);
$stateResumed = xhjob_state($id1, "lifecycle-svc");
check("paused == false after resume",
    ($stateResumed['paused'] ?? '') === 'false',
    "paused=" . var_export($stateResumed['paused'] ?? null, true));
check("new triggers after resume",
    ($stateResumed['execution_count'] ?? 0) > $beforeCount,
    "before=$beforeCount after=" . ($stateResumed['execution_count'] ?? 0));

// Test 3: cancel Pending task → CANCELLED state
echo "Test 3: cancel Pending task → CANCELLED state\n";
$id2 = Xhjob::task()
    ->service('lifecycle-svc')
    ->viaShell('echo task2')
    ->cron('*/1 * * * * *')
    ->startAt(time() + 60)  // delay start so it stays PENDING
    ->dispatch();

$state2 = xhjob_state($id2, "lifecycle-svc");
check("task2 starts PENDING (startAt delayed)",
    ($state2['state'] ?? '') === 'pending',
    "state=" . ($state2['state'] ?? ''));

$ok = xhjob_cancel($id2, "lifecycle-svc");
check("xhjob_cancel returns true", $ok === true);

$state2 = xhjob_state($id2, "lifecycle-svc");
check("state == CANCELLED after cancel",
    ($state2['state'] ?? '') === 'cancelled',
    "state=" . ($state2['state'] ?? ''));

// Test 4: remove task definition
echo "Test 4: remove task definition\n";
$id3 = Xhjob::task()
    ->service('lifecycle-svc')
    ->viaShell('echo task3')
    ->cron('*/1 * * * * *')
    ->dispatch();

$ok = xhjob_remove($id3, "lifecycle-svc");
check("xhjob_remove returns true", $ok === true);

// Verify task is no longer in list
$json = xhjob_list("lifecycle-svc");
$tasks = json_decode($json, true);
if (!is_array($tasks)) $tasks = [];
$ids = array_column($tasks, 'id');
check("removed task not in list", !in_array($id3, $ids), "id3 still in list: " . implode(',', $ids));

// Test 5: list all tasks
echo "Test 5: list all tasks\n";
$json = xhjob_list("lifecycle-svc");
$tasks = json_decode($json, true);
check("xhjob_list returns valid JSON array", is_array($tasks), "json=" . substr($json, 0, 100));

// Should have at least id1 (id2 was cancelled but still in store, id3 was removed)
$foundId1 = false;
$foundId2 = false;
foreach ($tasks as $t) {
    if (($t['id'] ?? '') === $id1) $foundId1 = true;
    if (($t['id'] ?? '') === $id2) $foundId2 = true;
}
check("id1 (paused/resumed) in list", $foundId1);
check("id2 (cancelled) in list", $foundId2);
check("id3 (removed) NOT in list", !in_array($id3, array_column($tasks, 'id')));

// Test 6: list by state filter
// 注意：xhjob_list 与 xhjob_state 现在统一返回小写状态值（serde 风格，
// 如 "cancelled"），state_filter 输入参数同样接受小写。
echo "Test 6: list by state filter (cancelled)\n";
$json = xhjob_list("lifecycle-svc", "cancelled");
$tasks = json_decode($json, true);
check("filtered list returns array", is_array($tasks));

$allCancelled = true;
foreach ($tasks as $t) {
    if (($t['state'] ?? '') !== 'cancelled') $allCancelled = false;
}
check("all returned tasks are CANCELLED", $allCancelled);
check("id2 (cancelled) is in CANCELLED filter", in_array($id2, array_column($tasks, 'id')));

// Cleanup
xhjob_remove($id1, "lifecycle-svc");
xhjob_remove($id2, "lifecycle-svc");
xhjob_stop("lifecycle-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
