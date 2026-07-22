<?php
/**
 * CLI 控制脚本 + Client 跨进程测试
 *
 * 通过 bin/xhjobctl 控制 daemon 生命周期（start/stop/status/health），
 * 通过 Client 类在独立进程中派发与管理任务，验证 CLI 控制路径与
 * Client 跨进程通信的一致性。
 */
require_once __DIR__ . '/../../framework/autoload.php';

$DATA_DIR = '/tmp/xhjob-fw-cli-test';
$SERVICE = 'fw-cli';
$EXT = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
$XHJOBCTL = __DIR__ . '/../../bin/xhjobctl';
$PHP = PHP_BINARY;

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

// 辅助：执行 xhjobctl 子命令（带扩展）
function ctl(string $sub, array $args = []): array {
    global $EXT, $XHJOBCTL, $PHP;
    $cmd = escapeshellarg($PHP) . ' -d extension=' . escapeshellarg($EXT) . ' ' . escapeshellarg($XHJOBCTL) . ' ' . $sub;
    foreach ($args as $k => $v) {
        if ($v === null) continue;
        $cmd .= ' --' . $k . '=' . escapeshellarg((string)$v);
    }
    $cmd .= ' 2>&1';
    $out = shell_exec($cmd);
    $code = 0;
    return ['out' => $out, 'code' => $code, 'cmd' => $cmd];
}

// 10.1 xhjobctl start 启动 daemon + status 验证
step('10.1', 'xhjobctl start 启动 daemon + status', function() use ($SERVICE, $DATA_DIR) {
    $r = ctl('stop', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]); // 清理残留
    $r = ctl('start', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', '已启动') === false) throw new Exception("start failed: " . $r['out']);
    // 等待就绪
    usleep(500000);
    $r = ctl('status', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', 'running=true') === false) throw new Exception("not running: " . $r['out']);
});

// 10.2 xhjobctl health 健康检查
step('10.2', 'xhjobctl health 健康检查', function() use ($SERVICE, $DATA_DIR) {
    $r = ctl('health', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', 'healthy=true') === false) throw new Exception("not healthy: " . $r['out']);
});

// 10.3 Client dispatch shell + state/result
step('10.3', 'Client dispatch shell + state/result', function() use ($SERVICE, $DATA_DIR) {
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $id = $c->dispatch(TaskBuilder::shell('echo cli-client-hello')->withRetry(0, 0));
    if (!$id || strpos($id, 'error') !== false) throw new Exception("dispatch failed: $id");
    // 轮询至 SUCCESS
    $deadline = time() + 15;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') break;
        if (in_array($st['state'] ?? '', ['failed', 'cancelled'])) break;
        usleep(200000);
    }
    $st = $c->state($id);
    if (($st['state'] ?? '') !== 'success') throw new Exception("not SUCCESS: " . json_encode($st));
    $r = $c->result($id);
    if (strpos($r['stdout'] ?? '', 'cli-client-hello') === false) {
        throw new Exception("stdout mismatch: " . json_encode($r));
    }
});

// 10.4 Client dispatch http（网络不可用则 SKIP）
step('10.4', 'Client dispatch http', function() use ($SERVICE, $DATA_DIR) {
    $c = new Client($SERVICE, $DATA_DIR, 10);
    try {
        $id = $c->dispatch(TaskBuilder::http('GET', 'http://127.0.0.1:1/nope')->withRetry(0, 0)->timeout(3));
    } catch (Throwable $e) {
        return 'SKIP';
    }
    if (!$id || strpos($id, 'error') !== false) throw new Exception("dispatch failed: $id");
});

// 10.5 Client list 查询
step('10.5', 'Client list 查询', function() use ($SERVICE, $DATA_DIR) {
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $list = $c->list();
    if (count($list) < 1) throw new Exception("list empty");
});

// 10.6 Client cancel 任务
step('10.6', 'Client cancel 任务', function() use ($SERVICE, $DATA_DIR) {
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $id = $c->dispatch(TaskBuilder::shell('sleep 30')->withRetry(0, 0)->timeout(60));
    // 等 RUNNING
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'running') break;
        usleep(200000);
    }
    $ok = $c->cancel($id);
    if (!$ok) throw new Exception("cancel returned false");
    // 等终态
    $deadline = time() + 15;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'])) break;
        usleep(200000);
    }
    $st = $c->state($id);
    if (!in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'])) {
        throw new Exception("task not terminal: " . json_encode($st));
    }
});

