<?php
/**
 * 跨环境一致性测试
 *
 * 模拟 PHP-FPM 进程启动 daemon（XhjobService），CLI 子进程通过 Client 派发任务，
 * FPM 父进程用 TaskManager 查询并验证两者看到的状态一致。
 *
 * 通过 proc_open / shell_exec 启动独立 PHP 子进程，确保 Client 与 TaskManager
 * 在不同进程地址空间内与同一个 daemon 通信。
 */
require_once __DIR__ . '/../../framework/autoload.php';

$DATA_DIR = '/tmp/xhjob-fw-cross-test';
$SERVICE = 'fw-cross';
$EXT = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
$PHP_BIN = PHP_BINARY;

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

/**
 * 在独立 PHP 子进程中执行一段代码。
 *
 * @param string $code PHP 代码（不含 <?php 标签和 autoload）
 * @return string 子进程 stdout
 */
function runChild(string $code): string {
    global $EXT, $PHP_BIN;
    $tmp = tempnam(sys_get_temp_dir(), 'xhjob_child_') . '.php';
    $bootstrap = '<?php require_once "/workspace/framework/autoload.php"; ';
    file_put_contents($tmp, $bootstrap . $code);
    $cmd = escapeshellarg($PHP_BIN) . ' -d extension=' . escapeshellarg($EXT) . ' ' . escapeshellarg($tmp) . ' 2>&1';
    $out = shell_exec($cmd);
    @unlink($tmp);
    return $out ?? '';
}

/**
 * 生成子进程代码字符串（使用 sprintf 安全插入参数，避免转义问题）。
 */
function childDispatchCode(string $service, string $dataDir, string $marker): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $id = $c->dispatch(TaskBuilder::shell(%s)->withRetry(0, 0)); echo $id;',
        var_export($service, true),
        var_export($dataDir, true),
        var_export('echo ' . $marker, true)
    );
}

function childResultCode(string $service, string $dataDir, string $id): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $r = $c->result(%s); echo json_encode($r);',
        var_export($service, true),
        var_export($dataDir, true),
        var_export($id, true)
    );
}

function childListCode(string $service, string $dataDir): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $list = $c->list(); $ids = array_column($list, "id"); echo implode(",", $ids);',
        var_export($service, true),
        var_export($dataDir, true)
    );
}

function childCancelCode(string $service, string $dataDir, string $id): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $ok = $c->cancel(%s); echo $ok ? "OK" : "FAIL";',
        var_export($service, true),
        var_export($dataDir, true),
        var_export($id, true)
    );
}

function childRequeueCode(string $service, string $dataDir, string $id): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $ok = $c->requeue(%s); echo $ok ? "OK" : "FAIL";',
        var_export($service, true),
        var_export($dataDir, true),
        var_export($id, true)
    );
}

function childEventsCode(string $service, string $dataDir, string $id): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $ev = $c->events(0, %s); echo json_encode($ev);',
        var_export($service, true),
        var_export($dataDir, true),
        var_export($id, true)
    );
}

function childGetCode(string $service, string $dataDir, string $id): string {
    return sprintf(
        '$c = new Client(%s, %s, 10); $g = $c->get(%s); echo json_encode($g);',
        var_export($service, true),
        var_export($dataDir, true),
        var_export($id, true)
    );
}

/**
 * 等待任务达到指定状态。返回最终状态。
 */
function waitForState(TaskManager $mgr, string $id, string $expected, int $timeout = 15): string {
    $deadline = time() + $timeout;
    $last = 'UNKNOWN';
    while (time() < $deadline) {
        $st = $mgr->state($id);
        $last = $st['state'] ?? 'UNKNOWN';
        if ($last === $expected) return $last;
        if (in_array($last, ['failed', 'cancelled'])) return $last;
        usleep(200000);
    }
    return $last;
}

// 11.1 FPM 进程启动 daemon
step('11.1', 'FPM 进程启动 daemon', function() use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    if ($pid <= 0) throw new Exception("pid=$pid");
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    if (!$h['healthy']) throw new Exception("not healthy: " . json_encode($h));
});

