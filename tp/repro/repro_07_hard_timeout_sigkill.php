<?php
// +----------------------------------------------------------------------
// | repro_07: 硬超时 SIGKILL（Task 10 / SubTask 10.7）
// +----------------------------------------------------------------------
// | 场景：派发 sleep 30 + timeout=2（无 soft_timeout），验证 2s 后
// |       任务 failed + 错误含 "timeout after 2s" + 子进程被 reap。
// |
// | 预期（shell.rs 硬超时路径）：
// |   1. tokio::timeout(2s) 触发 → start_kill() (SIGKILL) + wait() reap
// |   2. 返回 Err("timeout after 2s")
// |   3. 任务标记 failed，last_error 含 "timeout after 2s"
// |   4. sleep 子进程已死（无僵尸残留）
// |
// | 注意：bash -c "sleep 30" 单命令会被 exec 替换，所以 child PID ==
// |       sleep PID。start_kill() 直接杀 sleep，wait() reap 避免僵尸。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;

$service = 'repro07';
$dataDir = '/tmp/xhjob-repro07';

repro_header(7, '硬超时 SIGKILL（timeout=2 无 soft_timeout）');

$pid = null;
$taskId = null;
$workerPid = null;

try {
    repro_step('启动 daemon', function () use ($service, $dataDir, &$pid) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
    });

    $mgr = new TaskManager($service, $dataDir);

    repro_step('派发 sleep 30 任务 (timeout=2, 无 soft_timeout)', function () use ($mgr, &$taskId) {
        $taskId = $mgr->create(
            TaskBuilder::shell('sleep 30')
                ->timeout(2)
                ->withRetry(0, 0)
        );
        repro_assert(!empty($taskId) && strpos($taskId, 'error') === false, "dispatch 失败: {$taskId}");
    });

    // 等 2s 让任务进入 Running + 记录 worker_pid
    echo "  等待 2s 让任务进入 Running...\n";
    sleep(2);

    repro_step('任务进入 Running + worker_pid 已记录', function () use ($mgr, $taskId, &$workerPid) {
        $st = $mgr->state($taskId);
        $state = $st['state'] ?? '?';
        $workerPid = $st['worker_pid'] ?? null;
        echo "  state={$state} worker_pid=" . var_export($workerPid, true) . "\n";
        // xhjob_state 当前未透出 worker_pid 字段（Rust 侧 StateInfo 未含此字段），
        // 且 worker_pid 仅在任务完成后才写入 store。此处不阻断测试——
        // 核心断言在后续的 failed + timeout after 2s + 无残留进程。
        if ($workerPid === null) {
            echo "  [INFO] xhjob_state 未返回 worker_pid（已知限制），跳过 worker_pid 断言\n";
            return 'SKIP';
        }
        repro_assert($workerPid > 0, "worker_pid 应 > 0，实际: " . var_export($workerPid, true));
    });

    // 等待硬超时触发（timeout=2s + reap 余量）
    echo "  等待 5s 让硬超时 (2s) 触发 + reap...\n";
    sleep(5);

    repro_step('任务状态为 failed', function () use ($mgr, $taskId) {
        $st = $mgr->state($taskId);
        $state = $st['state'] ?? '?';
        echo "  state={$state}\n";
        repro_assert($state === 'failed', "任务应为 failed（硬超时 SIGKILL），实际: {$state}");
    });

    repro_step('last_error 含 "timeout after 2s"', function () use ($mgr, $taskId) {
        $st = $mgr->state($taskId);
        $lastError = (string)($st['last_error'] ?? '');
        echo "  last_error=" . substr($lastError, 0, 200) . "\n";
        repro_assert(
            stripos($lastError, 'timeout after 2s') !== false,
            "last_error 应含 'timeout after 2s'，实际: {$lastError}"
        );
    });

    // 验证子进程已被 reap（worker_pid 已死）
    repro_step('sleep 子进程已被 reap（worker_pid 已死）', function () use ($workerPid) {
        if ($workerPid === null || $workerPid <= 0) {
            return 'SKIP';
        }
        $alive = posix_kill($workerPid, 0); // signal 0 = 探活
        repro_assert(!$alive, "worker_pid={$workerPid} 应已死（被 SIGKILL + reap），但仍存活");
    });

    // 辅助验证：ps 无残留 sleep 30 子进程（排除测试自身的 grep）
    repro_step('ps 无残留 "sleep 30" 子进程', function () {
        $cmd = 'ps aux 2>/dev/null | grep "sleep 30" | grep -v grep | grep -v repro || true';
        $out = trim((string) shell_exec($cmd));
        echo "  ps 残留: " . ($out === '' ? '(无)' : $out) . "\n";
        repro_assert($out === '', "不应有残留 sleep 30 进程，实际: {$out}");
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    stop_daemon($service, $dataDir);
    // 兜底清理：杀掉可能残留的 sleep 30 子进程
    @system('pkill -f "sleep 30" 2>/dev/null');
    cleanup_data_dir($dataDir);
}

repro_summary();
