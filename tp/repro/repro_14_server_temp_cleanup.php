<?php
// +----------------------------------------------------------------------
// | repro_14: server 启动时清理过期 ini scan dir（防止 /tmp 无限累积）
// +----------------------------------------------------------------------
// | 场景：xhjob_server.php 的 setupExtensionScanDir() 每次运行都会在
// |       sys_get_temp_dir() 下创建 xhjob_ini_scan_<pid>/xhjob.ini，原实现
// |       不清理历史残留，导致 /tmp 无限累积（每次 cron / 重启留一个孤儿目录）。
// |
// | 修复：setupExtensionScanDir() 启动时扫描并删除 mtime 距今 > 3600s 的
// |       xhjob_ini_scan_* 目录（仅删过期目录，不删当前运行目录，避免破坏
// |       正在运行的 daemon）。
// |
// | 验证：
// |   1. 手动创建一个过期（mtime 2 小时前）的 xhjob_ini_scan_* 目录
// |   2. 以子进程运行 xhjob_server.php（触发启动清理）
// |   3. 断言过期目录已被删除
// |   4. 断言当前运行的 temp dir 未被删除（启动清理只删过期目录）
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

$service = 'repro14';
$dataDir = '/tmp/xhjob-repro14';
$serverScript = __DIR__ . '/../xhjob_server.php';
$so = '/workspace/target/release/libxhjob.so';
$php = PHP_BINARY;

// 过期目录唯一标识，避免与真实 xhjob_ini_scan_<pid> 冲突
$staleMarker = 'stale_repro14_' . substr(md5(uniqid('', true)), 0, 8);
$staleDir = sys_get_temp_dir() . '/xhjob_ini_scan_' . $staleMarker;

repro_header(14, 'server 启动时清理过期 ini scan dir（防止 /tmp 无限累积）');

// repro 自身 PID（_bootstrap 已为其创建 xhjob_ini_scan_<pid>，需在比对/清理时排除）
$reproPid = posix_getpid();

try {
    repro_step('创建过期 ini scan dir（mtime 2 小时前）', function () use ($staleDir) {
        @mkdir($staleDir, 0700, true);
        repro_assert(is_dir($staleDir), "无法创建 stale dir: {$staleDir}");
        file_put_contents($staleDir . '/xhjob.ini', "extension=dummy\n");
        // 写入 ini 会更新目录 mtime，故写入后再 touch 目录为 2 小时前
        shell_exec('touch -d "2 hours ago" '
            . escapeshellarg($staleDir) . ' '
            . escapeshellarg($staleDir . '/xhjob.ini'));
        // mkdir/file_put_contents 已缓存目录 stat 为「现在」，touch 通过 shell 修改
        // 了真实 mtime 但 PHP stat 缓存未更新，需清缓存后读取
        clearstatcache(true, $staleDir);
        $age = time() - (int) @filemtime($staleDir);
        echo "  stale dir: {$staleDir} (age={$age}s)\n";
        repro_assert($age > 3600, "stale dir age 应 > 3600s，实际: {$age}s");
    });

    repro_step('过期 dir 存在', is_dir($staleDir), $staleDir);

    // 记录运行前（排除 repro 自身 PID 目录）的 ini scan dir 集合
    $beforeDirs = list_ini_scan_dirs($reproPid);

    repro_step('以子进程运行 xhjob_server.php 触发启动清理', function () use ($php, $so, $serverScript, $service, $dataDir) {
        $cmd = escapeshellarg($php)
            . ' -d extension=' . escapeshellarg($so)
            . ' ' . escapeshellarg($serverScript)
            . ' --service=' . escapeshellarg($service)
            . ' --data-dir=' . escapeshellarg($dataDir)
            . ' 2>&1';
        $out = shell_exec($cmd);
        echo "  server 输出: " . trim((string) $out) . "\n";
        repro_assert(stripos((string) $out, 'READY') !== false, "server 未输出 READY: {$out}");
    });

    // server 子进程（独立 PHP 进程）删除了 stale dir，但本进程 stat 缓存仍为
    // 旧值，需清缓存后再判断
    clearstatcache(true, $staleDir);
    repro_step('过期 dir 已被启动清理删除', !is_dir($staleDir), $staleDir);

    repro_step('当前 run 的 temp dir 未被删除（启动后仍存在）', function () use ($reproPid, $beforeDirs) {
        $afterDirs = list_ini_scan_dirs($reproPid);
        $newDirs = array_values(array_diff($afterDirs, $beforeDirs));
        echo "  运行后新增 ini scan dir: " . (empty($newDirs) ? '(无)' : implode(', ', $newDirs)) . "\n";
        repro_assert(!empty($newDirs), 'server 子进程的 temp dir 应存在（启动清理不应删除当前 dir）');
        return true;
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    // 停止 server 子进程启动的 daemon
    stop_daemon($service, $dataDir);
    cleanup_data_dir($dataDir);
    // 清理 server 子进程遗留的 ini scan dir（修复方案不在退出时删除当前 dir，
    // 故子进程目录会残留，测试需自行清理）
    cleanup_leftover_ini_scan_dirs($reproPid);
}

repro_summary();

// -----------------------------------------------------------------
// 辅助函数
// -----------------------------------------------------------------

/**
 * 列出 sys_get_temp_dir() 下所有 xhjob_ini_scan_* 目录，排除指定 PID 的目录。
 *
 * @return string[] 排序后的目录绝对路径列表
 */
function list_ini_scan_dirs(int $excludePid): array
{
    $pattern = sys_get_temp_dir() . '/xhjob_ini_scan_*';
    $dirs = glob($pattern, GLOB_ONLYDIR) ?: [];
    $result = [];
    foreach ($dirs as $d) {
        if (basename($d) === 'xhjob_ini_scan_' . $excludePid) {
            continue;
        }
        $result[] = $d;
    }
    sort($result);
    return $result;
}

/**
 * 清理 server 子进程遗留的 ini scan dir（排除 repro 自身 PID 目录，
 * 后者由 _bootstrap.php 的 shutdown 函数清理）。
 */
function cleanup_leftover_ini_scan_dirs(int $excludePid): void
{
    $pattern = sys_get_temp_dir() . '/xhjob_ini_scan_*';
    $dirs = glob($pattern, GLOB_ONLYDIR) ?: [];
    foreach ($dirs as $d) {
        if (basename($d) === 'xhjob_ini_scan_' . $excludePid) {
            continue;
        }
        @unlink($d . '/xhjob.ini');
        @rmdir($d);
    }
}
