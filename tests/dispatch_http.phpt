--TEST--
xhjob dispatch HTTP task and verify state/result
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
// Probe network access via file_get_contents on the actual HTTPS URL we'll use.
$probe = @file_get_contents('https://httpbin.org/get?probe=1', false, stream_context_create(['http' => ['timeout' => 5]]));
if ($probe === false) {
    echo "skip no network access to httpbin.org\n";
}
?>
--FILE--
<?php
@xhjob_stop(); usleep(300000);
xhjob_start() or die("FAIL: cannot start daemon\n");

// Dispatch an HTTP GET via raw JSON
$payload = json_encode([
    'task_type'    => 'http',
    'payload'      => ['method' => 'GET', 'url' => 'https://httpbin.org/get?src=xhjob-test', 'headers' => [], 'body' => null],
    'retry_max'    => 0,
    'retry_delay'  => 1,
    'timeout'      => 15,
    'priority'     => 0,
    'allow_overlap'=> false,
    'max_instances'=> 1,
    'coalesce'     => true,
    'persist'      => false,
]);
$id = xhjob_dispatch($payload);
if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop();
    exit(1);
}
echo "dispatched: $id\n";

// Poll until SUCCESS (max 10 seconds)
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($id);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}

if ($finalState !== 'SUCCESS') {
    echo "FAIL: expected SUCCESS, got: $finalState\n";
    var_dump(xhjob_state($id));
    var_dump(xhjob_result($id));
    xhjob_stop();
    exit(1);
}
echo "final state: SUCCESS\n";

// Verify result has body and status_code=200
$r = xhjob_result($id);
if (!isset($r['body']) || !isset($r['status_code'])) {
    echo "FAIL: missing body/status_code in result\n";
    var_dump($r);
    xhjob_stop();
    exit(1);
}
if ((int)$r['status_code'] !== 200) {
    echo "FAIL: expected status_code=200, got: {$r['status_code']}\n";
    xhjob_stop();
    exit(1);
}
if (strpos($r['body'], 'xhjob-test') === false) {
    echo "FAIL: expected body to contain 'xhjob-test'\n";
    echo "body: {$r['body']}\n";
    xhjob_stop();
    exit(1);
}
echo "status_code=200, body contains marker OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
dispatched: %s
final state: SUCCESS
status_code=200, body contains marker OK
TEST PASSED
