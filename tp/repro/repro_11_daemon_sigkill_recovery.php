<?php
// +----------------------------------------------------------------------
// | repro_11: daemon SIGKILL 崩溃恢复（Task 10 / SubTask 10.11）
// +----------------------------------------------------------------------
// | 场景：persist daemon 派发 cron 任务，kill -9 模拟 OOM，重启 daemon。
// |
// | 预期（Task 3 + Task 4 修复后）：
// |   1. kill -9 后 pid 文件残留（stale）
// |   2. 重启时 stale pid check（starttime 校验）清理旧 pid 文件
// |   3. SQLite quick_check 通过（DB 未因 SIGKILL 损坏）
// |   4. cron 任务从 DB 恢复（xhjob_get 返回非 null）
// |   5. cron 表达式保留（"* * * * *"）
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$service = 'repro11';
$dataDir = '/tmp/xhjob-repro11';
$pidFile = $dataDir . '/xhjob.' . $service . '.pid';
$dbPath = $dataDir . '/xhjob.' . $service . '.db';

repro_header(11, 'daemon SIGKILL 崩溃恢复（stale pid 清理 + DB 一致性 + 任务恢复）');

putenv('XHJOB_PERSIST=1');

$pid = null;
$taskId = null;

try {
    repro_step('启动 persist daemon', function () use ($service, $dataDir, &$pid) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
    });

    $mgr = new TaskManager($service, $dataDir);

    repro_step('派发 cron 任务 (cron="* * * * *", persist=true)', function () use ($mgr, &$taskId) {
        $taskId = $mgr->create(
            TaskBuilder::shell('echo cron-tick')
                ->cron('* * * * *')
                ->persist(true)
                ->timeout(10)
                ->withRetry(0, 0)
        );
        repro_assert(!empty($taskId) && strpos($taskId, 'error') === false, "dispatch 失败: {$taskId}");
    });

    echo "  等待 3s 确保任务写入 DB...\n";
    sleep(3);

    repro_step('任务已持久化到 DB（xhjob_get 返回非 null）', function () use ($mgr, $taskId) {
        $task = $mgr->get($taskId);
        repro_assert($task !== null, 'xhjob_get 返回 null（任务未持久化）');
        repro_assert(($task['cron'] ?? null) === '* * * * *', "cron 表达式应保留，实际: " . ($task['cron'] ?? '?'));
        echo "  task_id={$taskId} cron=" . ($task['cron'] ?? '?') . "\n";
    });

    repro_step('确认 DB 文件存在 + pid 文件存在', function () use ($dbPath, $pidFile) {
        repro_assert(file_exists($dbPath), "DB 文件不存在: {$dbPath}");
        repro_assert(file_exists($pidFile), "pid 文件不存在: {$pidFile}");
        echo "  db_size=" . filesize($dbPath) . " pid_file=" . $pidFile . "\n";
    });

    // kill -9 模拟 OOM kill
    repro_step('kill -9 daemon 模拟 OOM（保留 DB + stale pid 文件）', function () use ($pid, $pidFile) {
        posix_kill($pid, 9); // SIGKILL
        $deadline = microtime(true) + 5;
        while (microtime(true) < $deadline) {
            if (!posix_kill($pid, 0)) {
                break;
            }
            usleep(100000);
        }
        // Reap zombie（测试环境中 daemon 是 PHP 进程的子进程，kill -9 后变 zombie）
        reap_process($pid);
        repro_assert(!posix_kill($pid, 0), "daemon 应已退出（SIGKILL）");
        // SIGKILL 后 pid 文件应残留（daemon 无机会清理）
        repro_assert(file_exists($pidFile), 'kill -9 后 pid 文件应残留（stale）');
        echo "  stale pid 文件保留: {$pidFile}\n";
    });

    // 等 1s 确保进程完全退出 + DB 文件句柄释放
    sleep(1);

    repro_step('重启 daemon（stale pid 清理 + SQLite quick_check 通过）', function () use ($service, $dataDir) {
        $svc = new XhjobService($service, $dataDir);
        // xhjob_start 应清理 stale pid（starttime 不匹配）并启动新 daemon
        $ok = xhjob_start($service, $dataDir);
        repro_assert($ok, 'xhjob_start 返回 false（stale pid 未清理或 DB 损坏）');
        $svc->wait(10, true);
        $h = $svc->healthCheck();
        repro_assert($h['healthy'], '重启后 daemon 不健康（DB 可能损坏）: ' . json_encode($h));
        echo "  新 daemon pid=" . $h['pid'] . "\n";
    });

    repro_step('cron 任务从 DB 恢复（xhjob_get 返回非 null）', function () use ($mgr, $taskId) {
        $task = $mgr->get($taskId);
        repro_assert($task !== null, '重启后 xhjob_get 返回 null（任务未恢复）');
        repro_assert(($task['cron'] ?? null) === '* * * * *', "cron 表达式应保留，实际: " . ($task['cron'] ?? '?'));
        $state = $task['state'] ?? '?';
        echo "  state={$state} cron=" . ($task['cron'] ?? '?') . "\n";
        // 恢复后任务应为 pending（等待下次 cron 触发）或 running（恰好在触发）
        repro_assert(
            in_array($state, ['pending', 'running', 'success'], true),
            "恢复后任务应为 pending/running/success，实际: {$state}"
        );
    });

    // 辅助断言：SQLite quick_check 通过（DB 一致性）
    repro_step('SQLite quick_check 通过（DB 未因 SIGKILL 损坏）', function () use ($dbPath) {
        $cmd = 'sqlite3 ' . escapeshellarg($dbPath) . ' "PRAGMA quick_check;" 2>&1';
        $out = trim((string) shell_exec($cmd));
        echo "  quick_check: {$out}\n";
        repro_assert($out === 'ok', "quick_check 应返回 ok，实际: {$out}");
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    stop_daemon($service, $dataDir);
    cleanup_data_dir($dataDir);
}

repro_summary();
