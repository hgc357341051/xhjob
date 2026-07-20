<?php
/**
 * php-fpm 业务场景模拟 - 客户端测试
 *
 * 模拟生产环境：多个独立 HTTP 请求（不同 PHP worker 进程）连接同一个 daemon。
 * 本脚本启动 PHP 内置 server（模拟 php-fpm worker 池），然后发起多个 HTTP 请求。
 *
 * 关键验证点：
 *   1. 启动请求（worker A）创建 daemon 后退出
 *   2. 后续请求（worker B/C/D）连接到同一个 daemon
 *   3. 每个请求看到的 worker_pid 不同（独立进程），但 daemon pid 相同
 *   4. 重启请求触发 daemon PID 变化
 *   5. 停止请求清理 daemon
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so client_test.php
 */

$projectRoot = realpath(__DIR__ . '/../../..');
$soPath = $projectRoot . '/target/release/libxhjob.so';
$handlerPath = __DIR__ . '/handler.php';

if (!file_exists($soPath)) {
    fwrite(STDERR, "ERROR: 扩展未编译: $soPath\n");
    exit(1);
}

// 启动 PHP 内置 server（模拟 php-fpm）
$serverCmd = sprintf(
    'exec %s -d extension=%s -S 127.0.0.1:18080 %s > /tmp/xhjob-fpm-server.log 2>&1 & echo $!',
    escapeshellarg(PHP_BINARY),
    escapeshellarg($soPath),
    escapeshellarg($handlerPath)
);

echo "[client] 启动 PHP 内置 server (模拟 php-fpm)...\n";
$serverPid = (int)trim(shell_exec($serverCmd));
echo "[client] server PID: {$serverPid}\n";

if ($serverPid <= 0) {
    fwrite(STDERR, "FAIL: 无法启动 server\n");
    exit(1);
}

// 等待 server 就绪
$serverReady = false;
for ($i = 0; $i < 30; $i++) {
    $fp = @fsockopen('127.0.0.1', 18080, $errno, $errstr, 1);
    if ($fp) {
        fclose($fp);
        $serverReady = true;
        break;
    }
    usleep(100000);
}
if (!$serverReady) {
    fwrite(STDERR, "FAIL: server 未就绪\n");
    echo shell_exec('cat /tmp/xhjob-fpm-server.log');
    posix_kill($serverPid, 9);
    exit(2);
}
echo "[client] server 就绪\n\n";

// 辅助函数：发起 HTTP 请求
function httpRequest(string $method, string $path, array $query = []): array {
    $url = 'http://127.0.0.1:18080' . $path;
    if ($query) {
        $url .= '?' . http_build_query($query);
    }
    $ch = curl_init($url);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    curl_setopt($ch, CURLOPT_CUSTOMREQUEST, $method);
    curl_setopt($ch, CURLOPT_TIMEOUT, 30);
    $body = curl_exec($ch);
    $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
    $err = curl_error($ch);
    curl_close($ch);
    if ($err) {
        return ['_error' => $err, '_code' => $code];
    }
    $json = json_decode($body, true);
    if ($json === null) {
        return ['_raw' => $body, '_code' => $code];
    }
    $json['_code'] = $code;
    return $json;
}

$failures = [];

function check(string $name, bool $cond, string $detail = ''): void {
    global $failures;
    if ($cond) {
        echo "  [PASS] {$name}\n";
    } else {
        echo "  [FAIL] {$name} {$detail}\n";
        $failures[] = $name;
    }
}

