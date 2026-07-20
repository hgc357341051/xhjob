<?php
/**
 * php-cli 业务场景 - 第 1 步：创建（启动）服务
 *
 * 模拟生产环境：运维脚本或部署脚本启动一个独立的后台任务调度服务。
 * 该脚本执行完毕后退出，但 daemon 进程必须继续存活。
 *
 * 用法：
 *   php -d extension=../../../target/release/libxhjob.so 01_start_service.php [service_name]
 *
 * 默认服务名：production-queue
 */

$serviceName = $argv[1] ?? 'production-queue';

echo "[start] 准备启动服务: {$serviceName}\n";

// 清理可能残留的旧实例
@xhjob_stop($serviceName);
usleep(300000);

// 检查启动前状态
$before = xhjob_status($serviceName);
echo "[start] 启动前状态: running={$before['running']}\n";

// 启动 daemon
$result = xhjob_start($serviceName);
if ($result !== true) {
    echo "[start] FAIL: xhjob_start 返回 false\n";
    exit(1);
}
echo "[start] xhjob_start({$serviceName}) = true\n";

// 等待 daemon 完全就绪
$ready = false;
for ($i = 0; $i < 50; $i++) {
    $st = xhjob_status($serviceName);
    if ($st['running'] === 'true' && isset($st['pid'])) {
        echo "[start] 服务已就绪: pid={$st['pid']}\n";
        $ready = true;
        break;
    }
    usleep(100000);
}
if (!$ready) {
    echo "[start] FAIL: 服务未在 5 秒内就绪\n";
    exit(2);
}

// 确认当前 PHP 进程退出后 daemon 仍存活（通过查询 status 验证）
$pid = $st['pid'];
echo "[start] 当前 PHP PID=" . posix_getpid() . ", daemon PID={$pid}\n";
echo "[start] daemon 是独立进程（PID 不同）: " . ((int)$pid !== posix_getpid() ? 'YES' : 'NO') . "\n";

// 验证服务名持久化（PID 文件存在）
$pidFile = sys_get_temp_dir() . "/xhjob.{$serviceName}.pid";
if (file_exists($pidFile)) {
    $filePid = (int)trim(file_get_contents($pidFile));
    echo "[start] PID 文件 {$pidFile} 存在, filePid={$filePid}, matches=" . ($filePid === (int)$pid ? 'YES' : 'NO') . "\n";
} else {
    echo "[start] WARN: PID 文件不存在 {$pidFile}\n";
}

echo "[start] SUCCESS: 服务 {$serviceName} 已启动并就绪\n";
echo "[start] 注意: 本 PHP 进程退出后, daemon PID={$pid} 将继续运行\n";
exit(0);
