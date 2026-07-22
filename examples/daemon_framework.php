<?php
/**
 * Example: 完整的 daemon 框架流程演示
 *
 * 展示如何在 PHP-FPM / CLI 环境中通过框架类（XhjobService + TaskManager +
 * TaskBuilder + Client）管理 daemon 生命周期与任务调度。
 *
 * 用法：
 *   php -d extension=xhjob.so examples/daemon_framework.php
 */
require_once __DIR__ . '/../framework/autoload.php';

$DATA_DIR = '/tmp/xhjob-demo-framework';
$SERVICE  = 'demo-fw';

// 清理上次残留
@unlink("$DATA_DIR/xhjob.$SERVICE.pid");
@unlink("$DATA_DIR/xhjob.$SERVICE.sock");

echo "=== xhjob 框架完整流程演示 ===\n\n";

// ---------------------------------------------------------------
// 1. 启动 daemon（XhjobService）
// ---------------------------------------------------------------
echo "[1] 启动 daemon...\n";
$svc = new XhjobService($SERVICE, $DATA_DIR);
$svc->ensureStopped();
$pid = $svc->start();
echo "    daemon PID = {$pid}\n";

$health = $svc->healthCheck();
echo "    healthy = " . ($health['healthy'] ? 'true' : 'false') . "\n";
echo "    stats   = " . json_encode($health['stats']) . "\n\n";

// ---------------------------------------------------------------
// 2. 派发 shell 任务（TaskManager + TaskBuilder）
// ---------------------------------------------------------------
echo "[2] 派发 shell 任务...\n";
$mgr = new TaskManager($SERVICE, $DATA_DIR);
$id1 = $mgr->create(
    TaskBuilder::shell('echo hello-from-framework')
        ->withRetry(2, 1)
        ->timeout(10)
);
echo "    task_id = {$id1}\n";

// 等待完成
$mgr->waitForState($id1, 'success', 15);
$r = $mgr->result($id1);
echo "    state   = " . ($mgr->state($id1)['state'] ?? '?') . "\n";
echo "    stdout  = " . trim($r['stdout'] ?? '') . "\n";
echo "    exit    = " . ($r['exit_code'] ?? '?') . "\n\n";

// ---------------------------------------------------------------
// 3. 派发带标签的任务
// ---------------------------------------------------------------
echo "[3] 派发带标签的任务...\n";
$id2 = $mgr->create(
    TaskBuilder::shell('echo tagged-task')
        ->tags(['demo', 'batch'])
        ->priority(5)
);
echo "    task_id = {$id2}\n";
$mgr->waitForState($id2, 'success', 10);
echo "    state   = " . ($mgr->state($id2)['state'] ?? '?') . "\n\n";

// ---------------------------------------------------------------
// 4. 派发 chain 任务
// ---------------------------------------------------------------
echo "[4] 派发 chain 任务...\n";
$chainId = $mgr->createChain([
    TaskBuilder::shell('echo step1-output'),
    TaskBuilder::shell('cat'),
]);
echo "    chain_id = {$chainId}\n";
// 等待 chain 完成
$deadline = time() + 15;
while (time() < $deadline) {
    $cs = $mgr->chainState($chainId);
    if ($cs !== null && ($cs['state'] ?? '') === 'success') break;
    usleep(300000);
}
echo "    chain state = " . ($cs['state'] ?? '?') . "\n\n";

// ---------------------------------------------------------------
// 5. 派发 group 任务
// ---------------------------------------------------------------
echo "[5] 派发 group 任务...\n";
$groupId = $mgr->createGroup([
    TaskBuilder::shell('echo group-a'),
    TaskBuilder::shell('echo group-b'),
    TaskBuilder::shell('echo group-c'),
]);
echo "    group_id = {$groupId}\n";
$deadline = time() + 15;
while (time() < $deadline) {
    $gs = $mgr->groupState($groupId);
    if ($gs !== null && ($gs['state'] ?? '') === 'success') break;
    usleep(300000);
}
echo "    group state = " . ($gs['state'] ?? '?') . "\n\n";

// ---------------------------------------------------------------
// 6. 使用 Client（跨环境客户端）
// ---------------------------------------------------------------
echo "[6] 使用 Client 派发任务...\n";
$client = new Client($SERVICE, $DATA_DIR, 10);
$client->retry(2, 100);
$id3 = $client->dispatch(
    TaskBuilder::shell('echo client-side')
        ->withMeta('{"source":"demo"}')
);
echo "    task_id = {$id3}\n";
$deadline = time() + 10;
while (time() < $deadline) {
    $st = $client->state($id3);
    if (($st['state'] ?? '') === 'success') break;
    usleep(200000);
}
$r3 = $client->result($id3);
echo "    state   = " . ($st['state'] ?? '?') . "\n";
echo "    stdout  = " . trim($r3['stdout'] ?? '') . "\n\n";

// ---------------------------------------------------------------
// 7. list 查询
// ---------------------------------------------------------------
echo "[7] 查询任务列表...\n";
$list = $mgr->list();
echo "    总任务数 = " . count($list) . "\n";
foreach ($list as $task) {
    echo "    - id=" . substr($task['id'] ?? '?', 0, 8)
         . " type=" . ($task['task_type'] ?? '?')
         . " state=" . ($task['state'] ?? '?') . "\n";
}
echo "\n";

// ---------------------------------------------------------------
// 8. 事件日志查询
// ---------------------------------------------------------------
echo "[8] 查询任务事件日志...\n";
$logs = $mgr->logs($id1);
foreach ($logs as $log) {
    $ts = date('H:i:s', (int)($log['ts'] ?? 0));
    echo "    [{$ts}] " . ($log['event_type'] ?? 'UNKNOWN') . "\n";
}
echo "\n";

// ---------------------------------------------------------------
// 9. 重启任务（requeue）
// ---------------------------------------------------------------
echo "[9] 重启任务（requeue）...\n";
$mgr->restart($id1);
$mgr->waitForState($id1, 'success', 15);
echo "    重启后 state = " . ($mgr->state($id1)['state'] ?? '?') . "\n\n";

// ---------------------------------------------------------------
// 10. 停止 daemon
// ---------------------------------------------------------------
echo "[10] 停止 daemon...\n";
$svc->stop();
$svc->wait(10, false);
$st = $svc->status();
echo "     running = " . ($st['running'] ? 'true' : 'false') . "\n";
echo "     pid 文件清理 = " . (file_exists("$DATA_DIR/xhjob.$SERVICE.pid") ? 'no' : 'yes') . "\n";

echo "\n=== 演示完成 ===\n";
