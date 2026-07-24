<?php
// +----------------------------------------------------------------------
// | Xhjob 复现脚本公共启动逻辑（Task 9 / SubTask 9.2）
// +----------------------------------------------------------------------
// | 提供：
// |   - 扩展加载（dl() 兜底）
// |   - PHP_INI_SCAN_DIR 注入（复用 xhjob_server.php 的 setupExtensionScanDir 逻辑）
// |   - start_daemon($service, $dataDir) 启动 daemon 并等待 READY
// |   - stop_daemon($service, $dataDir) 停止 daemon
// |   - cleanup_data_dir($dataDir) 清理数据目录
// |   - repro_header($num, $title) 输出分割线
// |   - repro_step($name, $cond, $detail='') 断言函数，输出 [PASS] / [FAIL] / [SKIP]
// |   - repro_assert($cond, $msg) 断言失败时抛出异常
// |   - repro_summary() 输出单脚本汇总（runner.php 解析此行统计）
// |
// | 用法（每个 repro 脚本顶部）：
// |   require __DIR__ . '/_bootstrap.php';
// |   repro_header(1, '任务假死 watchdog 检测');
// |   $pid = start_daemon('repro01', '/tmp/xhjob-repro01');
// |   repro_step('daemon 启动', $pid > 0);
// |   ...
// |   stop_daemon('repro01', '/tmp/xhjob-repro01');
// |   repro_summary();
// +----------------------------------------------------------------------

require __DIR__ . '/../vendor/autoload.php';

// ======================================================================
// 扩展加载兜底
// ======================================================================
if (!extension_loaded('xhjob')) {
    $candidates = [
        '/workspace/target/release/libxhjob.so',
        '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so',
    ];
    foreach ($candidates as $so) {
        if (file_exists($so) && function_exists('dl')) {
            @dl($so);
            if (extension_loaded('xhjob')) {
                break;
            }
        }
    }
}

use Xhjob\XhjobService;

// ======================================================================
// PHP_INI_SCAN_DIR 注入（与 xhjob_server.php 一致）
// ======================================================================
/**
 * 注入 PHP_INI_SCAN_DIR，让 daemon 子进程通过 ini scan dir 加载 xhjob 扩展。
 *
 * daemon 子进程通过 re-exec 当前 PHP binary + `-d extension=xhjob.so` 启动。
 * `-d extension=xhjob.so` 仅在 extension_dir 中查找，若不存在则失败。
 * 通过 PHP_INI_SCAN_DIR 注入临时 ini 文件（内容为 extension=<so 完整路径>），
 * PHP 启动时会扫描该目录并加载完整路径的扩展。
 */
function setupExtensionScanDir(): void
{
    $so = '/workspace/target/release/libxhjob.so';
    if (!file_exists($so)) {
        $alt = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
        if (file_exists($alt)) {
            $so = $alt;
        }
    }
    $tmpDir = sys_get_temp_dir() . '/xhjob_ini_scan_' . posix_getpid();
    if (!is_dir($tmpDir)) {
        @mkdir($tmpDir, 0700, true);
    }
    file_put_contents($tmpDir . '/xhjob.ini', "extension={$so}\n");
    putenv('PHP_INI_SCAN_DIR=' . $tmpDir);
}

// 进程启动时立即注入（daemon 子进程依赖此 env var）
setupExtensionScanDir();

// ======================================================================
// daemon 启停 helper
// ======================================================================

/**
 * 启动 daemon 并等待 READY。
 *
 * 流程（复用 xhjob_server.php 模式）：
 *   1. ensureStopped 清理上一次残留 daemon
 *   2. rm -rf + mkdir 清理数据目录
 *   3. xhjob_start 启动 daemon
 *   4. 等待 running + 健康检查
 *
 * @param string $service  服务名（如 repro01）
 * @param string $dataDir  数据目录（如 /tmp/xhjob-repro01）
 * @return int daemon pid（>0 表示成功）
 * @throws \Exception 启动失败时抛出
 */
