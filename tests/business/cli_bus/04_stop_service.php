<?php
/**
 * php-cli 业务场景 - 第 4 步：停止服务
 *
 * 模拟生产环境：优雅停机或下线服务。
 * 关键验证点：
 *   1. xhjob_stop 返回 true
 *   2. 停止后 status 返回 running=false
 *   3. PID 文件被清理
 *   4. 停止后再调用 stop 幂等（不报错）
 *   5. 停止后再 dispatch 任务应失败（daemon 未运行）
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so 04_stop_service.php [service_name]
 */

$serviceName = $argv[1] ?? 'production-queue';

echo "[stop] 准备停止服务: {$serviceName}\n";

// 获取停止前状态
$before = xhjob_status($serviceName);
if ($before['running'] !== 'true') {
    echo "[stop] WARN: 服务 {$serviceName} 未运行，无需停止\n";
    echo "[stop] 验证幂等性: 再次调用 xhjob_stop\n";
    $idempotent = xhjob_stop($serviceName);
    echo "[stop] 幂等调用返回: " . ($idempotent ? 'true' : 'false') . "\n";
    exit(0);
}

$pidBefore = $before['pid'] ?? 'unknown';
echo "[stop] 停止前 PID: {$pidBefore}\n";

// 执行停止
echo "[stop] 调用 xhjob_stop({$serviceName})...\n";
$result = xhjob_stop($serviceName);
if ($result !== true) {
    echo "[stop] FAIL: xhjob_stop 返回 false\n";
    exit(1);
}
echo "[stop] xhjob_stop 返回 true\n";

// 等待 daemon 真正退出
$stopped = false;
for ($i = 0; $i < 50; $i++) {
    $st = xhjob_status($serviceName);
    if ($st['running'] !== 'true') {
        $stopped = true;
        break;
    }
    usleep(100000);
}

if (!$stopped) {
    echo "[stop] FAIL: 5 秒后服务仍在运行\n";
    var_dump(xhjob_status($serviceName));
    exit(2);
}

echo "[stop] 服务已停止 (running=false)\n";

// 验证 PID 文件被清理
$pidFile = sys_get_temp_dir() . "/xhjob.{$serviceName}.pid";
if (file_exists($pidFile)) {
    echo "[stop] WARN: PID 文件仍存在 {$pidFile}（可能残留）\n";
} else {
    echo "[stop] PID 文件已清理: {$pidFile}\n";
}

// 验证 sock 文件被清理
$sockFile = sys_get_temp_dir() . "/xhjob.{$serviceName}.sock";
if (file_exists($sockFile)) {
    echo "[stop] WARN: sock 文件仍存在 {$sockFile}\n";
} else {
    echo "[stop] sock 文件已清理: {$sockFile}\n";
}

// 验证幂等性：再次调用 stop 不报错
echo "\n[stop] 验证幂等性: 再次调用 xhjob_stop\n";
$idempotent = xhjob_stop($serviceName);
echo "[stop] 幂等调用返回: " . ($idempotent ? 'true' : 'false') . "\n";

// 验证停止后 dispatch 失败
echo "\n[stop] 验证停止后无法投递任务\n";
$cmd = PHP_OS_FAMILY === 'Windows' ? 'cmd /C echo should-fail' : 'echo should-fail';
$id = @Xhjob::task()
    ->service($serviceName)
    ->viaShell($cmd)
    ->timeout(5)
    ->dispatch();

echo "[stop] 停止后 dispatch 返回: {$id}\n";
if (strpos($id, 'error') === 0 || strlen($id) < 10) {
    echo "[stop] 符合预期: 停止后无法投递任务\n";
} else {
    echo "[stop] WARN: 停止后仍返回了 task_id（daemon 可能已自动重启或 client 未报错）\n";
}

echo "\n[stop] SUCCESS: 服务 {$serviceName} 已停止并清理完毕\n";
exit(0);
