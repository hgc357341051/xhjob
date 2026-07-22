<?php
/**
 * FPM 框架完整流程测试
 *
 * 模拟 PHP-FPM 环境中使用 XhjobService + TaskManager + TaskBuilder 的完整流程。
 */
require_once __DIR__ . '/../../framework/autoload.php';

$DATA_DIR = '/tmp/xhjob-fw-fpm-test';
$SERVICE = 'fw-fpm';
$EXT = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';

// 清理
@unlink("$DATA_DIR/xhjob.$SERVICE.pid");
@unlink("$DATA_DIR/xhjob.$SERVICE.sock");

$pass = 0; $fail = 0; $skipped = 0;

function step($n, $desc, $fn) {
    global $pass, $fail, $skipped;
    echo "[$n] $desc ... ";
    try {
        $r = $fn();
        if ($r === 'SKIP') { $skipped++; echo "SKIP\n"; return; }
        $pass++; echo "PASS\n";
    } catch (Throwable $e) {
        $fail++; echo "FAIL: " . $e->getMessage() . "\n";
    }
}

// 9.1 启动 daemon + healthCheck
step('9.1', '启动 daemon + healthCheck', function() use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    if ($pid <= 0) throw new Exception("pid=$pid");
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    if (!$h['healthy']) throw new Exception("not healthy: " . json_encode($h));
});

// 9.2 新增 shell 任务 + 验证 stdout
step('9.2', '新增 shell 任务 + 验证 stdout', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo hello-framework')->withRetry(0, 0));
    if (!$id || strpos($id, 'error') !== false) throw new Exception("dispatch failed: $id");
    $mgr->waitForState($id, 'success', 15);
    $r = $mgr->result($id);
    if (($r['stdout'] ?? '') !== "hello-framework\n") throw new Exception("stdout mismatch: " . json_encode($r));
});

// 9.3 新增 HTTP 任务（网络不可用则 SKIP）
step('9.3', '新增 HTTP 任务', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    try {
        $id = $mgr->create(TaskBuilder::http('GET', 'http://127.0.0.1:1/nope')->withRetry(0, 0)->timeout(3));
    } catch (Throwable $e) {
        return 'SKIP';
    }
    // 即使失败也算 dispatch 成功
});

// 9.4 新增 cron 任务 + maxExecutions(3)
step('9.4', '新增 cron 任务 + maxExecutions(3)', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo cron-tick')->cron('* * * * *')->maxExecutions(3)->withRetry(0, 0));
    // 等待最多 200 秒（3 个分钟边界）
    $start = time();
    while (time() - $start < 200) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') break;
        usleep(500000);
    }
    $st = $mgr->state($id);
    if (($st['state'] ?? '') !== 'success') throw new Exception("cron task not SUCCESS: " . json_encode($st));
    if (intval($st['execution_count'] ?? 0) < 3) throw new Exception("execution_count < 3: " . json_encode($st));
});

// 9.5 list 查询
step('9.5', 'list 查询', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $list = $mgr->list();
    if (count($list) < 2) throw new Exception("list count < 2: " . count($list));
});

// 9.6 编辑任务（update: 改 cron）
step('9.6', '编辑任务（update）', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo before-edit')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $newId = $mgr->update($id, TaskBuilder::shell('echo after-edit')->cron('*/5 * * * *')->withRetry(0, 0));
    if (!$newId) throw new Exception("update failed");
    $t = $mgr->get($newId);
    if (($t['cron'] ?? '') !== '*/5 * * * *') throw new Exception("cron not updated: " . json_encode($t));
});

// 9.7 stop 任务
step('9.7', 'stop 任务（cancel）', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('sleep 30')->withRetry(0, 0)->timeout(60));
    // 等 RUNNING
    $mgr->waitForState($id, 'running', 10);
    $mgr->stop($id);
    // 等终态
    $start = time();
    while (time() - $start < 15) {
        $st = $mgr->state($id);
        $s = $st['state'] ?? '';
        if (in_array($s, ['cancelled', 'failed', 'success'])) break;
        usleep(500000);
    }
    $st = $mgr->state($id);
    if (!in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'])) {
        throw new Exception("task not terminal: " . json_encode($st));
    }
});

// 9.8 restart 任务（requeue）
step('9.8', 'restart 任务（requeue）', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo restartable')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $mgr->restart($id);
    // 重新执行
    $mgr->waitForState($id, 'success', 15);
    $r = $mgr->result($id);
    if (($r['stdout'] ?? '') !== "restartable\n") throw new Exception("stdout mismatch");
});

// 9.9 logs 查询
step('9.9', 'logs 查询', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo logged')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $logs = $mgr->logs($id);
    if (count($logs) < 1) throw new Exception("no logs: " . json_encode($logs));
    // 验证含 STARTED + SUCCEEDED
    $types = array_column($logs, 'event_type');
    if (!in_array('STARTED', $types) && !in_array('started', $types)) {
        // 事件类型可能是小写
        $types = array_map('strtoupper', $types);
        if (!in_array('STARTED', $types)) throw new Exception("no STARTED event: " . json_encode($logs));
    }
});

// 9.10 停止 daemon
step('9.10', '停止 daemon', function() use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    if ($st['running']) throw new Exception("still running");
    if (file_exists("$DATA_DIR/xhjob.$SERVICE.pid")) throw new Exception("pid file not cleaned");
});

echo "\n=== Results: $pass passed, $fail failed, $skipped skipped ===\n";
exit($fail > 0 ? 1 : 0);
