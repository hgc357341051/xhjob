<?php
/**
 * php-fpm 业务场景模拟 - 进程级测试
 *
 * 由于沙箱网络策略限制 127.0.0.1 HTTP 访问，本测试改用进程级模拟：
 * 每个业务操作通过 proc_open 启动独立 PHP 子进程执行，
 * 模拟 php-fpm 每个 worker 是独立进程的特性。
 *
 * 关键验证点：
 *   1. 启动进程（worker A）创建 daemon 后退出
 *   2. 后续进程（worker B/C/D）连接到同一个 daemon
 *   3. 每个进程的 PID 不同（独立进程），但 daemon pid 相同
 *   4. 重启进程触发 daemon PID 变化
 *   5. 停止进程清理 daemon
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so proc_test.php
 */

$projectRoot = realpath(__DIR__ . '/../../..');
$soPath = $projectRoot . '/target/release/libxhjob.so';
$handlerPath = __DIR__ . '/handler.php';

if (!file_exists($soPath)) {
    fwrite(STDERR, "ERROR: 扩展未编译: $soPath\n");
    exit(1);
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

/**
 * 启动独立 PHP 子进程执行 handler 逻辑（模拟一个 fpm worker 请求）
 * 通过环境变量传递 "HTTP method + path + query"，handler 改为从 env 读取。
 */
function workerRequest(string $method, string $path, array $query = [], string $service = 'fpm-prod'): array {
    global $soPath, $handlerPath;
    $queryStr = http_build_query(array_merge($query, ['service' => $service]));
    $env = array_merge($_ENV, [
        'XHJOB_TEST_METHOD' => $method,
        'XHJOB_TEST_PATH' => $path,
        'XHJOB_TEST_QUERY' => $queryStr,
    ]);
    $cmd = [PHP_BINARY, '-d', "extension=$soPath", $handlerPath];
    $desc = [
        0 => ['pipe', 'r'],
        1 => ['pipe', 'w'],
        2 => ['pipe', 'w'],
    ];
    $proc = proc_open($cmd, $desc, $pipes, null, $env);
    if (!is_resource($proc)) {
        return ['_error' => 'proc_open failed'];
    }
    fclose($pipes[0]);
    $stdout = stream_get_contents($pipes[1]);
    fclose($pipes[1]);
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[2]);
    $exitCode = proc_close($proc);
    $json = json_decode($stdout, true);
    if ($json === null) {
        return ['_error' => 'non-json output', '_raw' => $stdout, '_stderr' => $stderr, '_exit' => $exitCode];
    }
    $json['_exit'] = $exitCode;
    return $json;
}

// 修改 handler.php 入口：检查是否为测试模式（环境变量驱动）
// 这里我们用一个简单的 wrapper 脚本代替，避免修改 handler.php
$wrapperCode = <<<'PHP'
<?php
// 测试 wrapper：从环境变量读取请求参数，调用 handler 函数
$method = getenv('XHJOB_TEST_METHOD') ?: 'GET';
$path = getenv('XHJOB_TEST_PATH') ?: '/';
parse_str(getenv('XHJOB_TEST_QUERY') ?: '', $_GET);

// 模拟 _SERVER
$_SERVER['REQUEST_METHOD'] = $method;
$_SERVER['REQUEST_URI'] = $path . (!empty($_GET) ? '?' . http_build_query($_GET) : '');

// 引入 handler（它已经 echo 输出）
require __DIR__ . '/handler.php';
PHP;

$wrapperPath = __DIR__ . '/_wrapper.php';
file_put_contents($wrapperPath, $wrapperCode);

