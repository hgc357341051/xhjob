--TEST--
xhjob dispatch HTTP task through a user-supplied proxy (XHJOB_TEST_PROXY)
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
$proxy = getenv('XHJOB_TEST_PROXY');
if ($proxy === false || $proxy === '') {
    echo "skip XHJOB_TEST_PROXY env not set\n";
}
?>
--FILE--
<?php
// This test is only run when the user provides a real proxy URL via the
// XHJOB_TEST_PROXY environment variable, e.g.:
//   XHJOB_TEST_PROXY=socks5://127.0.0.1:1080 php test/run-tests.php tests/proxy.phpt
//   XHJOB_TEST_PROXY=http://proxy.local:8080 php test/run-tests.php tests/proxy.phpt
// CI sandboxes typically have no proxy available, so this test SKIPs by default.

$proxy = getenv('XHJOB_TEST_PROXY');

@xhjob_stop(); usleep(300000);
xhjob_start() or die("FAIL: cannot start daemon\n");

// Dispatch an HTTP GET through the supplied proxy via the chainable API.
$id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?src=xhjob-proxy')
    ->withProxy($proxy)
    ->withRetry(0, 1)
    ->timeout(20)
    ->dispatch();

if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop();
    exit(1);
}
echo "dispatched: $id\n";

// Poll until terminal state (max 15 seconds).
$finalState = null;
for ($i = 0; $i < 150; $i++) {
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

// Verify result has body and status_code=200.
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
if (strpos($r['body'], 'xhjob-proxy') === false) {
    echo "FAIL: expected body to contain 'xhjob-proxy'\n";
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