// ============================================================
// 步骤 1: 启动服务（HTTP 请求 1, worker A）
// ============================================================
echo "[step 1] GET /start (worker A 创建 daemon)\n";
$r = httpRequest('GET', '/start', ['service' => 'fpm-prod']);
echo "  HTTP {$r['_code']}, daemon pid=" . ($r['pid'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('start returns ok', ($r['ok'] ?? false) === true, json_encode($r));
check('daemon pid present', isset($r['pid']) && $r['pid'] > 0);
$daemonPid1 = $r['pid'] ?? 0;
$workerPidA = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 2: 查询状态（HTTP 请求 2, worker B）
// ============================================================
echo "\n[step 2] GET /status (worker B 查询状态)\n";
$r = httpRequest('GET', '/status', ['service' => 'fpm-prod']);
echo "  HTTP {$r['_code']}, running=" . ($r['running'] ? 'true' : 'false') . ", daemon pid=" . ($r['pid'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('status running', ($r['running'] ?? false) === true);
check('daemon pid same as step1', ($r['pid'] ?? 0) === $daemonPid1, "expected {$daemonPid1}, got " . ($r['pid'] ?? 'null'));
$workerPidB = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 3: 投递 shell 任务（HTTP 请求 3, worker C）
// ============================================================
echo "\n[step 3] GET /dispatch/shell (worker C 投递任务)\n";
$marker = 'fpm-biz-' . uniqid();
$r = httpRequest('GET', '/dispatch/shell', ['service' => 'fpm-prod', 'marker' => $marker]);
echo "  HTTP {$r['_code']}, task_id=" . ($r['task_id'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('dispatch returns task_id', !empty($r['task_id']) && strlen($r['task_id']) > 10);
$taskId = $r['task_id'] ?? '';
$workerPidC = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 4: 轮询任务状态（HTTP 请求 4, worker D）
// ============================================================
echo "\n[step 4] GET /state (worker D 轮询任务状态)\n";
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $r = httpRequest('GET', '/state', ['service' => 'fpm-prod', 'id' => $taskId]);
    $state = $r['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}
echo "  最终状态: {$finalState}, worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('task reaches SUCCESS', $finalState === 'SUCCESS', "got {$finalState}");
$workerPidD = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 5: 查询结果（HTTP 请求 5, worker E）
// ============================================================
echo "\n[step 5] GET /result (worker E 查询结果)\n";
$r = httpRequest('GET', '/result', ['service' => 'fpm-prod', 'id' => $taskId]);
echo "  HTTP {$r['_code']}, exit_code=" . ($r['exit_code'] ?? 'null') . ", stdout=" . trim($r['stdout'] ?? '') . "\n";
check('exit_code is 0', ($r['exit_code'] ?? -1) === 0);
check('stdout contains marker', strpos($r['stdout'] ?? '', $marker) !== false, "expected marker {$marker}");
$workerPidE = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 6: 验证 worker 进程独立性
// ============================================================
echo "\n[step 6] 验证 worker 进程独立性\n";
$workerPids = [$workerPidA, $workerPidB, $workerPidC, $workerPidD, $workerPidE];
$uniqueWorkers = count(array_unique(array_filter($workerPids)));
echo "  worker PIDs: " . implode(', ', $workerPids) . "\n";
echo "  唯一 worker 数: {$uniqueWorkers}\n";
check('multiple distinct worker PIDs', $uniqueWorkers >= 2, "only {$uniqueWorkers} unique");
echo "  daemon PID 全程: {$daemonPid1}（不变）\n";
check('daemon PID stable across requests', $daemonPid1 > 0);

// ============================================================
// 步骤 7: 并发请求测试（多个 worker 同时投递任务）
// ============================================================
echo "\n[step 7] 并发投递 3 个任务（模拟并发 HTTP 请求）\n";
$mh = curl_multi_init();
$handles = [];
$markers = [];
for ($i = 0; $i < 3; $i++) {
    $m = "concurrent-{$i}-" . uniqid();
    $markers[] = $m;
    $url = 'http://127.0.0.1:18080/dispatch/shell?service=fpm-prod&marker=' . urlencode($m);
    $ch = curl_init($url);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    curl_multi_add_handle($mh, $ch);
    $handles[] = $ch;
}
do {
    $status = curl_multi_exec($mh, $active);
    if ($active) {
        curl_multi_select($mh);
    }
} while ($active && $status === CURLM_OK);

$concurrentIds = [];
foreach ($handles as $i => $ch) {
    $body = curl_multi_getcontent($ch);
    $json = json_decode($body, true);
    $concurrentIds[] = $json['task_id'] ?? '';
    echo "  并发任务 {$i}: id=" . ($json['task_id'] ?? 'null') . ", worker=" . ($json['worker_pid'] ?? 'null') . "\n";
    curl_multi_remove_handle($mh, $ch);
}
curl_multi_close($mh);

// 等待全部完成
echo "  等待并发任务完成...\n";
$successCount = 0;
foreach ($concurrentIds as $i => $id) {
    for ($j = 0; $j < 100; $j++) {
        $r = httpRequest('GET', '/state', ['service' => 'fpm-prod', 'id' => $id]);
        $state = $r['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            if ($state === 'SUCCESS') $successCount++;
            break;
        }
        usleep(100000);
    }
}
echo "  并发任务成功: {$successCount}/3\n";
check('all concurrent tasks succeed', $successCount === 3, "got {$successCount}/3");

// ============================================================
// 步骤 8: 重启服务（HTTP 请求, worker F）
// ============================================================
echo "\n[step 8] POST /restart (worker F 触发重启)\n";
$r = httpRequest('POST', '/restart', ['service' => 'fpm-prod']);
echo "  HTTP {$r['_code']}, pid_before=" . ($r['pid_before'] ?? 'null') . ", pid_after=" . ($r['pid_after'] ?? 'null') . "\n";
check('restart returns ok', ($r['ok'] ?? false) === true);
check('pid changed after restart', ($r['pid_changed'] ?? false) === true, json_encode($r));
$daemonPid2 = $r['pid_after'] ?? 0;

// ============================================================
// 步骤 9: 重启后验证新 daemon 可服务（HTTP 请求, worker G）
// ============================================================
echo "\n[step 9] 重启后投递新任务 (worker G)\n";
$marker2 = 'after-restart-' . uniqid();
$r = httpRequest('GET', '/dispatch/shell', ['service' => 'fpm-prod', 'marker' => $marker2]);
$taskId2 = $r['task_id'] ?? '';
echo "  新任务 id={$taskId2}, worker=" . ($r['worker_pid'] ?? 'null') . "\n";
check('dispatch after restart succeeds', !empty($taskId2) && strlen($taskId2) > 10);

// 等待完成
for ($i = 0; $i < 100; $i++) {
    $r = httpRequest('GET', '/state', ['service' => 'fpm-prod', 'id' => $taskId2]);
    $state = $r['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') break;
    usleep(100000);
}
check('task after restart reaches SUCCESS', $state === 'SUCCESS', "got {$state}");

// ============================================================
// 步骤 10: 停止服务（HTTP 请求, worker H）
// ============================================================
echo "\n[step 10] DELETE /stop (worker H 停止 daemon)\n";
$r = httpRequest('DELETE', '/stop', ['service' => 'fpm-prod']);
echo "  HTTP {$r['_code']}, ok=" . ($r['ok'] ? 'true' : 'false') . ", running=" . ($r['running'] ? 'true' : 'false') . "\n";
check('stop returns ok', ($r['ok'] ?? false) === true);
check('daemon stopped after stop', ($r['running'] ?? true) === false);

// 验证状态
$r = httpRequest('GET', '/status', ['service' => 'fpm-prod']);
check('status running=false after stop', ($r['running'] ?? true) === false);

// ============================================================
// 清理：停止 PHP 内置 server
// ============================================================
echo "\n[cleanup] 停止 PHP 内置 server (PID={$serverPid})\n";
posix_kill($serverPid, 15);
usleep(500000);
if (posix_kill($serverPid, 0)) {
    posix_kill($serverPid, 9);
    echo "  server 强制终止\n";
} else {
    echo "  server 已正常退出\n";
}
@unlink('/tmp/xhjob-fpm-server.log');

// ============================================================
// 汇总
// ============================================================
echo "\n============================================================\n";
if (empty($failures)) {
    echo "全部 php-fpm 业务场景测试通过\n";
    echo "============================================================\n";
    exit(0);
} else {
    echo "FAIL: " . count($failures) . " 项测试失败:\n";
    foreach ($failures as $f) {
        echo "  - {$f}\n";
    }
    echo "============================================================\n";
    exit(1);
}
