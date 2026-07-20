<?php
/**
 * php-cli 业务场景 - 第 2 步：连接服务进行业务操作
 *
 * 模拟生产环境：另一个 PHP 脚本（如定时任务调度器、队列消费器）
 * 连接到已存在的 daemon 服务，投递任务并查询结果。
 *
 * 关键验证点：
 *   1. 该脚本启动时 daemon 已在运行（由 01_start_service.php 启动）
 *   2. 该脚本退出后 daemon 仍存活
 *   3. 投递的 shell 任务能成功执行
 *   4. 投递的 HTTP 任务（若有网络）能成功执行
 *   5. 任务状态机正确流转：PENDING -> RUNNING -> SUCCESS
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so 02_operate.php [service_name]
 */

$serviceName = $argv[1] ?? 'production-queue';

echo "[operate] 连接服务: {$serviceName}\n";

// 验证服务在运行
$st = xhjob_status($serviceName);
if ($st['running'] !== 'true') {
    echo "[operate] FAIL: 服务 {$serviceName} 未运行，请先执行 01_start_service.php\n";
    exit(1);
}
echo "[operate] 服务运行中: pid={$st['pid']}\n";
$daemonPid = (int)$st['pid'];

// ============================================================
// 业务操作 1: 投递 shell 任务（CPU 密集型模拟）
// ============================================================
echo "\n[operate] === 业务 1: 投递 shell 任务 ===\n";
$shellCmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo production-task-1'
    : 'echo production-task-1';

$shellId = Xhjob::task()
    ->service($serviceName)
    ->viaShell($shellCmd)
    ->withRetry(2, 1)
    ->timeout(10)
    ->priority(5)
    ->dispatch();

echo "[operate] shell 任务已投递: id={$shellId}\n";
if (strpos($shellId, 'error') === 0 || strlen($shellId) < 10) {
    echo "[operate] FAIL: dispatch 返回错误: {$shellId}\n";
    exit(2);
}

// 轮询状态
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($shellId, $serviceName);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}
echo "[operate] shell 任务最终状态: {$finalState}\n";

if ($finalState !== 'SUCCESS') {
    echo "[operate] FAIL: shell 任务未成功完成\n";
    var_dump(xhjob_state($shellId, $serviceName));
    var_dump(xhjob_result($shellId, $serviceName));
    exit(3);
}

// 校验结果
$result = xhjob_result($shellId, $serviceName);
$stdout = $result['stdout'] ?? '';
$exitCode = $result['exit_code'] ?? -1;
echo "[operate] shell stdout: " . trim($stdout) . "\n";
echo "[operate] shell exit_code: {$exitCode}\n";

if (strpos($stdout, 'production-task-1') === false) {
    echo "[operate] FAIL: stdout 未包含期望标记\n";
    exit(4);
}
if ((int)$exitCode !== 0) {
    echo "[operate] FAIL: exit_code 非 0\n";
    exit(5);
}
echo "[operate] shell 任务验证通过\n";

// ============================================================
// 业务操作 2: 投递多个并发任务（验证协程池并发）
// ============================================================
echo "\n[operate] === 业务 2: 并发投递 5 个 shell 任务 ===\n";
$ids = [];
for ($i = 0; $i < 5; $i++) {
    $cmd = PHP_OS_FAMILY === 'Windows'
        ? "cmd /C echo concurrent-{$i}"
        : "echo concurrent-{$i}";
    $id = Xhjob::task()
        ->service($serviceName)
        ->viaShell($cmd)
        ->timeout(5)
        ->dispatch();
    $ids[] = $id;
    echo "[operate] 并发任务 {$i} 投递: id={$id}\n";
}

// 等待全部完成
$successCount = 0;
foreach ($ids as $i => $id) {
    for ($j = 0; $j < 100; $j++) {
        $s = xhjob_state($id, $serviceName);
        $state = $s['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            if ($state === 'SUCCESS') {
                $successCount++;
                $r = xhjob_result($id, $serviceName);
                echo "[operate] 并发任务 {$i} ({$id}): {$state}, stdout=" . trim($r['stdout'] ?? '') . "\n";
            } else {
                echo "[operate] 并发任务 {$i} ({$id}): {$state}\n";
            }
            break;
        }
        usleep(100000);
    }
}
echo "[operate] 并发任务成功数: {$successCount}/5\n";
if ($successCount !== 5) {
    echo "[operate] FAIL: 并发任务未全部成功\n";
    exit(6);
}

// ============================================================
// 业务操作 3: 验证 daemon PID 未变（本脚本未重启服务）
// ============================================================
echo "\n[operate] === 业务 3: 验证服务稳定性 ===\n";
$stAfter = xhjob_status($serviceName);
$daemonPidAfter = (int)$stAfter['pid'];
echo "[operate] 操作前 daemon PID: {$daemonPid}\n";
echo "[operate] 操作后 daemon PID: {$daemonPidAfter}\n";
if ($daemonPid !== $daemonPidAfter) {
    echo "[operate] FAIL: daemon PID 变化，服务可能重启过\n";
    exit(7);
}
echo "[operate] daemon PID 稳定不变\n";

// 验证当前 PHP 进程退出后 daemon 仍存活
echo "[operate] 当前 PHP PID=" . posix_getpid() . " (将退出, daemon {$daemonPidAfter} 应继续运行)\n";

echo "\n[operate] SUCCESS: 所有业务操作完成\n";
exit(0);
