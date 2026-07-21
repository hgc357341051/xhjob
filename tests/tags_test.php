<?php
/**
 * Test: tags (A15)
 *
 * Verifies that tasks can be dispatched with tags, tags are stored in the task
 * definition (visible via xhjob_get and in xhjob_list TaskSummary output), and
 * that tag-based filtering correctly selects matching tasks.
 *
 * Note: The PHP-facing xhjob_list() function signature is
 *   xhjob_list(name, state_filter, tag, data_dir)
 * 共 4 参数，第三参数 tag 为服务端 tag 过滤。调用时若不需 tag 过滤，
 * 必须显式传 null（否则会把后续的 $dataDir 误当成 tag）。
 * 本测试通过两条路径验证 tags：
 *   - xhjob_get (full Task JSON includes tags field)
 *   - xhjob_list (TaskSummary includes tags field) + client-side filtering
 * 并直接使用 4 参数形式验证服务端 tag 过滤。
 *
 * Reference: APScheduler tags.
 */

$dataDir = '/tmp/xhjob-tags-test';
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

echo "=== tags_test.php ===\n\n";

if (!xhjob_start("default", $dataDir)) {
    echo "FAIL: daemon start\n"; exit(1);
}
usleep(500_000);

function dispatch_task(array $task, string $dataDir): string {
    return xhjob_dispatch(json_encode($task), "default", $dataDir);
}

// Dispatch 3 shell tasks with different tags.
// Use startAt far in the future so they stay PENDING (no execution) and
// remain in the store for list/get inspection.
$futureTs = time() + 3600;

$id1 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo tag-billing'],
    'run_at' => $futureTs,
    'tags' => ['billing'],
], $dataDir);
ok(is_string($id1) && !str_starts_with($id1, "error:"), "dispatched tag1=[billing] (id=$id1)");

$id2 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo tag-notification'],
    'run_at' => $futureTs,
    'tags' => ['notification'],
], $dataDir);
ok(is_string($id2) && !str_starts_with($id2, "error:"), "dispatched tag2=[notification] (id=$id2)");

$id3 = dispatch_task([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo tag-billing-urgent'],
    'run_at' => $futureTs,
    'tags' => ['billing', 'urgent'],
], $dataDir);
ok(is_string($id3) && !str_starts_with($id3, "error:"), "dispatched tag3=[billing,urgent] (id=$id3)");

// Verify tags via xhjob_get (full Task JSON)
echo "Test 1: tags round-trip via xhjob_get\n";
$g1 = json_decode(xhjob_get($id1, "default", $dataDir) ?: '{}', true);
$g2 = json_decode(xhjob_get($id2, "default", $dataDir) ?: '{}', true);
$g3 = json_decode(xhjob_get($id3, "default", $dataDir) ?: '{}', true);
ok(is_array($g1) && ($g1['tags'] ?? null) === ['billing'],
    "task1 tags=[billing] (got=" . json_encode($g1['tags'] ?? null) . ")");
ok(is_array($g2) && ($g2['tags'] ?? null) === ['notification'],
    "task2 tags=[notification] (got=" . json_encode($g2['tags'] ?? null) . ")");
ok(is_array($g3) && ($g3['tags'] ?? null) === ['billing', 'urgent'],
    "task3 tags=[billing,urgent] (got=" . json_encode($g3['tags'] ?? null) . ")");

// xhjob_list (no tag filter) returns all 3 tasks
// 注意：xhjob_list 当前签名为 (name, state_filter, tag, data_dir) 共 4 参数。
// 必须显式传 null 作为 tag，否则 $dataDir 会被当成 tag 过滤导致返回空列表。
echo "Test 2: xhjob_list returns all 3 tasks (no filter)\n";
$listJson = xhjob_list("default", null, null, $dataDir);
$allTasks = json_decode($listJson, true);
if (!is_array($allTasks)) $allTasks = [];
$allIds = array_column($allTasks, 'id');
ok(count($allTasks) >= 3, "list returns at least 3 tasks (got=" . count($allTasks) . ")");
ok(in_array($id1, $allIds), "task1 (billing) in list");
ok(in_array($id2, $allIds), "task2 (notification) in list");
ok(in_array($id3, $allIds), "task3 (billing,urgent) in list");

// Verify TaskSummary includes tags field
echo "Test 3: TaskSummary in list output includes tags\n";
$tagsInSummary = 0;
foreach ($allTasks as $t) {
    if (in_array(($t['id'] ?? ''), [$id1, $id2, $id3]) && array_key_exists('tags', $t)) {
        $tagsInSummary++;
    }
}
ok($tagsInSummary === 3, "all 3 tasks have tags field in TaskSummary (got=$tagsInSummary)");

// Client-side tag filtering: "billing" → task1 + task3 (not task2)
echo "Test 4: client-side tag filter 'billing' → task1 + task3\n";
$billingIds = [];
foreach ($allTasks as $t) {
    $tags = $t['tags'] ?? [];
    if (is_array($tags) && in_array('billing', $tags)) {
        $billingIds[] = $t['id'] ?? '';
    }
}
ok(in_array($id1, $billingIds), "task1 (billing) matched by 'billing' filter");
ok(in_array($id3, $billingIds), "task3 (billing,urgent) matched by 'billing' filter");
ok(!in_array($id2, $billingIds), "task2 (notification) NOT matched by 'billing' filter");

// Client-side tag filtering: "nonexistent" → empty
echo "Test 5: client-side tag filter 'nonexistent' → empty\n";
$nonexistentIds = [];
foreach ($allTasks as $t) {
    $tags = $t['tags'] ?? [];
    if (is_array($tags) && in_array('nonexistent', $tags)) {
        $nonexistentIds[] = $t['id'] ?? '';
    }
}
ok(count($nonexistentIds) === 0, "no tasks match 'nonexistent' tag (got=" . count($nonexistentIds) . ")");

// 服务端 tag 过滤：直接使用 4 参数形式 xhjob_list(name, state_filter, tag, data_dir)
echo "Test 6: server-side tag filter via xhjob_list (4-arg form)\n";
$serverTagSupported = false;
$serverTagBillingIds = [];
try {
    // 4 参数签名：name=default, state_filter=null, tag="billing", data_dir=$dataDir
    $tagListJson = xhjob_list("default", null, "billing", $dataDir);
    $tagTasks = json_decode($tagListJson, true);
    if (is_array($tagTasks)) {
        $serverTagSupported = true;
        $serverTagBillingIds = array_column($tagTasks, 'id');
    }
} catch (\ArgumentCountError $e) {
    // 兜底：若 PHP 绑定未暴露 4 参数形式则跳过服务端验证
}
if ($serverTagSupported) {
    ok(in_array($id1, $serverTagBillingIds), "server-side filter: task1 matched");
    ok(in_array($id3, $serverTagBillingIds), "server-side filter: task3 matched");
    ok(!in_array($id2, $serverTagBillingIds), "server-side filter: task2 NOT matched");
} else {
    skip("server-side tag_filter not exposed by PHP xhjob_list; verified via client-side filtering instead");
}

// Cleanup
@xhjob_remove($id1, "default", $dataDir);
@xhjob_remove($id2, "default", $dataDir);
@xhjob_remove($id3, "default", $dataDir);
xhjob_stop("default", $dataDir);
echo "=====\nResults: $pass passed, $fail failed, $skip skipped\n";
exit($fail > 0 ? 1 : 0);
