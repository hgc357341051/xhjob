#!/usr/bin/env php
<?php
/**
 * start_date / end_date Test
 *
 * Verifies cron task time window:
 * - startAt(ts): no triggers before ts
 * - endAt(ts): state=SUCCESS after ts
 * Reference: APScheduler start_date / end_date
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

echo "=== start_end_date.php ===\n\n";

xhjob_start("start-end-svc");

// Test 1: startAt delays triggering
echo "Test 1: startAt delays triggering\n";
$now = time();
$start = $now + 5;  // start in 5 seconds

$id1 = Xhjob::task()
    ->service('start-end-svc')
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->startAt($start)
    ->dispatch();

check("dispatched task with startAt", is_string($id1) && !str_starts_with($id1, "error:"), "id=$id1");

// Wait 3 seconds (before start_date)
sleep(3);
$state = xhjob_state($id1, "start-end-svc");
check("no triggers before start_date",
    ($state['execution_count'] ?? 0) == 0,
    "count=" . ($state['execution_count'] ?? 0));

// Wait until start_date + 3 seconds for at least one trigger
sleep(5);  // total 8 seconds, start was at +5
$state = xhjob_state($id1, "start-end-svc");
check("triggered after start_date",
    ($state['execution_count'] ?? 0) >= 1,
    "count=" . ($state['execution_count'] ?? 0));

// Test 2: endAt stops triggering after ts
echo "Test 2: endAt stops triggering after ts\n";
$now = time();
$end = $now + 3;  // end in 3 seconds

$id2 = Xhjob::task()
    ->service('start-end-svc')
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->endAt($end)
    ->dispatch();

check("dispatched task with endAt", is_string($id2) && !str_starts_with($id2, "error:"), "id=$id2");

// Wait for end_date to pass
sleep(6);
$state = xhjob_state($id2, "start-end-svc");
check("state == SUCCESS after end_date",
    ($state['state'] ?? '') === 'SUCCESS',
    "state=" . ($state['state'] ?? ''));

// Cleanup
xhjob_remove($id1, "start-end-svc");
xhjob_remove($id2, "start-end-svc");
xhjob_stop("start-end-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
