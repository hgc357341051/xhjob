<?php
// +----------------------------------------------------------------------
// | repro_08: 外键约束（Task 10 / SubTask 10.8）
// +----------------------------------------------------------------------
// | 场景：persist daemon 写入 task + result 后停止，用 sqlite3 CLI / PDO
// |       尝试在 results 表插入 task_id='nonexistent' 的行。
// |
// | 预期（Task 4 修复后）：
// |   1. results 表有 FOREIGN KEY (task_id) REFERENCES tasks(id)
// |   2. PRAGMA foreign_keys=ON 后，插入不存在的 task_id 被拒绝
// |   3. sqlite3 / PDO 返回 "FOREIGN KEY constraint failed"
// |
// | 注意：foreign_keys 默认 OFF，必须显式 PRAGMA foreign_keys=ON 才生效。
// |       daemon 内部已开启，但外部 CLI/PDO 连接需自行开启。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$service = 'repro08';
$dataDir = '/tmp/xhjob-repro08';
$dbPath = $dataDir . '/xhjob.' . $service . '.db';

repro_header(8, '外键约束（FOREIGN KEY (task_id) REFERENCES tasks(id)）');

putenv('XHJOB_PERSIST=1');

$pid = null;
$taskId = null;

try {
    repro_step('启动 persist daemon + 派发任务', function () use ($service, $dataDir, &$pid, &$taskId) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
        $mgr = new TaskManager($service, $dataDir);
        $taskId = $mgr->create(
            TaskBuilder::shell('echo repro08-ok')->timeout(10)->withRetry(0, 0)
        );
        repro_assert(!empty($taskId), "dispatch 失败: {$taskId}");
        $mgr->waitForState($taskId, 'success', 10);
    });

    repro_step('确认 task + result 行存在', function () use ($dbPath, $taskId) {
        repro_assert(file_exists($dbPath), "DB 不存在: {$dbPath}");
        $state = read_scalar_from_db($dbPath, "SELECT state FROM tasks WHERE id='{$taskId}'");
        echo "  task state={$state}\n";
        repro_assert($state === 'success', "任务应为 success，实际: {$state}");
    });

    // 停止 daemon（释放 DB 文件锁）
    repro_step('停止 daemon', function () use ($service, $dataDir) {
        $svc = new XhjobService($service, $dataDir);
        $svc->ensureStopped();
        $st = $svc->status();
        repro_assert(!$st['running'], 'daemon 应已停止');
    });

    // 尝试插入不存在的 task_id（应被 FK 拒绝）
    repro_step('插入 nonexistent task_id 被 FK 拒绝（FOREIGN KEY constraint failed）', function () use ($dbPath) {
        $err = try_insert_nonexistent_result($dbPath);
        echo "  插入结果: " . trim($err) . "\n";
        repro_assert(
            stripos($err, 'FOREIGN KEY') !== false || stripos($err, 'constraint') !== false,
            "应返回 FOREIGN KEY constraint failed，实际: {$err}"
        );
    });

    // 辅助断言：插入已存在的 task_id 应成功（FK 不应误拒合法行）
    repro_step('插入已存在 task_id 成功（FK 不误拒合法行）', function () use ($dbPath, $taskId) {
        $err = try_insert_existing_result($dbPath, $taskId);
        echo "  插入结果: " . trim($err) . "\n";
        // 已存在 task_id 插入应成功（无错误），或因 PRIMARY KEY 冲突（task_id 已有 result 行）
        repro_assert(
            $err === '' || stripos($err, 'UNIQUE') !== false || stripos($err, 'PRIMARY KEY') !== false,
            "合法 task_id 插入应成功或仅因 UNIQUE 冲突，实际: {$err}"
        );
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    stop_daemon($service, $dataDir);
    cleanup_data_dir($dataDir);
}

repro_summary();

// -----------------------------------------------------------------
// 辅助函数
// -----------------------------------------------------------------

function read_scalar_from_db(string $dbPath, string $sql): string
{
    if (class_exists('PDO') && in_array('sqlite', PDO::getAvailableDrivers(), true)) {
        try {
            $pdo = new PDO('sqlite:' . $dbPath);
            $pdo->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
            return (string) $pdo->query($sql)->fetchColumn();
        } catch (\Throwable $e) {
            return 'error: ' . $e->getMessage();
        }
    }
    $cmd = 'sqlite3 ' . escapeshellarg($dbPath) . ' "' . str_replace('"', '\\"', $sql) . '" 2>&1';
    return trim((string) shell_exec($cmd));
}

function try_insert_nonexistent_result(string $dbPath): string
{
    // 优先 PDO（更可靠地开启 foreign_keys）
    if (class_exists('PDO') && in_array('sqlite', PDO::getAvailableDrivers(), true)) {
        try {
            $pdo = new PDO('sqlite:' . $dbPath);
            $pdo->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
            $pdo->exec('PRAGMA foreign_keys=ON');
            $pdo->exec("INSERT INTO results (task_id) VALUES ('nonexistent-repro08')");
            return ''; // 无异常 = 插入成功（FK 未生效，测试失败）
        } catch (\PDOException $e) {
            return $e->getMessage();
        } catch (\Throwable $e) {
            return $e->getMessage();
        }
    }
    // 回退 sqlite3 CLI
    $cmd = 'sqlite3 ' . escapeshellarg($dbPath)
        . ' "PRAGMA foreign_keys=ON; INSERT INTO results (task_id) VALUES (\'nonexistent-repro08\');" 2>&1';
    return trim((string) shell_exec($cmd));
}

function try_insert_existing_result(string $dbPath, string $taskId): string
{
    if (class_exists('PDO') && in_array('sqlite', PDO::getAvailableDrivers(), true)) {
        try {
            $pdo = new PDO('sqlite:' . $dbPath);
            $pdo->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
            $pdo->exec('PRAGMA foreign_keys=ON');
            // 先删已存在的 result 行（避免 PRIMARY KEY 冲突干扰 FK 验证）
            $pdo->exec("DELETE FROM results WHERE task_id='{$taskId}'");
            $pdo->exec("INSERT INTO results (task_id) VALUES ('{$taskId}')");
            return ''; // 成功
        } catch (\PDOException $e) {
            return $e->getMessage();
        } catch (\Throwable $e) {
            return $e->getMessage();
        }
    }
    $cmd = 'sqlite3 ' . escapeshellarg($dbPath)
        . ' "PRAGMA foreign_keys=ON; DELETE FROM results WHERE task_id=\'' . $taskId . '\'; INSERT INTO results (task_id) VALUES (\'' . $taskId . '\');" 2>&1';
    return trim((string) shell_exec($cmd));
}