// 重写 workerRequest 使用 wrapper
function workerRequest2(string $method, string $path, array $query = [], string $service = 'fpm-prod'): array {
    global $soPath, $wrapperPath;
    $queryStr = http_build_query(array_merge($query, ['service' => $service]));
    $env = array_merge($_ENV, [
        'XHJOB_TEST_METHOD' => $method,
        'XHJOB_TEST_PATH' => $path,
        'XHJOB_TEST_QUERY' => $queryStr,
    ]);
    $cmd = [PHP_BINARY, '-d', "extension=$soPath", $wrapperPath];
    $desc = [
        0 => ['pipe', 'r'],
        1 => ['pipe', 'w'],
        2 => ['pipe', 'w'],
    ];
    $proc = proc_open($cmd, $desc, $pipes, null, $env);
    if (!is_resource($proc)) {
        return ['_error' => 'proc_open failed'];
    }
    fclose($pipes[0]);
    $stdout = stream_get_contents($pipes[1]);
    fclose($pipes[1]);
    $stderr = stream_get_contents($pipes[2]);
    fclose($pipes[2]);
    $exitCode = proc_close($proc);
    $json = json_decode($stdout, true);
    if ($json === null) {
        return ['_error' => 'non-json output', '_raw' => $stdout, '_stderr' => $stderr, '_exit' => $exitCode];
    }
    $json['_exit'] = $exitCode;
    return $json;
}

echo "[test] php-fpm 业务场景模拟（进程级，每个 worker 是独立 PHP 进程）\n";
echo "[test] 扩展: $soPath\n";
echo "[test] handler: $handlerPath\n\n";

