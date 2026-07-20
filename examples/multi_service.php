<?php
/**
 * Example: 多服务实例（named services）。
 *
 * 演示如何启动两个独立的 daemon 实例（cron-svc 与 queue-svc），
 * 分别 dispatch 任务，并验证它们彼此隔离、独立运行。
 *
 * 用法：
 *   php -d extension=target/release/libxhjob.so examples/multi_service.php
 */

// 清理可能残留的旧服务
@xhjob_stop('cron-svc');  usleep(200000);
@xhjob_stop('queue-svc'); usleep(200000);

// 1. 启动两个独立服务
if (!xhjob_start('cron-svc')) {
    fwrite(STDERR, "Failed to start cron-svc\n");
    exit(1);
}
if (!xhjob_start('queue-svc')) {
    fwrite(STDERR, "Failed to start queue-svc\n");
    xhjob_stop('cron-svc');
    exit(1);
}

$s1 = xhjob_status('cron-svc');
$s2 = xhjob_status('queue-svc');
echo "cron-svc  running={$s1['running']} pid=" . ($s1['pid'] ?? 'N/A') . "\n";
echo "queue-svc running={$s2['running']} pid=" . ($s2['pid'] ?? 'N/A') . "\n";

// 2. 通过 Xhjob::service($name)->task()->... 链式 API 分别 dispatch
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo from-cron-svc'
    : 'echo from-cron-svc';

$id1 = Xhjob::service('cron-svc')->task()
    ->viaShell($cmd)
    ->timeout(10)
    ->dispatch();
echo "cron-svc dispatched: {$id1}\n";

$id2 = Xhjob::service('queue-svc')->task()
    ->viaShell('echo from-queue-svc')
    ->timeout(10)
    ->dispatch();
echo "queue-svc dispatched: {$id2}\n";

// 3. 轮询各自服务的任务状态
foreach (['cron-svc' => $id1, 'queue-svc' => $id2] as $svc => $id) {
    for ($i = 0; $i < 50; $i++) {
        $st = xhjob_state($id, $svc);
        $state = $st['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            echo "{$svc} task {$id}: state={$state}\n";
            $r = xhjob_result($id, $svc);
            if (isset($r['stdout'])) echo "  stdout={$r['stdout']}\n";
            break;
        }
        usleep(100000);
    }
}

// 4. 清理两个服务
xhjob_stop('cron-svc');
xhjob_stop('queue-svc');
echo "Done.\n";
