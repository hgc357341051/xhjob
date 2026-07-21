#!/usr/bin/env php
<?php
/**
 * Expires Test (C6)
 *
 * Verifies that a Pending task whose `expires` window has elapsed transitions
 * to the Expired terminal state. Uses `startAt` far in the future so the task
 * stays Pending (never fires) and `expires(2)` to mark it stale after 2s.
 *
 * Reference: APScheduler expires.
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

echo "=== expires_test.php ===\n\n";

xhjob_start("expires-svc");

// Test 1: expires(2) field round-trip via xhjob_state.
echo "Test 1: expires(2) field round-trip\n";
$id = Xhjob::task()
    ->service('expires-svc')
    ->viaShell('echo expires')
    ->startAt(time() + 3600) // delay start so it stays Pending
    ->expires(2)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

$state = xhjob_state($id, "expires-svc");
check("state == PENDING initially",
    ($state['state'] ?? '') === 'PENDING',
    "state=" . ($state['state'] ?? ''));
check("expires == 2 in state info",
    ($state['expires'] ?? '') === '2',
    "expires=" . var_export($state['expires'] ?? null, true));

// Test 2: after 4 seconds the Pending task should transition to EXPIRED.
echo "Test 2: Pending task becomes EXPIRED after expires window\n";
$start = time();
$finalState = '';
while (time() - $start < 8) {
    $s = xhjob_state($id, "expires-svc");
    $finalState = $s['state'] ?? '';
    if ($finalState === 'EXPIRED') break;
    usleep(500_000);
}

check("state == EXPIRED after expires window",
    $finalState === 'EXPIRED',
    "state=" . $finalState);

// Verify EXPIRED is terminal (no further state changes).
sleep(1);
$s = xhjob_state($id, "expires-svc");
check("state stays EXPIRED (terminal)",
    ($s['state'] ?? '') === 'EXPIRED',
    "state=" . ($s['state'] ?? ''));

// Test 3: xhjob_list with EXPIRED filter returns the expired task.
echo "Test 3: xhjob_list state filter EXPIRED\n";
$json = xhjob_list("expires-svc", "EXPIRED");
$tasks = json_decode($json, true);
if (!is_array($tasks)) $tasks = [];
$found = false;
foreach ($tasks as $t) {
    if (($t['id'] ?? '') === $id) { $found = true; break; }
}
check("expired task appears in EXPIRED filter list", $found, "id=$id not in EXPIRED list");

// Cleanup
xhjob_remove($id, "expires-svc");
xhjob_stop("expires-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