// 11.2 CLI 子进程 Client dispatch，FPM 父进程 TaskManager 查询一致
step('11.2', 'CLI 子进程 dispatch + FPM 父进程查询一致', function() use ($SERVICE, $DATA_DIR) {
    $marker = 'cross-env-' . uniqid();
    $id = trim(runChild(childDispatchCode($SERVICE, $DATA_DIR, $marker)));
    if (!$id || strpos($id, 'error') !== false || strpos($id, 'Error') !== false) {
        throw new Exception("child dispatch failed: " . $id);
    }
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $st = $mgr->state($id);
    if (empty($st)) throw new Exception("parent cannot see task: " . json_encode($st));
    $final = waitForState($mgr, $id, 'success', 15);
    if ($final !== 'success') throw new Exception("not SUCCESS: $final, state=" . json_encode($mgr->state($id)));
    $r = $mgr->result($id);
    if (strpos($r['stdout'] ?? '', $marker) === false) {
        throw new Exception("stdout mismatch: " . json_encode($r));
    }
});

// 11.3 FPM 父进程 dispatch，CLI 子进程 Client 查询一致
step('11.3', 'FPM 父进程 dispatch + CLI 子进程查询一致', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $marker = 'parent-dispatch-' . uniqid();
    $id = $mgr->create(TaskBuilder::shell('echo ' . $marker)->withRetry(0, 0));
    if (!$id) throw new Exception("parent dispatch failed: $id");
    $final = waitForState($mgr, $id, 'success', 15);
    if ($final !== 'success') throw new Exception("parent task not SUCCESS: $final");
    $out = trim(runChild(childResultCode($SERVICE, $DATA_DIR, $id)));
    $decoded = json_decode($out, true);
    if (!is_array($decoded)) throw new Exception("child result not json: $out");
    if (strpos($decoded['stdout'] ?? '', $marker) === false) {
        throw new Exception("child cannot see parent task result: $out");
    }
});

// 11.4 跨进程 list 一致性
step('11.4', '跨进程 list 一致性', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id1 = $mgr->create(TaskBuilder::shell('echo list-a')->withRetry(0, 0));
    $id2 = trim(runChild(childDispatchCode($SERVICE, $DATA_DIR, 'list-b')));
    if (!$id2) throw new Exception("child dispatch failed for list-b");
    $list = $mgr->list();
    $ids = array_column($list, 'id');
    if (!in_array($id1, $ids)) throw new Exception("id1 not in parent list: " . json_encode($ids));
    if (!in_array($id2, $ids)) throw new Exception("id2 not in parent list: " . json_encode($ids));
    $childIds = trim(runChild(childListCode($SERVICE, $DATA_DIR)));
    $childIdArr = explode(',', $childIds);
    if (!in_array($id1, $childIdArr)) throw new Exception("id1 not in child list: $childIds");
    if (!in_array($id2, $childIdArr)) throw new Exception("id2 not in child list: $childIds");
});

// 11.5 子进程 cancel，父进程查询状态一致
step('11.5', '子进程 cancel + 父进程查询状态一致', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('sleep 30')->withRetry(0, 0)->timeout(60));
    waitForState($mgr, $id, 'running', 10);
    $out = trim(runChild(childCancelCode($SERVICE, $DATA_DIR, $id)));
    if ($out !== 'OK') throw new Exception("child cancel failed: $out");
    $deadline = time() + 15;
    $last = 'UNKNOWN';
    while (time() < $deadline) {
        $st = $mgr->state($id);
        $last = $st['state'] ?? 'UNKNOWN';
        if (in_array($last, ['cancelled', 'failed', 'success'])) break;
        usleep(200000);
    }
    if (!in_array($last, ['cancelled', 'failed', 'success'])) {
        throw new Exception("task not terminal after child cancel: $last");
    }
});

// 11.6 子进程 requeue，父进程查询结果一致
step('11.6', '子进程 requeue + 父进程查询结果一致', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $marker = 'requeue-cross-' . uniqid();
    $id = $mgr->create(TaskBuilder::shell('echo ' . $marker)->withRetry(0, 0));
    waitForState($mgr, $id, 'success', 10);
    $out = trim(runChild(childRequeueCode($SERVICE, $DATA_DIR, $id)));
    if ($out !== 'OK') throw new Exception("child requeue failed: $out");
    $final = waitForState($mgr, $id, 'success', 15);
    if ($final !== 'success') throw new Exception("not SUCCESS after requeue: $final");
    $r = $mgr->result($id);
    if (strpos($r['stdout'] ?? '', $marker) === false) {
        throw new Exception("stdout mismatch after requeue: " . json_encode($r));
    }
});

