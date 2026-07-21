#!/usr/bin/env php
<?php
/**
 * Jitter Test (A9)
 *
 * Verifies jitter(N) spreads task triggers over time.
 * Reference: APScheduler jitter.
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

echo "=== jitter_test.php ===\n\n";

xhjob_start("jitter-svc");

// Test 1: jitter(5) is reflected in xhjob_state().
echo "Test 1: jitter(5) field round-trip\n";
$id = Xhjob::task()
    ->service('jitter-svc')
    ->viaShell('echo jitter')
    ->every(2)
    ->jitter(5)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

$state = xhjob_state($id, "jitter-svc");
check("jitter == 5 in state info",
    ($state['jitter'] ?? '') === '5',
    "jitter=" . var_export($state['jitter'] ?? null, true));
check("interval == 2 in state info",
    ($state['interval'] ?? '') === '2',
    "interval=" . var_export($state['interval'] ?? null, true));

// Test 2: Multiple jitter tasks eventually fire at least once.
echo "Test 2: jitter(5) tasks fire within reasonable window\n";
$ids = [];
for ($i = 0; $i < 3; $i++) {
    $tid = Xhjob::task()
        ->service('jitter-svc')
        ->viaShell('echo multi-' . $i)
        ->every(3)
        ->jitter(5)
        ->dispatch();
    if (is_string($tid) && !str_starts_with($tid, "error:")) {
        $ids[] = $tid;
    }
}
check("dispatched 3 jitter tasks", count($ids) === 3, "count=" . count($ids));

// Wait up to 12 seconds for all 3 tasks to fire at least once.
$start = time();
$firedCount = 0;
while (time() - $start < 12) {
    $firedCount = 0;
    foreach ($ids as $tid) {
        $s = xhjob_state($tid, "jitter-svc");
        if (($s['execution_count'] ?? 0) >= 1) {
            $firedCount++;
        }
    }
    if ($firedCount === count($ids)) break;
    usleep(500_000);
}
check("all 3 jitter tasks fired at least once",
    $firedCount === count($ids),
    "fired={$firedCount}/" . count($ids));

// Cleanup
xhjob_remove($id, "jitter-svc");
foreach ($ids as $tid) {
    xhjob_remove($tid, "jitter-svc");
}
xhjob_stop("jitter-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