function start_daemon(string $service, string $dataDir): int
{
    $svc = new XhjobService($service, $dataDir);
    // 1. 停止上一次残留 daemon（pid 文件可能在 dataDir 内，rm 前先停）
    $svc->ensureStopped();
    // 2. 清理数据目录
    @system('rm -rf ' . escapeshellarg($dataDir));
    if (!is_dir($dataDir)) {
        @mkdir($dataDir, 0777, true);
    }
    // 3. 启动 daemon
    $pid = $svc->start();
    if ($pid <= 0) {
        throw new \Exception("daemon 启动失败 pid={$pid} service={$service}");
    }
    // 4. 等待就绪 + 健康检查
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    if (!$h['healthy']) {
        $svc->ensureStopped();
        throw new \Exception("daemon 不健康: " . json_encode($h));
    }
    return $pid;
}

/**
 * 停止 daemon（graceful SIGTERM → 等 drain → reap zombie）。
 *
 * 不直接调用 xhjob_stop：xhjob_stop 内部的 send_terminate 会轮询
 * is_process_alive_with_starttime，而 zombie 进程的 kill(pid,0)=0，
 * 导致 35s 超时后才返回。此处直接发 SIGTERM + 显式 reap zombie，
 * 避免 35s 等待，同时仍允许 daemon 优雅 drain。
 */
function stop_daemon(string $service, string $dataDir): void
{
    $svc = new XhjobService($service, $dataDir);
    $status = $svc->status();
    if (!$status['running'] || $status['pid'] === null) {
        return;
    }
    $daemonPid = (int) $status['pid'];

    // SIGTERM 触发 daemon 优雅关闭（drain in-flight tasks）
    posix_kill($daemonPid, 15);

    // 等待 daemon 退出（最长 15s drain），检测 zombie 或进程消失
    $deadline = microtime(true) + 15;
    while (microtime(true) < $deadline) {
        if (!posix_kill($daemonPid, 0)) {
            break; // 进程已完全消失（被 init reap）
        }
        if (is_zombie($daemonPid)) {
            break; // daemon 已退出变 zombie，等待 reap
        }
        usleep(100000);
    }

    // 若仍存活（未退出也未 zombie），SIGKILL 强制终止
    if (posix_kill($daemonPid, 0) && !is_zombie($daemonPid)) {
        posix_kill($daemonPid, 9);
        usleep(300000);
    }

    // Reap zombie（测试环境中 daemon 是当前 PHP 进程的子进程）
    reap_process($daemonPid);
}

/**
 * 清理数据目录（测试结束后调用）。
 */
function cleanup_data_dir(string $dataDir): void
{
    @system('rm -rf ' . escapeshellarg($dataDir));
}

/**
 * 检查进程是否为 zombie（Linux: /proc/<pid>/stat state='Z'）。
 *
 * zombie 进程的 kill(pid,0) 返回 0（看似存活），但实际已退出，等待父进程 reap。
 * 在测试环境中 daemon 是 PHP 进程的子进程（spawn_via_double_fork 仅 setsid
 * 未真正 double-fork），kill -9/SIGTERM 后变 zombie，需要显式 reap。
 */
function is_zombie(int $pid): bool
{
    $statFile = "/proc/{$pid}/stat";
    if (!is_file($statFile)) {
        return false;
    }
    $stat = @file_get_contents($statFile);
    if ($stat === false) {
        return false;
    }
    // /proc/<pid>/stat: "pid (comm) state ..." — state 紧跟最后一个 ')' 后
    $afterComm = strrchr($stat, ')');
    if ($afterComm === false) {
        return false;
    }
    $state = trim(substr($afterComm, 1, 2));
    return $state === 'Z';
}

/**
 * Reap zombie 子进程。
 *
 * 测试环境中 daemon 是当前 PHP 进程的子进程，kill -9/SIGTERM 后变 zombie。
 * kill(pid,0) 对 zombie 返回 0 导致 is_process_alive 误判存活，
 * 需要显式 pcntl_waitpid reap。生产环境中 daemon 被 init(PID 1) 收养并自动 reap。
 */
function reap_process(int $pid): void
{
    if ($pid <= 0 || !function_exists('pcntl_waitpid')) {
        return;
    }
    $status = 0;
    // WNOHANG=1: 非阻塞。返回 $pid=已reap, 0=仍在运行, -1=错误/非子进程
    $r = pcntl_waitpid($pid, $status, 1);
    if ($r == 0) {
        // 仍是 zombie，阻塞等待 reap
        pcntl_waitpid($pid, $status, 0);
    }
    usleep(200000); // 等 /proc/<pid> 清理
}

