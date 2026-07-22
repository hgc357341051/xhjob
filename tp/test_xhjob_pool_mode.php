#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 两种池模式对比测试
// +----------------------------------------------------------------------
// | 验证 async（默认 async task 池，tokio M:N，最大并发 1024）
// | 与 thread（1:1 线程池，std::thread + crossbeam-channel，线程数=CPU 核数）
// | 两种模式都能正常工作。
// |
// | 用法：
// |   EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so
// |   php -d extension=$EXT /workspace/tp/test_xhjob_pool_mode.php
// +----------------------------------------------------------------------
// | 原理：daemon 启动时读取 XHJOB_POOL_MODE 环境变量决定使用哪种池模式。
// | daemon 进程由 PHP 进程 fork 而来，会继承 PHP 进程的环境变量，
// | 因此在 xhjob_start 前调用 putenv("XHJOB_POOL_MODE=thread") 即可切换模式。
// | 两种模式切换时必须停止 daemon、清理 data_dir、重新启动。
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

$SERVICE  = 'pool-mode-test';
$DATA_DIR = '/tmp/xhjob-pool-mode-test';

$pass = 0;
$fail = 0;
$failedSteps = [];

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $failedSteps;
    echo "  [$n] $desc ... ";
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

/**
 * 以指定池模式运行全部测试步骤
 *
 * @param string $mode 'async' 或 'thread'（'coroutine' 为 'async' 兼容别名）
 * @return array{0:int,1:int,2:array} [pass, fail, failedSteps]
 */
