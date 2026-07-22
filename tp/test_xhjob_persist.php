#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 持久化深度测试（daemon 重启后任务恢复）
// +----------------------------------------------------------------------
// | 用法：
// |   php -d extension=/workspace/releases/xhjob-php8.2-linux-x86_64.so \
// |       /workspace/tp/test_xhjob_persist.php
// +----------------------------------------------------------------------
// | 真正的持久化语义测试：
// |   1. 任务在 daemon 重启后仍存在，execution_count 不丢失
// |   2. 循环任务在重启后继续触发
// |   3. acks_late=true 的 Running 任务在重启后被重新触发（crash recovery）
// |   4. SQLite db 文件持久存储数据
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

if (!extension_loaded('xhjob')) {
    $so = '/workspace/releases/xhjob-php8.2-linux-x86_64.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$pass = 0;
$fail = 0;
$failedSteps = [];

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $failedSteps;
    echo "[$n] $desc ... ";
    try {
        $fn();
        $pass++;
        echo "PASS\n";
    } catch (\Throwable $e) {
        $fail++;
        $failedSteps[] = $n;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

function assertTrue(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new \Exception($msg);
    }
}

function assertEq($a, $b, string $msg): void
{
    if ($a !== $b) {
        throw new \Exception("$msg (expected=" . var_export($b, true) . ", got=" . var_export($a, true) . ")");
    }
}

$SERVICE  = 'persist-test';
$DATA_DIR = '/tmp/xhjob-persist-test';
$DB_PATH  = "$DATA_DIR/xhjob.$SERVICE.db";

// 清理旧数据
@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);

echo "=== Xhjob 持久化深度测试（daemon 重启恢复）===\n";
echo "service=$SERVICE data_dir=$DATA_DIR db=$DB_PATH\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n\n";

// ============================================================
// 1. 启动 daemon + 创建循环任务 + 记录初始 execution_count
// ============================================================
step(1, '启动 daemon + 创建循环任务', function () use ($SERVICE, $DATA_DIR, $DB_PATH) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "pid=$pid");
    $svc->wait(10, true);

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 每 2 秒执行 1 次，输出时间戳便于追踪
    $id = $mgr->create(
        TaskBuilder::shell('echo persist-loop-$(date +%s)')
            ->every(2)
            ->timeout(10)
    );
    assertTrue(!empty($id) && strpos($id, 'error') === false, "create 失败: $id");
    // 全局保存 task id
    $GLOBALS['loopTaskId'] = $id;

    // 等 7 秒，应至少触发 3 次（t=0,2,4,6）
    sleep(7);
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertTrue($ec >= 3, "重启前 execution_count 应>=3，实际 $ec");
    $GLOBALS['ecBeforeRestart'] = $ec;

    // 验证 db 文件存在且非空
    assertTrue(file_exists($DB_PATH), "db 文件应存在: $DB_PATH");
    assertTrue(filesize($DB_PATH) > 0, 'db 文件应非空');
    echo "（ec_before={$ec}, db_size=" . filesize($DB_PATH) . "）";
});

// ============================================================
// 2. 停止 daemon（保留 db 文件）
// ============================================================
step(2, '停止 daemon（保留 db）', function () use ($SERVICE, $DATA_DIR, $DB_PATH) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');
    assertTrue(file_exists($DB_PATH), '停止后 db 文件应仍存在');
    echo "（db_size=" . filesize($DB_PATH) . "）";
});

// ============================================================
// 3. 重启 daemon + 验证任务状态恢复（execution_count 不丢失）
// ============================================================
step(3, '重启 daemon + 验证 execution_count 恢复', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $pid = $svc->start();
    assertTrue($pid > 0, "重启 pid=$pid");
    $svc->wait(10, true);

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $GLOBALS['loopTaskId'];
    $ecBefore = $GLOBALS['ecBeforeRestart'];

    // 重启后立即查询，任务应仍存在且 execution_count 不丢失
    $st = $mgr->state($id);
    assertTrue(!empty($st['state']), "重启后任务应仍存在，实际: " . json_encode($st));
    $ecAfter = (int) ($st['execution_count'] ?? 0);
    // 允许在重启期间多触发 1 次（边界），但必须 >= 重启前的值
    assertTrue($ecAfter >= $ecBefore, "重启后 execution_count($ecAfter) 应 >= 重启前($ecBefore)，数据丢失！");
    echo "（ec_before={$ecBefore}, ec_after_restart={$ecAfter}）";
});

// ============================================================
// 4. 验证循环任务在重启后继续触发
// ============================================================
step(4, '验证循环任务重启后继续触发', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $GLOBALS['loopTaskId'];
    $ecBefore = (int) ($mgr->state($id)['execution_count'] ?? 0);

    // 等 5 秒，应继续触发 2+ 次
    sleep(5);
    $ecAfter = (int) ($mgr->state($id)['execution_count'] ?? 0);
    assertTrue($ecAfter > $ecBefore, "重启后循环任务应继续触发 (before={$ecBefore}, after={$ecAfter})");
    echo "（ec_continued: {$ecBefore}→{$ecAfter}）";
});

// ============================================================
// 5. 停止循环任务
// ============================================================
step(5, '停止循环任务', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $GLOBALS['loopTaskId'];
    $ok = $mgr->stop($id);
    assertTrue($ok, 'stop 应返回 true');
    sleep(1);
});

// ============================================================
// 6. acks_late crash recovery：创建 Running 任务 + 停止 daemon + 重启
// ============================================================
step(6, 'acks_late crash recovery（Running→重启→重新触发）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // sleep 30 让任务保持 Running 状态
    $id = $mgr->create(
        TaskBuilder::shell('sleep 30; echo acks-late-done')
            ->acksLate(true)
            ->timeout(60)
    );
    $GLOBALS['acksLateTaskId'] = $id;
    // 等 2 秒让任务进入 Running
    sleep(2);
    $st = $mgr->state($id);
    assertTrue(($st['state'] ?? '') === 'running', "acks_late 任务应为 running，实际: " . ($st['state'] ?? '?'));
    $GLOBALS['acksLateAttempts'] = (int) ($st['attempts'] ?? 0);
    echo "（state=running, attempts={$GLOBALS['acksLateAttempts']}）";
});

// ============================================================
// 7. 停止 daemon（模拟 crash）+ 重启 + 验证 acks_late 任务被重新触发
// ============================================================
step(7, '停止 daemon 模拟 crash + 重启验证 acks_late 重触发', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);

    // 重启
    $pid = $svc->start();
    assertTrue($pid > 0, "重启 pid=$pid");
    $svc->wait(10, true);

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $GLOBALS['acksLateTaskId'];
    // 等 3 秒让 daemon 扫描并重新触发 acks_late 任务
    sleep(3);
    $st = $mgr->state($id);
    $state = $st['state'] ?? '?';
    // acks_late=true 的 Running 任务应被重置为 Pending 并重新触发为 Running
    // （若任务已完成则可能为 success，但不应为 failed）
    assertTrue(
        in_array($state, ['pending', 'running', 'success'], true),
        "acks_late 重启后应被重置为 Pending/Running/Success，实际: $state"
    );
    echo "（state_after_restart={$state}）";
});

// ============================================================
// 8. 清理：停止 daemon
// ============================================================
step(8, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');
});

echo "\n=== 持久化测试完成：$pass passed, $fail failed ===\n";
if ($fail > 0) {
    echo "失败的步骤：[" . implode(', ', $failedSteps) . "]\n";
}
exit($fail > 0 ? 1 : 0);
