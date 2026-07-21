#!/usr/bin/env php
<?php
/**
 * get_job Test (A12)
 *
 * Verifies that xhjob_get() returns the full Task JSON for a dispatched task,
 * containing all configuration fields (cron / retry_max / timeout / priority
 * / allow_overlap / max_instances / coalesce / interval / run_at / etc.),
 * distinguishing it from xhjob_state() (which returns the trimmed StateInfo).
 *
 * Also verifies that xhjob_get() of a non-existent id returns null.
 *
 * Reference: APScheduler get_job.
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

echo "=== get_job_test.php ===\n\n";

xhjob_start("getjob-svc");

// Test 1: dispatch a cron task with many configuration fields and verify
// xhjob_get returns the full Task JSON.
echo "Test 1: xhjob_get returns full Task JSON with config fields\n";
$id = Xhjob::task()
    ->service('getjob-svc')
    ->viaShell('echo getjob-target')
    ->cron('*/5 * * * *')
    ->withRetry(7, 13)
    ->timeout(45)
    ->priority(9)
    ->allowOverlap(true)
    ->maxInstances(3)
    ->coalesce(false)
    ->maxExecutions(5)
    ->resultTtl(120)
    ->withMeta('{"k":"v"}')
    ->withTimezone('Asia/Shanghai')
    ->jitter(4)
    ->expires(60)
    ->retryBackoff(true)
    ->dispatch();

check("dispatched", is_string($id) && !str_starts_with($id, "error:"), "id=$id");

// Wait briefly for the task to be recorded by the daemon.
usleep(200_000);

$json = xhjob_get($id, "getjob-svc");
check("xhjob_get returns a string", is_string($json), "type=" . gettype($json));

if (is_string($json)) {
    $task = json_decode($json, true);
    check("json_decode succeeds", is_array($task), "json_err=" . json_last_error_msg());

    if (is_array($task)) {
        // Identity + execution metadata.
        // Note: TaskState serializes to PascalCase (e.g. "Pending" / "Running")
        // via serde's default enum serialization; this differs from
        // xhjob_state() which returns the uppercase form via as_str().
        check("id matches dispatched id", ($task['id'] ?? '') === $id, "id=" . ($task['id'] ?? ''));
        check("task_type=shell", ($task['task_type'] ?? '') === 'shell', "task_type=" . ($task['task_type'] ?? ''));
        check("state is Pending or Running",
            in_array($task['state'] ?? '', ['Pending', 'Running', 'PENDING', 'RUNNING']),
            "state=" . ($task['state'] ?? ''));
        check("attempts present", array_key_exists('attempts', $task));
        check("execution_count present", array_key_exists('execution_count', $task));
        check("created_at present", array_key_exists('created_at', $task));
        check("next_fire present", array_key_exists('next_fire', $task));
        check("paused present", array_key_exists('paused', $task));
        check("cancel_requested present", array_key_exists('cancel_requested', $task));

        // Configuration fields that distinguish `get` from `state`.
        check("cron='*/5 * * * *'", ($task['cron'] ?? null) === '*/5 * * * *', "cron=" . var_export($task['cron'] ?? null, true));
        check("retry_max=7", ($task['retry_max'] ?? -1) === 7, "retry_max=" . var_export($task['retry_max'] ?? null, true));
        check("retry_delay=13", ($task['retry_delay'] ?? -1) === 13, "retry_delay=" . var_export($task['retry_delay'] ?? null, true));
        check("timeout=45", ($task['timeout'] ?? -1) === 45, "timeout=" . var_export($task['timeout'] ?? null, true));
        check("priority=9", ($task['priority'] ?? 0) === 9, "priority=" . var_export($task['priority'] ?? null, true));
        check("allow_overlap=true", ($task['allow_overlap'] ?? null) === true, "allow_overlap=" . var_export($task['allow_overlap'] ?? null, true));
        check("max_instances=3", ($task['max_instances'] ?? 0) === 3, "max_instances=" . var_export($task['max_instances'] ?? null, true));
        check("coalesce=false", ($task['coalesce'] ?? null) === false, "coalesce=" . var_export($task['coalesce'] ?? null, true));
        check("max_executions=5", ($task['max_executions'] ?? 0) === 5, "max_executions=" . var_export($task['max_executions'] ?? null, true));
        check("result_ttl=120", ($task['result_ttl'] ?? 0) === 120, "result_ttl=" . var_export($task['result_ttl'] ?? null, true));
        check("meta='{\"k\":\"v\"}'", ($task['meta'] ?? null) === '{"k":"v"}', "meta=" . var_export($task['meta'] ?? null, true));
        check("timezone='Asia/Shanghai'", ($task['timezone'] ?? null) === 'Asia/Shanghai', "timezone=" . var_export($task['timezone'] ?? null, true));
        check("jitter=4", ($task['jitter'] ?? 0) === 4, "jitter=" . var_export($task['jitter'] ?? null, true));
        check("expires=60", ($task['expires'] ?? 0) === 60, "expires=" . var_export($task['expires'] ?? null, true));
        check("retry_backoff=true", ($task['retry_backoff'] ?? null) === true, "retry_backoff=" . var_export($task['retry_backoff'] ?? null, true));
        check("payload.cmd='echo getjob-target'",
            ($task['payload']['cmd'] ?? null) === 'echo getjob-target',
            "payload=" . json_encode($task['payload'] ?? null));
    }
}

// Test 2: xhjob_get of a non-existent id returns null.
echo "Test 2: xhjob_get of non-existent id returns null\n";
$result = xhjob_get("nonexistent-id-99999", "getjob-svc");
check("xhjob_get of non-existent returns null", $result === null, "type=" . gettype($result));

// Cleanup
xhjob_remove($id, "getjob-svc");
xhjob_stop("getjob-svc");

echo "\n=== Summary ===\n";
echo "PASS: {$pass}\n";
echo "FAIL: {$fail}\n";
exit($fail === 0 ? 0 : 1);
