--TEST--
xhjob SQLite schema auto-migration (proxy/encoding/timezone columns)
--SKIPIF--
<?php
if (!extension_loaded('xhjob')) {
    echo "skip xhjob extension not loaded\n";
}
// The `persist` cargo feature must be compiled in for this test to be
// meaningful. Probe by starting a throwaway service with XHJOB_PERSIST=1
// and checking that a DB file gets created inside XHJOB_DB_DIR.
$probeSvc = 'mig-probe-' . getmypid();
$probeDir = '/tmp/xhjob-migration-probe';
@mkdir($probeDir, 0777, true);
@xhjob_stop($probeSvc); usleep(200000);
putenv('XHJOB_DB_DIR=' . $probeDir);
putenv('XHJOB_PERSIST=1');
xhjob_start($probeSvc);
usleep(400000);
$probePath = "{$probeDir}/xhjob.{$probeSvc}.db";
$hasDb = file_exists($probePath);
@xhjob_stop($probeSvc);
usleep(200000);
@unlink($probePath);
@unlink("{$probePath}-wal");
@unlink("{$probePath}-shm");
@rmdir($probeDir);
putenv('XHJOB_DB_DIR');
putenv('XHJOB_PERSIST');
if (!$hasDb) {
    echo "skip persist feature not enabled or DB file not created\n";
}
?>
--ENV--
XHJOB_DB_DIR=/tmp/xhjob-migration-test
XHJOB_PERSIST=1
--FILE--
<?php
// This test verifies that the SQLite store auto-migrates legacy databases
// (created before Task 25, lacking the proxy/encoding/timezone columns) on
// daemon startup.
//
// Flow:
//   1. Pre-create a legacy-schema DB at the service-derived path.
//   2. Insert a legacy 'test-1' task row.
//   3. Start the daemon — it should ALTER TABLE to add the missing columns.
//   4. Verify 'test-1' is recovered as PENDING.
//   5. Verify the new columns now exist via PRAGMA table_info.
//   6. Dispatch a new task that uses withProxy + withEncoding + withTimezone.
//   7. Verify the new task runs to SUCCESS and the three fields are persisted.
//   8. Cleanup.

$svc = 'migration-' . getmypid();
$dbDir = '/tmp/xhjob-migration-test';
$dbPath = "{$dbDir}/xhjob.{$svc}.db";

@mkdir($dbDir, 0777, true);

// Clean slate
@xhjob_stop($svc); usleep(300000);
@unlink($dbPath);
@unlink("{$dbPath}-wal");
@unlink("{$dbPath}-shm");

// 1. Pre-create a legacy schema DB (no proxy/encoding/timezone columns) and
//    insert a PENDING task row that the daemon must recover after migration.
$db = new PDO('sqlite:' . $dbPath);
$db->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
$db->exec("CREATE TABLE tasks (id TEXT PRIMARY KEY, type TEXT, payload TEXT, cron TEXT, retry_max INTEGER, retry_delay INTEGER, timeout INTEGER, priority INTEGER, allow_overlap INTEGER, max_instances INTEGER, coalesce INTEGER, persist INTEGER, state TEXT, attempts INTEGER, next_fire INTEGER, created_at INTEGER, started_at INTEGER, finished_at INTEGER, last_error TEXT)");
$db->exec("CREATE TABLE results (task_id TEXT PRIMARY KEY, body TEXT, status_code INTEGER, stdout TEXT, stderr TEXT, exit_code INTEGER)");
$now = time();
$db->exec("INSERT INTO tasks (id, type, payload, state, created_at) VALUES ('test-1', 'shell', '{\"cmd\":\"echo hi\"}', 'PENDING', {$now})");
$db = null; // close PDO handle so the daemon can open the file exclusively

// Sanity check: confirm the legacy schema lacks the new columns.
$check = new PDO('sqlite:' . $dbPath);
$cols = $check->query("PRAGMA table_info(tasks)")->fetchAll(PDO::FETCH_ASSOC);
$check = null;
$names = array_column($cols, 'name');
if (in_array('proxy', $names) || in_array('encoding', $names) || in_array('timezone', $names)) {
    echo "FAIL: pre-migration DB unexpectedly contains new columns\n";
    exit(1);
}
echo "legacy schema lacks proxy/encoding/timezone OK\n";

// 2. Start the daemon — it should auto-migrate the schema on open().
$r = xhjob_start($svc);
if ($r !== true) { echo "FAIL: xhjob_start returned: "; var_dump($r); exit(1); }
echo "daemon started OK\n";