/**
 * 清理 ini scan dir（脚本退出前调用）。
 */
function cleanup_ini_scan_dir(): void
{
    $iniDir = sys_get_temp_dir() . '/xhjob_ini_scan_' . posix_getpid();
    if (is_dir($iniDir)) {
        @unlink($iniDir . '/xhjob.ini');
        @rmdir($iniDir);
    }
}

// 注册 shutdown 函数：确保脚本异常退出时清理 ini scan dir
register_shutdown_function(function () {
    cleanup_ini_scan_dir();
});

// ======================================================================
// 断言与输出 helper
// ======================================================================

$GLOBALS['_repro_pass'] = 0;
$GLOBALS['_repro_fail'] = 0;
$GLOBALS['_repro_skip'] = 0;
$GLOBALS['_repro_failed_steps'] = [];

/**
 * 输出分割线 + 测试标题。
 *
 * @param int    $num   测试编号（如 1）
 * @param string $title 测试标题
 */
function repro_header(int $num, string $title): void
{
    $line = str_repeat('=', 70);
    echo "\n{$line}\n";
    echo sprintf("[repro_%02d] %s\n", $num, $title);
    echo "{$line}\n";
}

/**
 * 断言函数，输出 [PASS] / [FAIL] / [SKIP]。
 *
 * $cond 支持两种形式：
 *   - bool：true → PASS，false → FAIL（附带 $detail）
 *   - callable：调用之；抛异常 → FAIL；返回 'SKIP' → SKIP；其他 → PASS
 *
 * @param string        $name   步骤名
 * @param bool|callable $cond   断言条件或返回条件的可调用对象
 * @param string        $detail 失败时的附加说明
 */
function repro_step(string $name, $cond, string $detail = ''): void
{
    global $_repro_pass, $_repro_fail, $_repro_skip, $_repro_failed_steps;
    if (is_callable($cond)) {
        try {
            $r = $cond();
            if ($r === 'SKIP') {
                $_repro_skip++;
                echo "[SKIP] {$name}" . ($detail !== '' ? " — {$detail}" : "") . "\n";
                return;
            }
            if ($r === false) {
                $_repro_fail++;
                $_repro_failed_steps[] = $name;
                echo "[FAIL] {$name}" . ($detail !== '' ? " — {$detail}" : "") . "\n";
                return;
            }
            $_repro_pass++;
            echo "[PASS] {$name}\n";
        } catch (\Throwable $e) {
            $_repro_fail++;
            $_repro_failed_steps[] = $name;
            $msg = $e->getMessage();
            echo "[FAIL] {$name} — {$msg}" . ($detail !== '' ? " ({$detail})" : "") . "\n";
        }
        return;
    }
    if ($cond === 'SKIP') {
        $_repro_skip++;
        echo "[SKIP] {$name}" . ($detail !== '' ? " — {$detail}" : "") . "\n";
        return;
    }
    if ($cond) {
        $_repro_pass++;
        echo "[PASS] {$name}\n";
    } else {
        $_repro_fail++;
        $_repro_failed_steps[] = $name;
        echo "[FAIL] {$name}" . ($detail !== '' ? " — {$detail}" : "") . "\n";
    }
}

/**
 * 断言失败时抛出异常（供 callable 形式的 repro_step 内部使用）。
 *
 * @param bool   $cond 条件
 * @param string $msg  失败信息
 * @throws \Exception
 */
function repro_assert(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new \Exception($msg);
    }
}

/**
 * 输出单脚本汇总。runner.php 解析此行统计 PASS/FAIL。
 */
function repro_summary(): void
{
    global $_repro_pass, $_repro_fail, $_repro_skip, $_repro_failed_steps;
    $failed = empty($_repro_failed_steps) ? '' : ' | failed: [' . implode(', ', $_repro_failed_steps) . ']';
    echo sprintf(
        "=== repro: %d PASS / %d FAIL / %d SKIP%s ===\n",
        $_repro_pass,
        $_repro_fail,
        $_repro_skip,
        $failed
    );
}