function runTests(string $mode): array
{
    global $SERVICE, $DATA_DIR, $pass, $fail, $failedSteps;

    // 重置计数器
    $pass = 0;
    $fail = 0;
    $failedSteps = [];

    // 关键：在 xhjob_start 之前设置池模式环境变量，
    // daemon 进程会继承 PHP 进程的环境变量。
    putenv("XHJOB_POOL_MODE={$mode}");

    // 清理 + 启动
    @system("rm -rf $DATA_DIR");
    @mkdir($DATA_DIR, 0777, true);

    // ============================================================
    // 步骤 0：启动 daemon
    // ============================================================
    step(0, "启动 daemon (mode={$mode})", function () use ($SERVICE, $DATA_DIR) {
        $svc = new XhjobService($SERVICE, $DATA_DIR);
        $svc->ensureStopped();
        $pid = $svc->start();
        assertTrue($pid > 0, "pid={$pid} 应大于 0");
        $svc->wait(10, true);
    });

    $mgr = new TaskManager($SERVICE, $DATA_DIR);

    // ============================================================
    // 步骤 1：非阻塞派发（dispatch 立即返回 task_id）
    // ============================================================
    step(1, '非阻塞派发（立即返回 task_id）', function () use ($mgr) {
        $id = $mgr->create(TaskBuilder::shell('echo dispatch-test'));
        assertTrue(!empty($id) && strpos($id, 'error') === false, "dispatch 应返回有效 task_id，实际: {$id}");
        $ok = $mgr->waitForState($id, 'success', 10);
        assertTrue($ok, '任务应进入 success');
        echo "（task_id=" . substr($id, 0, 8) . "...）";
    });

    // ============================================================
    // 步骤 2：批量并发（5 个 shell 任务并发，全部 success）
    // ============================================================
    step(2, '批量并发（5 个任务，全部 success）', function () use ($mgr) {
        $ids = [];
        for ($i = 1; $i <= 5; $i++) {
            $ids[] = $mgr->create(TaskBuilder::shell("echo batch-{$i}"));
        }
        foreach ($ids as $i => $id) {
            $ok = $mgr->waitForState($id, 'success', 15);
            assertTrue($ok, 'batch 任务 #' . ($i + 1) . " ({$id}) 应进入 success");
        }
    });

    // ============================================================
    // 步骤 3：循环任务（every 2s，等 5 秒验证 execution_count>=2）
    // ============================================================
    step(3, '循环任务 every(2s) 验证 execution_count', function () use ($mgr) {
        $id = $mgr->create(
            TaskBuilder::shell('echo loop-tick')
                ->every(2)
                ->timeout(10)
        );
        // 等 5 秒，应至少触发 2 次（t=0 和 t=2，可能含 t=4）
        sleep(5);
        $st = $mgr->state($id);
        $ec = (int) ($st['execution_count'] ?? 0);
        assertTrue($ec >= 2, "execution_count 应>=2，实际 {$ec}");
        $mgr->stop($id);
        sleep(1);
        echo "（execution_count={$ec}）";
    });

    // ============================================================
    // 步骤 4：重试（exit 1 + withRetry(2,1)，验证 attempts>=3）
    // ============================================================
    step(4, '重试 withRetry(2,1)（exit 1 → attempts>=3）', function () use ($mgr) {
        $id = $mgr->create(
            TaskBuilder::shell('exit 1')
                ->withRetry(2, 1)
        );
        $ok = $mgr->waitForState($id, 'failed', 15);
        assertTrue($ok, '重试耗尽后应进入 failed');
        $st = $mgr->state($id);
        $attempts = (int) ($st['attempts'] ?? 0);
        assertTrue($attempts >= 3, "attempts 应>=3（1 初次 + 2 重试），实际 {$attempts}");
        echo "（attempts={$attempts}）";
    });

    // ============================================================
    // 步骤 5：超时（sleep 10 + timeout 2，验证 state=failed）
    // ============================================================
    step(5, '超时 timeout(2)（sleep 10 → failed）', function () use ($mgr) {
        $id = $mgr->create(
            TaskBuilder::shell('sleep 10')
                ->timeout(2)
        );
        // 等 4 秒让超时触发
        $ok = $mgr->waitForState($id, 'failed', 10);
        assertTrue($ok, '超时后应进入 failed');
        $st = $mgr->state($id);
        assertEq($st['state'] ?? '', 'failed', '超时后 state 应为 failed');
    });

    // ============================================================
    // 步骤 6：chain（2 步链式，验证 current_step=2）
    // ============================================================
    step(6, 'chain 链式任务（2 步 → current_step=2）', function () use ($mgr) {
        $chainId = $mgr->createChain([
            TaskBuilder::shell('echo chain-step1'),
            TaskBuilder::shell('echo chain-step2'),
        ]);
        assertTrue(!empty($chainId) && strpos($chainId, 'error') === false, "chain 应创建成功: {$chainId}");
        $final = null;
        $deadline = time() + 15;
        while (time() < $deadline) {
            $st = $mgr->chainState($chainId);
            if ($st && ($st['state'] ?? '') === 'success') {
                $final = $st;
                break;
            }
            usleep(300000);
        }
        assertTrue($final !== null, 'chain 应进入 success，实际: ' . json_encode($st ?? null));
        assertEq((int) ($final['current_step'] ?? 0), 2, 'chain 完成后 current_step 应=2');
    });

    // ============================================================
    // 步骤 7：group（2 任务并行，验证 state=success）
    // ============================================================
    step(7, 'group 组任务（2 任务并行 → success）', function () use ($mgr) {
        $groupId = $mgr->createGroup([
            TaskBuilder::shell('echo group-a'),
            TaskBuilder::shell('echo group-b'),
        ]);
        assertTrue(!empty($groupId) && strpos($groupId, 'error') === false, "group 应创建成功: {$groupId}");
        $final = null;
        $deadline = time() + 15;
        while (time() < $deadline) {
            $st = $mgr->groupState($groupId);
            if ($st && ($st['state'] ?? '') === 'success') {
                $final = $st;
                break;
            }
            usleep(300000);
        }
        assertTrue($final !== null, 'group 应进入 success，实际: ' . json_encode($st ?? null));
    });

    // ============================================================
    // 步骤 8：inspect stats（验证返回统计含 total/success）
    // ============================================================
    step(8, 'inspect stats（验证返回统计）', function () use ($mgr) {
        $stats = $mgr->inspect('stats');
        assertTrue(is_array($stats), 'inspect stats 应返回数组');
        assertTrue(isset($stats['total']), 'stats 应含 total 字段');
        assertTrue(isset($stats['success']), 'stats 应含 success 字段');
        assertTrue(($stats['total'] ?? 0) > 0, "total 应>0，实际 {$stats['total']}");
        echo "（total={$stats['total']}, success={$stats['success']}）";
    });

    // ============================================================
    // 步骤 9：停止 daemon
    // ============================================================
    step(9, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
        $svc = new XhjobService($SERVICE, $DATA_DIR);
        $svc->stop();
        $svc->wait(10, false);
        $st = $svc->status();
        assertTrue(!$st['running'], 'daemon 应已停止');
    });

    return [$pass, $fail, $failedSteps];
}

echo "=== 两种池模式对比测试 ===\n";
echo "service={$SERVICE} data_dir={$DATA_DIR}\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n\n";

// async 模式
echo "--- async 模式（async）---\n";
[$corPass, $corFail, $corFailed] = runTests('async');

// 线程池模式
echo "\n--- 多线程池模式（thread）---\n";
[$thrPass, $thrFail, $thrFailed] = runTests('thread');

// 汇总
echo "\n=== 汇总 ===\n";
echo "async 池: {$corPass} passed, {$corFail} failed";
if (!empty($corFailed)) {
    echo " (失败步骤: [" . implode(', ', $corFailed) . "])";
}
echo "\n";
echo "线程池: {$thrPass} passed, {$thrFail} failed";
if (!empty($thrFailed)) {
    echo " (失败步骤: [" . implode(', ', $thrFailed) . "])";
}
echo "\n";

// 对比结论
echo "\n--- 对比结论 ---\n";
if ($corFail === 0 && $thrFail === 0) {
    echo "两种池模式均通过全部测试，行为一致。\n";
} else {
    if ($corFail > 0) {
        echo "async 模式存在失败步骤: [" . implode(', ', $corFailed) . "]\n";
    }
    if ($thrFail > 0) {
        echo "线程池模式存在失败步骤: [" . implode(', ', $thrFailed) . "]\n";
    }
}

exit(($corFail + $thrFail) > 0 ? 1 : 0);