// 11.7 子进程 events 查询，父进程 logs 查询一致
step('11.7', '子进程 events + 父进程 logs 一致', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo events-cross')->withRetry(0, 0));
    waitForState($mgr, $id, 'success', 10);
    $parentLogs = $mgr->logs($id);
    $out = trim(runChild(childEventsCode($SERVICE, $DATA_DIR, $id)));
    $childEvents = json_decode($out, true);
    if (!is_array($childEvents)) throw new Exception("child events not array: $out");
    if (count($parentLogs) < 1) throw new Exception("parent logs empty");
    if (count($childEvents) < 1) throw new Exception("child events empty");
    if (count($parentLogs) !== count($childEvents)) {
        throw new Exception("events count mismatch: parent=" . count($parentLogs) . " child=" . count($childEvents));
    }
});

// 11.8 子进程 get 任务定义，父进程 get 一致
step('11.8', '子进程 get + 父进程 get 一致', function() use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo get-cross')->withRetry(0, 0)->timeout(30));
    $parentGet = $mgr->get($id);
    $out = trim(runChild(childGetCode($SERVICE, $DATA_DIR, $id)));
    $childGet = json_decode($out, true);
    if (!is_array($childGet)) throw new Exception("child get not array: $out");
    if (!is_array($parentGet)) throw new Exception("parent get not array");
    if (($parentGet['task_type'] ?? '') !== ($childGet['task_type'] ?? '')) {
        throw new Exception("task_type mismatch: parent=" . json_encode($parentGet['task_type'] ?? null) . " child=" . json_encode($childGet['task_type'] ?? null));
    }
    if ((string)($parentGet['timeout'] ?? '') !== (string)($childGet['timeout'] ?? '')) {
        throw new Exception("timeout mismatch: parent=" . json_encode($parentGet['timeout'] ?? null) . " child=" . json_encode($childGet['timeout'] ?? null));
    }
});

// 11.9 多子进程并发 dispatch，父进程全部可见
step('11.9', '多子进程并发 dispatch + 父进程全部可见', function() use ($SERVICE, $DATA_DIR, $EXT, $PHP_BIN) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $childIds = [];
    $marker = 'concurrent-' . uniqid();
    $jobs = [];
    for ($i = 0; $i < 3; $i++) {
        $tmp = tempnam(sys_get_temp_dir(), 'xhjob_conc_') . '.php';
        $m = $marker . '-' . $i;
        $code = childDispatchCode($SERVICE, $DATA_DIR, $m);
        file_put_contents($tmp, '<?php require_once "/workspace/framework/autoload.php"; ' . $code);
        $jobs[] = $tmp;
    }
    $handles = [];
    $descriptors = [['pipe', 'r'], ['pipe', 'w'], ['pipe', 'w']];
    foreach ($jobs as $i => $tmp) {
        $cmd = escapeshellarg($PHP_BIN) . ' -d extension=' . escapeshellarg($EXT) . ' ' . escapeshellarg($tmp);
        $proc = proc_open($cmd, $descriptors, $pipes);
        if (!is_resource($proc)) {
            @unlink($tmp);
            throw new Exception("failed to spawn child $i");
        }
        fclose($pipes[0]);
        $handles[] = ['proc' => $proc, 'stdout' => $pipes[1], 'stderr' => $pipes[2], 'tmp' => $tmp];
    }
    foreach ($handles as $h) {
        $out = stream_get_contents($h['stdout']);
        fclose($h['stdout']);
        fclose($h['stderr']);
        proc_close($h['proc']);
        @unlink($h['tmp']);
        $id = trim($out);
        if ($id) $childIds[] = $id;
    }
    if (count($childIds) !== 3) throw new Exception("expected 3 child ids, got " . count($childIds));
    $list = $mgr->list();
    $ids = array_column($list, 'id');
    foreach ($childIds as $i => $id) {
        if (!in_array($id, $ids)) throw new Exception("child id $i ($id) not in parent list");
    }
    foreach ($childIds as $id) {
        $final = waitForState($mgr, $id, 'success', 15);
        if ($final !== 'success') throw new Exception("child id $id not SUCCESS: $final");
    }
});

// 11.10 FPM 父进程停止 daemon
step('11.10', 'FPM 父进程停止 daemon', function() use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    if ($st['running']) throw new Exception("still running");
    if (file_exists("$DATA_DIR/xhjob.$SERVICE.pid")) throw new Exception("pid file not cleaned");
});

echo "\n=== Results: $pass passed, $fail failed, $skipped skipped ===\n";
exit($fail > 0 ? 1 : 0);