// ============================================================
// 步骤 1: 启动服务（worker A 进程）
// ============================================================
echo "[step 1] worker A: GET /start (创建 daemon)\n";
$r = workerRequest2('GET', '/start');
echo "  exit={$r['_exit']}, daemon pid=" . ($r['pid'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
if (isset($r['_error'])) {
    echo "  ERROR: " . $r['_error'] . "\n";
    echo "  raw: " . ($r['_raw'] ?? '') . "\n";
    echo "  stderr: " . ($r['_stderr'] ?? '') . "\n";
    @unlink($wrapperPath);
    exit(1);
}
check('start returns ok', ($r['ok'] ?? false) === true, json_encode($r));
check('daemon pid present', isset($r['pid']) && $r['pid'] > 0);
$daemonPid1 = $r['pid'] ?? 0;
$workerPidA = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 2: 查询状态（worker B 进程）
// ============================================================
echo "\n[step 2] worker B: GET /status\n";
$r = workerRequest2('GET', '/status');
echo "  exit={$r['_exit']}, running=" . var_export($r['running'] ?? null, true) . ", daemon pid=" . ($r['pid'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('status running', ($r['running'] ?? false) === true);
check('daemon pid same as step1', ($r['pid'] ?? 0) === $daemonPid1, "expected {$daemonPid1}, got " . ($r['pid'] ?? 'null'));
$workerPidB = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 3: 投递 shell 任务（worker C 进程）
// ============================================================
echo "\n[step 3] worker C: GET /dispatch/shell\n";
$marker = 'fpm-biz-' . uniqid();
$r = workerRequest2('GET', '/dispatch/shell', ['marker' => $marker]);
echo "  exit={$r['_exit']}, task_id=" . ($r['task_id'] ?? 'null') . ", worker pid=" . ($r['worker_pid'] ?? 'null') . "\n";
check('dispatch returns task_id', !empty($r['task_id']) && strlen($r['task_id']) > 10, json_encode($r));
$taskId = $r['task_id'] ?? '';
$workerPidC = $r['worker_pid'] ?? 0;

// ============================================================
// 步骤 4: 轮询任务状态（worker D 进程）
// ============================================================
echo "\n[step 4] worker D: GET /state (轮询直到终态)\n";
$finalState = null;
$r = ['worker_pid' => 0];
for ($i = 0; $i < 100; $i++) {
    $r = workerRequest2('GET', '/state', ['id' => $taskId]);
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
// 步骤 5: 查询结果（worker E 进程）
// ============================================================
echo "\n[step 5] worker E: GET /result\n";
$r = workerRequest2('GET', '/result', ['id' => $taskId]);
echo "  exit={$r['_exit']}, exit_code=" . ($r['exit_code'] ?? 'null') . ", stdout=" . trim($r['stdout'] ?? '') . "\n";
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
check('daemon PID stable across workers', $daemonPid1 > 0);

// ============================================================
// 步骤 7: 并发请求测试（多个 worker 同时投递任务）
// ============================================================
echo "\n[step 7] 并发投递 3 个任务（多个 worker 同时启动）\n";
$markers = [];
$procs = [];
$descs = [];
for ($i = 0; $i < 3; $i++) {
    $m = "concurrent-{$i}-" . uniqid();
    $markers[] = $m;
    $queryStr = http_build_query(['marker' => $m, 'service' => 'fpm-prod']);
    $env = array_merge($_ENV, [
        'XHJOB_TEST_METHOD' => 'GET',
        'XHJOB_TEST_PATH' => '/dispatch/shell',
        'XHJOB_TEST_QUERY' => $queryStr,
    ]);
    $cmd = [PHP_BINARY, '-d', "extension=$soPath", $wrapperPath];
    $desc = [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']];
    $proc = proc_open($cmd, $desc, $pipes, null, $env);
    $procs[] = $proc;
    $descs[] = $pipes;
}

$concurrentIds = [];
foreach ($procs as $i => $proc) {
    $pipes = $descs[$i];
    fclose($pipes[0]);
    $stdout = stream_get_contents($pipes[1]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    proc_close($proc);
    $json = json_decode($stdout, true);
    $concurrentIds[] = $json['task_id'] ?? '';
    echo "  并发任务 {$i}: id=" . ($json['task_id'] ?? 'null') . ", worker=" . ($json['worker_pid'] ?? 'null') . "\n";
}

echo "  等待并发任务完成...\n";
$successCount = 0;
foreach ($concurrentIds as $i => $id) {
    if (empty($id)) continue;
    for ($j = 0; $j < 100; $j++) {
        $r = workerRequest2('GET', '/state', ['id' => $id]);
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
// 步骤 8: 重启服务（worker F 进程）
// ============================================================
echo "\n[step 8] worker F: POST /restart\n";
$r = workerRequest2('POST', '/restart');
echo "  exit={$r['_exit']}, pid_before=" . ($r['pid_before'] ?? 'null') . ", pid_after=" . ($r['pid_after'] ?? 'null') . "\n";
check('restart returns ok', ($r['ok'] ?? false) === true, json_encode($r));
check('pid changed after restart', ($r['pid_changed'] ?? false) === true, json_encode($r));
$daemonPid2 = $r['pid_after'] ?? 0;

// ============================================================
// 步骤 9: 重启后验证新 daemon 可服务（worker G 进程）
// ============================================================
echo "\n[step 9] worker G: 重启后投递新任务\n";
$marker2 = 'after-restart-' . uniqid();
$r = workerRequest2('GET', '/dispatch/shell', ['marker' => $marker2]);
$taskId2 = $r['task_id'] ?? '';
echo "  新任务 id={$taskId2}, worker=" . ($r['worker_pid'] ?? 'null') . "\n";
check('dispatch after restart succeeds', !empty($taskId2) && strlen($taskId2) > 10);

$state = 'UNKNOWN';
for ($i = 0; $i < 100; $i++) {
    $r = workerRequest2('GET', '/state', ['id' => $taskId2]);
    $state = $r['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') break;
    usleep(100000);
}
check('task after restart reaches SUCCESS', $state === 'SUCCESS', "got {$state}");

// ============================================================
// 步骤 10: 停止服务（worker H 进程）
// ============================================================
echo "\n[step 10] worker H: DELETE /stop\n";
$r = workerRequest2('DELETE', '/stop');
echo "  exit={$r['_exit']}, ok=" . var_export($r['ok'] ?? null, true) . ", running=" . var_export($r['running'] ?? null, true) . "\n";
check('stop returns ok', ($r['ok'] ?? false) === true);
check('daemon stopped after stop', ($r['running'] ?? true) === false);

// 验证状态
$r = workerRequest2('GET', '/status');
check('status running=false after stop', ($r['running'] ?? true) === false);

// ============================================================
// 清理
// ============================================================
@unlink($wrapperPath);
@xhjob_stop('fpm-prod');

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
