#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 CLI 测试脚本
// +----------------------------------------------------------------------
// | 直接在 CLI 中运行，不经过 HTTP / ThinkPHP 路由：
// |   EXT=/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so
// |   php -d extension=$EXT test_xhjob.php
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

// CLI 测试时手动加载 xhjob 扩展（已通过 -d extension 加载则跳过）
if (!extension_loaded('xhjob')) {
    $so = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;
use Xhjob\Client;

$pass = 0;
$fail = 0;
$skipped = 0;

/**
 * 测试步骤封装
 *
 * @param int       $n    步骤编号
 * @param string    $desc 描述
 * @param callable  $fn   测试闭包，返回 'SKIP' 表示跳过
 */
function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $skipped;
    echo "[$n] $desc ... ";
    try {
        $r = $fn();
        if ($r === 'SKIP') {
            $skipped++;
            echo "SKIP\n";
            return;
        }
        $pass++;
        echo "PASS\n";
    } catch (Throwable $e) {
        $fail++;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

$SERVICE  = 'tp-test';
$DATA_DIR = '/tmp/xhjob-tp-test';

// 清理上一次测试残留
@system("rm -f $DATA_DIR/xhjob.$SERVICE.*");

// -----------------------------------------------------------------
// 测试步骤
// -----------------------------------------------------------------

// 1. 启动 daemon
step(1, '启动 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    if ($pid <= 0) {
        throw new Exception("pid=$pid");
    }
    $svc->wait(10, true);
});

// 2. 健康检查
step(2, '健康检查', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $h = $svc->healthCheck();
    if (!$h['healthy']) {
        throw new Exception("not healthy: " . json_encode($h));
    }
});

// 3. 创建 shell 任务 + 验证结果
step(3, '创建 shell 任务 + 验证结果', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->create(TaskBuilder::shell('echo hello-tp')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 15);
    $r = $mgr->result($id);
    if (($r['stdout'] ?? '') !== "hello-tp\n") {
        throw new Exception("stdout mismatch: " . json_encode($r));
    }
});

// 4. 创建 cron 任务 + maxExecutions
step(4, '创建 cron 任务 + maxExecutions(2)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->create(
        TaskBuilder::shell('echo cron-tp')
            ->cron('* * * * *')
            ->maxExecutions(2)
            ->withRetry(0, 0)
    );
    // cron 每分钟触发一次，最多等 180s
    $start = time();
    while (time() - $start < 180) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(500000);
    }
    $st = $mgr->state($id);
    if (($st['state'] ?? '') !== 'success') {
        throw new Exception("cron not success: " . json_encode($st));
    }
});

// 5. list 查询
step(5, 'list 查询', function () use ($SERVICE, $DATA_DIR) {
    $mgr  = new TaskManager($SERVICE, $DATA_DIR);
    $list = $mgr->list();
    if (count($list) < 1) {
        throw new Exception("list empty");
    }
});

// 6. 创建 chain
step(6, '创建 chain', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->createChain([
        TaskBuilder::shell('echo c1'),
        TaskBuilder::shell('echo c2'),
    ]);
    if (!$id || strpos($id, 'error') !== false) {
        throw new Exception("chain failed: $id");
    }
});

// 7. 创建 group
step(7, '创建 group', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->createGroup([
        TaskBuilder::shell('echo g1'),
        TaskBuilder::shell('echo g2'),
    ]);
    if (!$id || strpos($id, 'error') !== false) {
        throw new Exception("group failed: $id");
    }
});

// 8. stop 任务
step(8, 'stop 任务（cancel）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->create(
        TaskBuilder::shell('sleep 30')
            ->withRetry(0, 0)
            ->timeout(60)
    );
    $mgr->waitForState($id, 'running', 10);
    $mgr->stop($id);
    $start = time();
    while (time() - $start < 15) {
        $st = $mgr->state($id);
        if (in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'])) {
            break;
        }
        usleep(500000);
    }
    $st = $mgr->state($id);
    if (!in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'])) {
        throw new Exception("not terminal: " . json_encode($st));
    }
});

// 9. restart 任务
step(9, 'restart 任务（requeue）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id  = $mgr->create(TaskBuilder::shell('echo restartable-tp')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $mgr->restart($id);
    $mgr->waitForState($id, 'success', 15);
});

// 10. logs 查询
step(10, 'logs 查询', function () use ($SERVICE, $DATA_DIR) {
    $mgr  = new TaskManager($SERVICE, $DATA_DIR);
    $id   = $mgr->create(TaskBuilder::shell('echo logged-tp')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $logs = $mgr->logs($id);
    if (count($logs) < 1) {
        throw new Exception("no logs");
    }
});

// 11. Client 跨环境
step(11, 'Client 跨环境', function () use ($SERVICE, $DATA_DIR) {
    $c  = new Client($SERVICE, $DATA_DIR);
    $id = $c->dispatch(TaskBuilder::shell('echo client-tp')->withRetry(0, 0));
    if (!$id || strpos($id, 'error') !== false) {
        throw new Exception("dispatch failed: $id");
    }
    $start = time();
    while (time() - $start < 15) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(500000);
    }
});

// 12. 停止 daemon
step(12, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    if ($st['running']) {
        throw new Exception("still running");
    }
});

echo "\n=== Results: $pass passed, $fail failed, $skipped skipped ===\n";
exit($fail > 0 ? 1 : 0);
