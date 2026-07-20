--TEST--
xhjob Shell task output encoding conversion (Windows GBK code page)
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
if (PHP_OS_FAMILY !== 'Windows') {
    echo "skip encoding test only runs on Windows (no GBK code page on Unix)\n";
}
?>
--FILE--
<?php
// On Windows the default cmd code page is typically 936 (GBK) on zh-CN
// locales, which produces mojibake when stdout bytes are decoded as UTF-8.
// This test dispatches a Shell task that echoes a Chinese string and asks
// the executor to decode the captured bytes as GBK via withEncoding('GBK').
//
// Unix sandboxes have no GBK shell output path, so the test is SKIpped
// outside Windows.

@xhjob_stop(); usleep(300000);
xhjob_start() or die("FAIL: cannot start daemon\n");

$cmd = 'cmd /C "echo 中文"';

$id = Xhjob::task()
    ->viaShell($cmd)
    ->withEncoding('GBK')
    ->withRetry(0, 1)
    ->timeout(10)
    ->dispatch();

if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: $id\n";
    xhjob_stop();
    exit(1);
}
echo "dispatched: $id\n";

// Poll until terminal state (max 10 seconds).
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

$r = xhjob_result($id);
if (!isset($r['stdout'])) {
    echo "FAIL: missing stdout in result\n";
    var_dump($r);
    xhjob_stop();
    exit(1);
}

$stdout = $r['stdout'];
// The decoded stdout must be valid UTF-8 (mb_check_encoding returns true).
if (!mb_check_encoding($stdout, 'UTF-8')) {
    echo "FAIL: stdout is not valid UTF-8 after GBK decode\n";
    echo "stdout bytes: " . bin2hex($stdout) . "\n";
    xhjob_stop();
    exit(1);
}
echo "stdout is valid UTF-8\n";

// The decoded stdout should contain the Chinese character "中" (U+4E2D).
// We use a broader check because the exact output depends on how cmd echoes
// the input bytes (which can vary by Windows locale / code page).
if (strpos($stdout, '中') === false && strpos($stdout, '文') === false) {
    echo "FAIL: expected stdout to contain at least one of '中' or '文'\n";
    echo "stdout: {$stdout}\n";
    xhjob_stop();
    exit(1);
}
echo "stdout contains expected Chinese characters OK\n";

xhjob_stop();
echo "TEST PASSED\n";
?>
--EXPECTF--
dispatched: %s
final state: SUCCESS
stdout is valid UTF-8
stdout contains expected Chinese characters OK
TEST PASSED
