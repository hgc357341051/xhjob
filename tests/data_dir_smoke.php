<?php
// Smoke test: data_dir parameter should place all files in specified dir.
// Uses the Xhjob class chainable API with dataDir().
$dir = '/tmp/xhjob-data-dir-test';
@mkdir($dir, 0755, true);

// Enable SQLite persistence so we can verify the .db file lands in data_dir.
putenv('XHJOB_PERSIST=1');

// Clean state
foreach (glob("$dir/xhjob.smoke.*") as $f) @unlink($f);

echo "==> Starting daemon with data_dir=$dir\n";
$ok = xhjob_start('smoke', $dir);
echo "start: " . ($ok ? "OK" : "FAIL") . "\n";

$status = xhjob_status('smoke', $dir);
echo "status: " . json_encode($status) . "\n";

if (!$ok || empty($status['running'])) {
    echo "FAIL: daemon not running\n";
    exit(1);
}

// Verify files exist in the dir
$files = glob("$dir/xhjob.smoke.*");
echo "files in $dir:\n";
foreach ($files as $f) echo "  - $f\n";

if (empty($files)) {
    echo "FAIL: no files in data_dir\n";
    exit(1);
}

// Dispatch a shell task via the Xhjob class API with dataDir()
echo "\n==> Dispatching shell task via Xhjob::task()->dataDir()\n";
$id = Xhjob::task()
    ->service('smoke')
    ->dataDir($dir)
    ->viaShell('echo hello-data-dir')
    ->withRetry(1, 1)
    ->timeout(5)
    ->persist(true)
    ->dispatch();
echo "task_id: $id\n";

if (strlen($id) < 10) {
    echo "FAIL: dispatch returned: $id\n";
    xhjob_stop('smoke', $dir);
    exit(1);
}

// Wait for completion
$final = null;
for ($i = 0; $i < 50; $i++) {
    $state = xhjob_state($id, 'smoke', $dir);
    $s = $state['state'] ?? 'UNKNOWN';
    if (in_array($s, ['success', 'failed', 'cancelled'], true)) {
        $final = $state;
        break;
    }
    usleep(200_000);
}
echo "final state: " . json_encode($final) . "\n";

$result = xhjob_result($id, 'smoke', $dir);
echo "result: " . json_encode($result) . "\n";

if (($final['state'] ?? '') !== 'success') {
    echo "FAIL: task did not succeed\n";
    xhjob_stop('smoke', $dir);
    exit(1);
}

$stdout = $result['stdout'] ?? '';
if (strpos($stdout, 'hello-data-dir') === false) {
    echo "FAIL: stdout mismatch: $stdout\n";
    xhjob_stop('smoke', $dir);
    exit(1);
}

// Stop daemon
echo "\n==> Stopping daemon\n";
$stopped = xhjob_stop('smoke', $dir);
echo "stop: " . ($stopped ? "OK" : "FAIL") . "\n";

// Verify .db file exists (data persisted in data_dir)
echo "\n==> Final file listing:\n";
foreach (glob("$dir/xhjob.smoke.*") as $f) echo "  - $f\n";

$dbExists = file_exists("$dir/xhjob.smoke.db");
echo "db file exists: " . ($dbExists ? "YES" : "NO") . "\n";
if (!$dbExists) {
    echo "FAIL: db file not in data_dir\n";
    exit(1);
}

echo "\nSmoketest PASSED\n";
