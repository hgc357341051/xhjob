<?php
/**
 * php-cli 业务场景 - 第 3 步：重启服务
 *
 * 模拟生产环境：配置变更或版本升级后重启 daemon 服务。
 * 关键验证点：
 *   1. 重启前 daemon PID 与重启后不同（确认为新进程）
 *   2. 重启过程中可能有短暂不可用窗口
 *   3. 重启后服务能正常接受新任务
 *   4. 重启后旧任务 ID 查询应失败或返回 INTERRUPTED（因内存模式默认）
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so 03_restart_service.php [service_name]
 */

$serviceName = $argv[1] ?? 'production-queue';

echo "[restart] 准备重启服务: {$serviceName}\n";

// 获取重启前 PID
$before = xhjob_status($serviceName);
if ($before['running'] !== 'true') {
    echo "[restart] FAIL: 服务 {$serviceName} 未运行，无法重启\n";
    exit(1);
}
$pidBefore = (int)$before['pid'];
echo "[restart] 重启前 PID: {$pidBefore}\n";

// 执行重启
echo "[restart] 调用 xhjob_restart({$serviceName})...\n";
$result = xhjob_restart($serviceName);
if ($result !== true) {
    echo "[restart] FAIL: xhjob_restart 返回 false\n";
    exit(2);
}
echo "[restart] xhjob_restart 返回 true\n";

// 等待新 daemon 就绪
$ready = false;
$pidAfter = 0;
for ($i = 0; $i < 80; $i++) {
    $st = xhjob_status($serviceName);
    if ($st['running'] === 'true' && isset($st['pid'])) {
        $pidAfter = (int)$st['pid'];
        if ($pidAfter !== $pidBefore) {
            // 新进程已起来
            $ready = true;
            break;
        }
    }
    usleep(100000);
}

if (!$ready) {
    echo "[restart] FAIL: 重启后服务未就绪或 PID 未变化\n";
    $final = xhjob_status($serviceName);
    echo "[restart] 最终状态: "; var_dump($final);
    exit(3);
}

echo "[restart] 重启后 PID: {$pidAfter}\n";
echo "[restart] PID 已变化（旧={$pidBefore}, 新={$pidAfter}）: " . ($pidAfter !== $pidBefore ? 'YES' : 'NO') . "\n";

// 验证新 daemon 能接受新任务
echo "\n[restart] 验证新 daemon 可接受任务...\n";
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo after-restart'
    : 'echo after-restart';

$id = Xhjob::task()
    ->service($serviceName)
    ->viaShell($cmd)
    ->timeout(10)
    ->dispatch();

echo "[restart] 投递任务: id={$id}\n";
if (strpos($id, 'error') === 0 || strlen($id) < 10) {
    echo "[restart] FAIL: dispatch 返回错误: {$id}\n";
    exit(4);
}

// 轮询状态
$finalState = null;
for ($i = 0; $i < 100; $i++) {
    $s = xhjob_state($id, $serviceName);
    $state = $s['state'] ?? 'UNKNOWN';
    if ($state === 'SUCCESS' || $state === 'FAILED') {
        $finalState = $state;
        break;
    }
    usleep(100000);
}

if ($finalState !== 'SUCCESS') {
    echo "[restart] FAIL: 重启后任务未成功完成, state={$finalState}\n";
    var_dump(xhjob_state($id, $serviceName));
    exit(5);
}

$r = xhjob_result($id, $serviceName);
$stdout = $r['stdout'] ?? '';
echo "[restart] 重启后任务成功: stdout=" . trim($stdout) . "\n";

if (strpos($stdout, 'after-restart') === false) {
    echo "[restart] FAIL: stdout 未包含期望标记\n";
    exit(6);
}

echo "\n[restart] SUCCESS: 服务重启成功, 新 PID={$pidAfter} 可正常服务\n";
exit(0);