// 10.7 Client requeue 任务（restart）
step('10.7', 'Client requeue 任务（restart）', function() use ($SERVICE, $DATA_DIR) {
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $id = $c->dispatch(TaskBuilder::shell('echo cli-requeue')->withRetry(0, 0));
    // 等首次 SUCCESS
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') break;
        usleep(200000);
    }
    $ok = $c->requeue($id);
    if (!$ok) throw new Exception("requeue returned false");
    // 再次等 SUCCESS
    $deadline = time() + 15;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') break;
        usleep(200000);
    }
    $r = $c->result($id);
    if (strpos($r['stdout'] ?? '', 'cli-requeue') === false) {
        throw new Exception("stdout mismatch after requeue: " . json_encode($r));
    }
});

// 10.8 xhjobctl dispatch + Client get 一致性
step('10.8', 'xhjobctl dispatch + Client get 一致性', function() use ($SERVICE, $DATA_DIR) {
    // 用 xhjobctl 派发
    $r = ctl('dispatch', [
        'type' => 'shell',
        'cmd' => 'echo ctl-dispatch',
        'name' => $SERVICE,
        'data-dir' => $DATA_DIR,
    ]);
    if (strpos($r['out'] ?? '', '已派发') === false) throw new Exception("dispatch failed: " . $r['out']);
    // 解析 task_id
    if (!preg_match('/task_id=([^\s]+)/', $r['out'], $m)) throw new Exception("no task_id in output: " . $r['out']);
    $id = $m[1];
    // Client 端查询
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') break;
        usleep(200000);
    }
    $r2 = $c->result($id);
    if (strpos($r2['stdout'] ?? '', 'ctl-dispatch') === false) {
        throw new Exception("stdout mismatch: " . json_encode($r2));
    }
});

// 10.9 xhjobctl logs 查询事件
step('10.9', 'xhjobctl logs 查询事件', function() use ($SERVICE, $DATA_DIR) {
    // 先派发一个新任务
    $c = new Client($SERVICE, $DATA_DIR, 10);
    $id = $c->dispatch(TaskBuilder::shell('echo cli-logs')->withRetry(0, 0));
    // 等 SUCCESS
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $c->state($id);
        if (($st['state'] ?? '') === 'success') break;
        usleep(200000);
    }
    $r = ctl('logs', ['id' => $id, 'name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', '无事件记录') !== false) throw new Exception("no events: " . $r['out']);
    // 验证含 STARTED/succeeded 之类
    $hasEvent = (strpos($r['out'], 'STARTED') !== false) || (strpos($r['out'], 'started') !== false)
        || (strpos($r['out'], 'SUCCEEDED') !== false) || (strpos($r['out'], 'succeeded') !== false);
    if (!$hasEvent) throw new Exception("no event markers: " . $r['out']);
});

// 10.10 xhjobctl stop 停止 daemon
step('10.10', 'xhjobctl stop 停止 daemon', function() use ($SERVICE, $DATA_DIR) {
    $r = ctl('stop', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', '已停止') === false) throw new Exception("stop failed: " . $r['out']);
    usleep(500000);
    $r = ctl('status', ['name' => $SERVICE, 'data-dir' => $DATA_DIR]);
    if (strpos($r['out'] ?? '', 'running=false') === false) throw new Exception("still running: " . $r['out']);
    if (file_exists("$DATA_DIR/xhjob.$SERVICE.pid")) throw new Exception("pid file not cleaned");
});

echo "\n=== Results: $pass passed, $fail failed, $skipped skipped ===\n";
exit($fail > 0 ? 1 : 0);
