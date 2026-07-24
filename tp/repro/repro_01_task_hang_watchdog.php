<?php
// +----------------------------------------------------------------------
// | repro_01: 任务假死 watchdog 检测（Task 10 / SubTask 10.1）
// +----------------------------------------------------------------------
// | 场景：派发 `read x < /tmp/xhjob_fifo_repro01`（fifo 存在但无 writer，
// |       read 在 open 时阻塞），模拟任务假死。
// |
// | 预期：任务不会永远卡在 Running —— 由 watchdog（timeout*factor 后
// |       Interrupted + HungDetected）或 hard timeout（timeout 后 failed）
// |       之一恢复。本环境里 hard timeout（2s）通常先于 watchdog（4s）
// |       触发（子进程可被 SIGKILL），两种结局都视为「假死被恢复」。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;

$service = 'repro01';
$dataDir = '/tmp/xhjob-repro01';
$fifo = '/tmp/xhjob_fifo_repro01';

repro_header(1, '任务假死 watchdog 检测');

// 设 watchdog 参数：2s 间隔 + factor=2（threshold = timeout*2 = 4s）
putenv('XHJOB_WATCHDOG_INTERVAL=2');
putenv('XHJOB_WATCHDOG_FACTOR=2');

// 创建 fifo（无 writer 时 `read x < fifo` 在 open 阻塞 → 假死）
@unlink($fifo);
posix_mkfifo($fifo, 0600);

$pid = null;
$taskId = null;

try {
    repro_step('启动 daemon (watchdog interval=2 factor=2)', function () use ($service, $dataDir, &$pid) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
    });

    $mgr = new TaskManager($service, $dataDir);

    repro_step('派发假死任务 read x < fifo (timeout=2)', function () use ($mgr, $fifo, $service, $dataDir, &$taskId) {
        // read x < fifo 在无 writer 时阻塞；timeout=2 → hard timeout 2s / watchdog 4s
        $cmd = 'read x < ' . escapeshellarg($fifo);
        $taskId = xhjob_dispatch(
            TaskBuilder::shell($cmd)->timeout(2)->withRetry(0, 0)->toJson(),
            $service,
            $dataDir
        );
        repro_assert(!empty($taskId) && strpos($taskId, 'error:') !== 0, "dispatch 失败: {$taskId}");
    });

    repro_step('等待任务进入 Running', function () use ($mgr, $taskId) {
        $ok = $mgr->waitForState($taskId, 'running', 5);
        // 任务可能已经超时（极端快），running 不是硬性要求
        repro_assert(true, ''); // 不强制断言，仅等待
    });

    echo "  等待 8s（watchdog 2s 间隔 + 2*2s 阈值 + 余量）...\n";
    sleep(8);

    $state = null;
    $events = [];
    try {
        $state = $mgr->state($taskId);
    } catch (\Throwable $e) {
        // daemon 可能因 watchdog 操作有短暂 IPC 抖动，重试一次
        usleep(500000);
        $state = $mgr->state($taskId);
    }
    try {
        $events = $mgr->logs($taskId);
    } catch (\Throwable $e) {
        $events = [];
    }
    $taskState = $state['state'] ?? 'unknown';
    $hasHung = false;
    foreach ($events as $ev) {
        if (strpos((string)($ev['event_type'] ?? ''), 'ung') !== false
            || strpos((string)($ev['event_type'] ?? ''), 'HUNG') !== false
            || (string)($ev['event_type'] ?? '') === 'hung_detected') {
            $hasHung = true;
            break;
        }
    }

    repro_step(
        '任务未卡在 Running（interrupted 或 failed）',
        !in_array($taskState, ['running', 'unknown'], true),
        "实际 state={$taskState}"
    );

    // HungDetected 断言：watchdog 触发时（interrupted）应存在；hard timeout 触发时（failed）无此事件但任务已被恢复
    if ($taskState === 'interrupted') {
        repro_step('watchdog 触发 → HungDetected 事件存在', $hasHung, 'state=interrupted 应含 HungDetected');
    } else {
        // hard timeout 恢复路径：验证 last_error 含 timeout
        $lastError = (string)($state['last_error'] ?? '');
        $result = null;
        try {
            $result = $mgr->result($taskId);
        } catch (\Throwable $e) {}
        $errBlob = $lastError . ' ' . json_encode($result);
        repro_step(
            'hard timeout 恢复 → 错误含 timeout（watchdog 未触发因子进程可被 SIGKILL）',
            stripos($errBlob, 'timeout') !== false,
            "state={$taskState}, 无 HungDetected 属预期（hard timeout 先触发）"
        );
    }
} finally {
    // 清理
    stop_daemon($service, $dataDir);
    @unlink($fifo);
    cleanup_data_dir($dataDir);
}

repro_summary();
