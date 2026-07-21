#!/usr/bin/env php
<?php
/**
 * meta Field Test
 *
 * Verifies withMeta writes user metadata and xhjob_state reads it back.
 * Reference: Celery update_state meta
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

echo "=== meta_field.php ===\n\n";

xhjob_start("meta-svc");

// Test 1: withMeta writes JSON, xhjob_state reads it back
echo "Test 1: withMeta writes, xhjob_state reads\n";
$meta = json_encode(['order_id' => 'A123', 'user' => 'alice', 'priority' => 5]);

$id = Xhjob::task()
    ->service('meta-svc')
    ->viaShell('echo meta-test')
    ->withMeta($meta)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

$state = xhjob_state($id, "meta-svc");
check("state has meta field", isset($state['meta']), "keys: " . implode(',', array_keys($state)));

$stateMeta = $state['meta'] ?? null;
check("meta is the original JSON string",
    $stateMeta === $meta,
    "expected={$meta} got=" . var_export($stateMeta, true));

// Decode and verify structure
$decoded = json_decode($stateMeta, true);
check("meta JSON decodes to array", is_array($decoded));
check("meta order_id == A123", ($decoded['order_id'] ?? '') === 'A123');
check("meta user == alice", ($decoded['user'] ?? '') === 'alice');
check("meta priority == 5", ($decoded['priority'] ?? 0) === 5);

// Test 2: no withMeta → meta is null/string "null"
echo "Test 2: no withMeta → meta is null\n";
$id2 = Xhjob::task()
    ->service('meta-svc')
    ->viaShell('echo no-meta')
    ->dispatch();

$state2 = xhjob_state($id2, "meta-svc");
$meta2 = $state2['meta'] ?? 'null';
check("meta is null when not set",
    $meta2 === 'null' || $meta2 === null || $meta2 === '',
    "meta=" . var_export($meta2, true));

// Test 3: meta appears in xhjob_list output
echo "Test 3: meta in xhjob_list output\n";
$json = xhjob_list("meta-svc");
$tasks = json_decode($json, true);
$foundMetaTask = false;
foreach ($tasks as $t) {
    if (($t['id'] ?? '') === $id) {
        check("meta field exists in TaskSummary", array_key_exists('meta', $t), "keys: " . implode(',', array_keys($t)));
        check("meta value matches", ($t['meta'] ?? null) === $meta, "got=" . var_export($t['meta'] ?? null, true));
        $foundMetaTask = true;
        break;
    }
}
check("meta task found in list", $foundMetaTask);

// Cleanup
xhjob_remove($id, "meta-svc");
xhjob_remove($id2, "meta-svc");
xhjob_stop("meta-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
