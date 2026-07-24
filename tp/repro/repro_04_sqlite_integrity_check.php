<?php
// +----------------------------------------------------------------------
// | repro_04: SQLite 损坏检测（Task 10 / SubTask 10.4）
// +----------------------------------------------------------------------
// | 场景：persist daemon 写入任务后停止，破坏 DB 文件中部字节，
// |       尝试重启 daemon。
// |
// | 预期（Task 4 修复后）：
// |   1. SqliteStore::open 在 schema 创建后执行 PRAGMA quick_check
// |   2. 损坏 DB → quick_check 返回非 "ok" → 返回 store error
// |   3. daemon 启动失败（xhjob_start 返回 false 或 daemon 不健康 / 立即退出）
// |
// | 注意：DB 路径为 {dataDir}/xhjob.{service}.db（db_path_for 实现）。
// |       破坏文件中部 b-tree 页即可触发 quick_check 失败。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$service = 'repro04';
$dataDir = '/tmp/xhjob-repro04';
$dbPath = $dataDir . '/xhjob.' . $service . '.db';

repro_header(4, 'SQLite 损坏检测（PRAGMA quick_check）');

putenv('XHJOB_PERSIST=1');

$pid = null;
$taskId = null;

try {
    repro_step('启动 persist daemon + 写入任务', function () use ($service, $dataDir, &$pid, &$taskId) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
        $mgr = new TaskManager($service, $dataDir);
        // 派发多个任务让 DB 增长到多页（>1 page），以便破坏非首页 b-tree 页
        for ($i = 0; $i < 15; $i++) {
            $mgr->create(
                TaskBuilder::shell('echo repro04-task-' . $i)->timeout(10)->withRetry(0, 0)
            );
        }
        $taskId = $mgr->create(
            TaskBuilder::shell('echo repro04-ok')->timeout(10)->withRetry(0, 0)
        );
        repro_assert(!empty($taskId) && strpos($taskId, 'error') === false, "dispatch 失败: {$taskId}");
        // 等最后一个任务完成
        $mgr->waitForState($taskId, 'success', 15);
    });

    repro_step('确认 DB 文件存在且非空', function () use ($dbPath) {
        repro_assert(file_exists($dbPath), "DB 文件不存在: {$dbPath}");
        repro_assert(filesize($dbPath) > 1024, "DB 文件过小: " . filesize($dbPath));
        echo "  db_size=" . filesize($dbPath) . "\n";
    });

    // 停止 daemon（保留 DB 文件）—— 用 stop_daemon 避免 ensureStopped 的 35s zombie 超时
    repro_step('停止 daemon（保留 DB 文件）', function () use ($service, $dataDir) {
        stop_daemon($service, $dataDir);
        $svc = new XhjobService($service, $dataDir);
        $st = $svc->status();
        repro_assert(!$st['running'], 'daemon 应已停止');
    });

    // 删除 WAL/SHM 侧车文件：WAL 中可能有页的干净副本，会掩盖主 DB 的损坏。
    // 删除后主 DB 文件是唯一数据源，破坏它才能被 quick_check 检测到。
    repro_step('删除 WAL/SHM 侧车文件', function () use ($dbPath) {
        foreach (['-wal', '-shm', '-journal'] as $suffix) {
            $sidecar = $dbPath . $suffix;
            if (file_exists($sidecar)) {
                @unlink($sidecar);
                echo "  删除 {$sidecar}\n";
            }
        }
        repro_assert(true, '');
    });

    // 破坏 DB 文件：用 PDO wal_checkpoint(TRUNCATE) 确保数据落盘后，破坏非首页。
    // 参考 Rust 测试 test_integrity_check_fails_on_corrupt_db：
    //   - 保留 page 1（header + sqlite_master）让 open/PRAGMA/CREATE TABLE 通过
    //   - 破坏 page 5+ 的 b-tree 页类型字节 → quick_check 检测到 "malformed"
    repro_step('破坏 DB 文件非首页 b-tree 页', function () use ($dbPath) {
        $size = filesize($dbPath);
        repro_assert($size > 100, "DB 文件过小无法破坏: {$size}");

        // 读取 page size（SQLite header bytes 16-17, big-endian；值 1 表示 65536）
        $fh = fopen($dbPath, 'rb');
        fseek($fh, 16);
        $psBytes = fread($fh, 2);
        fclose($fh);
        $psRaw = unpack('n', $psBytes)[1];
        $pageSize = $psRaw === 1 ? 65536 : max($psRaw, 512);

        $fh = fopen($dbPath, 'r+b');
        repro_assert($fh !== false, "无法打开 DB 文件: {$dbPath}");

        if ($size >= $pageSize * 6) {
            // DB 足够大：破坏 page 5 的前 8 字节（b-tree 页头），
            // 将页类型字节设为 0xFF（非法值），参考 Rust 测试
            $targetOffset = 5 * $pageSize;
            fseek($fh, $targetOffset);
            fwrite($fh, str_repeat("\xFF", 8));
            echo "  已破坏 page 5 offset={$targetOffset}（page_size={$pageSize}）\n";
        } else {
            // DB 较小：破坏 page 2+ 的内容（若存在），否则破坏 page 1 的 b-tree 区
            $targetOffset = $pageSize; // page 2 起点
            if ($size > $targetOffset + 8) {
                fseek($fh, $targetOffset);
                fwrite($fh, str_repeat("\xFF", 8));
                echo "  已破坏 page 2 offset={$targetOffset}（page_size={$pageSize}）\n";
            } else {
                // 只有 1 页：破坏 b-tree 页头（byte 100+），SQLite open 会失败
                fseek($fh, 100);
                fwrite($fh, str_repeat("\xFF", 8));
                echo "  已破坏 page 1 b-tree header offset=100（page_size={$pageSize}）\n";
            }
        }
        fclose($fh);
        repro_assert(true, '');
    });

    // 尝试重启 daemon —— 应失败
    repro_step('重启 daemon 失败（integrity check 拒绝启动）', function () use ($service, $dataDir) {
        $svc = new XhjobService($service, $dataDir);
        // xhjob_start 可能返回 true（fork 成功）但 daemon 因 open 失败立即退出
        // 也可能返回 false。两种情况都视为「启动被拒」。
        $startOk = @xhjob_start($service, $dataDir);
        // 给 daemon 一点时间退出（如果它 fork 成功但立即崩溃）
        usleep(1500000);
        $st = $svc->status();
        $healthy = $st['running'] && $st['pid'] !== null && $st['pid'] > 0;
        echo "  start=" . var_export($startOk, true) . " running=" . var_export($st['running'], true) . " pid=" . var_export($st['pid'], true) . "\n";
        // 关键断言：daemon 不应处于健康运行状态
        repro_assert(!$healthy, '损坏 DB 后 daemon 不应健康运行（integrity check 应拒绝启动）');
    });

    // 辅助断言：用 sqlite3 CLI 验证 DB 确实损坏
    repro_step('sqlite3 CLI 确认 DB 损坏（quick_check 非 ok）', function () use ($dbPath) {
        if (!file_exists('/usr/bin/sqlite3') && !@system('command -v sqlite3 >/dev/null 2>&1', $rc)) {
            return 'SKIP';
        }
        $cmd = 'sqlite3 ' . escapeshellarg($dbPath) . ' "PRAGMA quick_check;" 2>&1';
        $out = shell_exec($cmd);
        echo "  quick_check 输出: " . trim((string) $out) . "\n";
        // 健康 DB 返回 "ok"；损坏 DB 返回错误描述或非 ok 行
        repro_assert(
            stripos((string) $out, 'ok') === false || strpos(trim((string) $out), 'ok') !== 0,
            'quick_check 应返回非 ok（DB 已损坏），实际: ' . $out
        );
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    stop_daemon($service, $dataDir);
    cleanup_data_dir($dataDir);
}

repro_summary();