// Give the daemon a moment to recover active tasks.
usleep(500000);

// 3. The legacy 'test-1' row should have been recovered as PENDING.
$s = xhjob_state('test-1', $svc);
if (($s['state'] ?? '') !== 'PENDING') {
    echo "FAIL: expected test-1 to be PENDING after migration, got: ";
    var_dump($s);
    xhjob_stop($svc); exit(1);
}
echo "legacy task recovered: state=PENDING OK\n";

// 4. Verify the new columns now exist in the DB.
$verify = new PDO('sqlite:' . $dbPath);
$colsAfter = $verify->query("PRAGMA table_info(tasks)")->fetchAll(PDO::FETCH_ASSOC);
$verify = null;
$namesAfter = array_column($colsAfter, 'name');
foreach (['proxy', 'encoding', 'timezone'] as $col) {
    if (!in_array($col, $namesAfter)) {
        echo "FAIL: expected column {$col} after migration, got: " . implode(',', $namesAfter) . "\n";
        xhjob_stop($svc); exit(1);
    }
}
echo "schema migrated: proxy/encoding/timezone columns present OK\n";

// 5. Dispatch a new task that uses all three new fields. Shell task ignores
//    proxy at execution time but the field is still persisted to the DB.
//    Asia/Shanghai is a valid IANA zone so dispatch() must accept it.
$id = Xhjob::task()
    ->service($svc)
    ->viaShell('echo migration-test')
    ->withProxy('http://127.0.0.1:9')
    ->withEncoding('UTF-8')
    ->withTimezone('Asia/Shanghai')
    ->withRetry(0, 1)
    ->timeout(10)
    ->dispatch();

if (strpos($id, '-') === false) {
    echo "FAIL: expected UUID, got: {$id}\n";
    xhjob_stop($svc); exit(1);
}
echo "new task dispatched with proxy/encoding/timezone: {$id} OK\n";

// 6. Wait for the new task to reach a terminal state.
$final = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($id, $svc);
    $st = $s['state'] ?? 'UNKNOWN';
    if ($st === 'SUCCESS' || $st === 'FAILED') {
        $final = $st;
        break;
    }
    usleep(100000);
}
if ($final !== 'SUCCESS') {
    echo "FAIL: expected SUCCESS for new task, got: {$final}\n";
    var_dump(xhjob_state($id, $svc));
    var_dump(xhjob_result($id, $svc));
    xhjob_stop($svc); exit(1);
}
echo "new task completed: SUCCESS OK\n";

// 7. Verify the new task's proxy/encoding/timezone were persisted to the DB.
$check2 = new PDO('sqlite:' . $dbPath);
$stmt = $check2->prepare("SELECT proxy, encoding, timezone FROM tasks WHERE id = ?");
$stmt->execute([$id]);
$row = $stmt->fetch(PDO::FETCH_ASSOC);
$check2 = null;
if ($row === false) {
    echo "FAIL: new task row not found in DB\n";
    xhjob_stop($svc); exit(1);
}
if ($row['proxy'] !== 'http://127.0.0.1:9') {
    echo "FAIL: expected proxy='http://127.0.0.1:9', got: " . var_export($row['proxy'], true) . "\n";
    xhjob_stop($svc); exit(1);
}
if ($row['encoding'] !== 'UTF-8') {
    echo "FAIL: expected encoding='UTF-8', got: " . var_export($row['encoding'], true) . "\n";
    xhjob_stop($svc); exit(1);
}
if ($row['timezone'] !== 'Asia/Shanghai') {
    echo "FAIL: expected timezone='Asia/Shanghai', got: " . var_export($row['timezone'], true) . "\n";
    xhjob_stop($svc); exit(1);
}
echo "new task proxy/encoding/timezone persisted OK\n";

// 8. Cleanup
xhjob_stop($svc);
usleep(300000);
@unlink($dbPath);
@unlink("{$dbPath}-wal");
@unlink("{$dbPath}-shm");
@rmdir($dbDir);

echo "TEST PASSED\n";
?>
--EXPECTF--
legacy schema lacks proxy/encoding/timezone OK
daemon started OK
legacy task recovered: state=PENDING OK
schema migrated: proxy/encoding/timezone columns present OK
new task dispatched with proxy/encoding/timezone: %s OK
new task completed: SUCCESS OK
new task proxy/encoding/timezone persisted OK
TEST PASSED
