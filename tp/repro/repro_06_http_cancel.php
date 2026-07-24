<?php
// +----------------------------------------------------------------------
// | repro_06: HTTP 任务取消（Task 10 / SubTask 10.6）
// +----------------------------------------------------------------------
// | 场景：启动假 HTTP server（sleep 30 后响应），派发 HTTP 任务，
// |       任务进入 Running 后调用 xhjob_cancel。
// |
// | 预期：
// |   1. cancel 触发 executor 的 cancel_flag → 中断 HTTP 请求
// |   2. 任务状态转为 cancelled（非 running）
// |   3. daemon 无 panic / 无未捕获错误
// |
// | 注意：假 server 用 php -S 127.0.0.1:<port> router.php 启动，
// |       router 内 sleep 30 模拟慢响应。端口冲突时自动 +1 重试。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;

$service = 'repro06';
$dataDir = '/tmp/xhjob-repro06';
// router 文件放在 dataDir 之外：start_daemon 会 rm -rf $dataDir，
// 若 router 在其中则 server 启动后文件被删 → 500 错误。
$routerFile = '/tmp/xhjob-repro06-router.php';

repro_header(6, 'HTTP 任务取消');

$pid = null;
$taskId = null;
$httpPort = 18080;
$httpServerPid = null;

try {
    // 准备假 HTTP server router（sleep 30 后响应）
    file_put_contents($routerFile, <<<'PHP'
<?php
// 慢响应 router：sleep 30 后返回 200，模拟长 HTTP 任务
sleep(30);
http_response_code(200);
header('Content-Type: text/plain');
echo "repro06-response-after-sleep";
PHP
    );

    // 选择可用端口（18080 起，最多试 5 个）
    repro_step('启动假 HTTP server (sleep 30)', function () use (&$httpPort, $routerFile, &$httpServerPid) {
        $php = PHP_BINARY;
        for ($p = 18080; $p <= 18085; $p++) {
            // 检测端口是否空闲
            $sock = @fsockopen('127.0.0.1', $p, $errno, $errstr, 0.2);
            if ($sock) {
                fclose($sock);
                continue; // 端口被占用
            }
            $httpPort = $p;
            break;
        }
        $cmd = 'env -u PHP_INI_SCAN_DIR ' . escapeshellarg($php)
            . ' -d extension= -S 127.0.0.1:' . $httpPort . ' ' . escapeshellarg($routerFile)
            . ' >/dev/null 2>&1 & echo $!';
        $out = trim((string) shell_exec($cmd));
        $httpServerPid = (int) $out;
        repro_assert($httpServerPid > 0, "假 HTTP server 启动失败");
        // 等待 server 就绪
        $ready = false;
        for ($i = 0; $i < 20; $i++) {
            $sock = @fsockopen('127.0.0.1', $httpPort, $errno, $errstr, 0.3);
            if ($sock) {
                fclose($sock);
                $ready = true;
                break;
            }
            usleep(200000);
        }
        repro_assert($ready, "假 HTTP server 未就绪 port={$httpPort}");
        echo "  port={$httpPort} pid={$httpServerPid}\n";
    });

    repro_step('启动 daemon', function () use ($service, $dataDir, &$pid) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
    });

    $mgr = new TaskManager($service, $dataDir);

    repro_step('派发 HTTP 任务 (timeout=60)', function () use ($mgr, $httpPort, &$taskId) {
        $taskId = $mgr->create(
            TaskBuilder::http('GET', 'http://127.0.0.1:' . $httpPort . '/')
                ->timeout(60)
                ->withRetry(0, 0)
        );
        repro_assert(!empty($taskId) && strpos($taskId, 'error') === false, "dispatch 失败: {$taskId}");
    });

    echo "  等待 1s 让 HTTP 任务进入 Running...\n";
    sleep(1);

    repro_step('HTTP 任务进入 Running', function () use ($mgr, $taskId) {
        $st = $mgr->state($taskId);
        $state = $st['state'] ?? '?';
        if ($state !== 'running') {
            $lastError = $st['last_error'] ?? '(无)';
            echo "  state={$state} last_error={$lastError}\n";
        }
        repro_assert($state === 'running', "任务应为 running，实际: {$state}");
    });

    // 若任务已失败（HTTP 连接问题等），跳过 cancel 测试
    $preState = $mgr->state($taskId);
    if (($preState['state'] ?? '') !== 'running') {
        repro_step('调用 xhjob_cancel', function () {
            return 'SKIP';
        }, '任务非 running，跳过 cancel');
    } else {
        repro_step('调用 xhjob_cancel', function () use ($mgr, $taskId) {
            $ok = $mgr->stop($taskId); // stop = cancel
            repro_assert($ok, 'xhjob_cancel 返回 false');
        });
    }

    echo "  等待 3s 让 cancel 生效...\n";
    sleep(3);

    repro_step('任务状态为 cancelled（非 running）', function () use ($mgr, $taskId) {
        $st = $mgr->state($taskId);
        $state = $st['state'] ?? '?';
        echo "  state={$state}\n";
        repro_assert(
            in_array($state, ['cancelled', 'interrupted', 'failed'], true),
            "任务应为 cancelled/interrupted/failed（HTTP 请求被中断），实际: {$state}"
        );
        repro_assert($state !== 'running', "任务不应仍为 running（cancel 未生效）");
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    // 清理：停 daemon + 杀假 HTTP server
    stop_daemon($service, $dataDir);
    if ($httpServerPid !== null && $httpServerPid > 0) {
        @posix_kill($httpServerPid, 9); // SIGKILL 假 server
        // 同时杀掉可能残留的 php -S 子进程
        @system('pkill -f ' . escapeshellarg($routerFile) . ' 2>/dev/null');
    }
    @unlink($routerFile);
    cleanup_data_dir($dataDir);
}

repro_summary();
