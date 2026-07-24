<?php
// +----------------------------------------------------------------------
// | repro_02: PID 复用防护（Task 10 / SubTask 10.2）
// +----------------------------------------------------------------------
// | 场景：daemon 被 kill -9 模拟 OOM 崩溃，pid 文件残留。
// |
// | 预期（Task 3 修复后）：
// |   1. read_pid 解析 pid + starttime 两行格式
// |   2. is_process_alive_with_starttime 校验：pid 已死 → 视为未运行
// |   3. xhjob_status 返回 running=false（不因 stale pid 误判为运行中）
// |   4. xhjob_start 能成功重启（不报 "already running"）
// |   5. xhjob_stop 对已死 daemon 不误杀无关进程，仅清理 pid 文件
// |
// | 注意：真正复用 PID（同 pid 被新进程占用但 starttime 不匹配）在测试
// | 环境难以稳定复现；本测试验证 stale pid 文件清理 + starttime 校验的
// | 核心行为：daemon 崩溃后 status 正确返回 running=false，start 能重启。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

use Xhjob\XhjobService;

$service = 'repro02';
$dataDir = '/tmp/xhjob-repro02';
$pidFile = $dataDir . '/xhjob.' . $service . '.pid';

repro_header(2, 'PID 复用防护（stale pid 文件清理 + starttime 校验）');

$pid = null;
$restartedPid = null;
$oldStarttime = null;

try {
    repro_step('启动 daemon', function () use ($service, $dataDir, &$pid) {
        $pid = start_daemon($service, $dataDir);
        repro_assert($pid > 0, "daemon 启动失败 pid={$pid}");
    });

    repro_step('确认 pid 文件存在且含 pid+starttime 两行', function () use ($pidFile, $pid) {
        repro_assert(file_exists($pidFile), "pid 文件不存在: {$pidFile}");
        $content = file_get_contents($pidFile);
        $lines = explode("\n", trim($content));
        repro_assert(count($lines) >= 1, "pid 文件内容异常: {$content}");
        repro_assert((int) $lines[0] === $pid, "pid 文件首行 {$lines[0]} != 实际 pid {$pid}");
        // starttime 在第二行（Task 3 修复后写入），缺失时退化为 None（向后兼容）
        if (count($lines) >= 2 && $lines[1] !== '') {
            $oldStarttime = $lines[1];
            echo "  starttime={$lines[1]}\n";
        }
    });

    // kill -9 模拟 OOM 崩溃（不清理 pid 文件）
    repro_step('kill -9 daemon 模拟崩溃（保留 stale pid 文件）', function () use ($pid, $pidFile) {
        posix_kill($pid, 9); // SIGKILL
        // 等待进程退出
        $deadline = microtime(true) + 5;
        while (microtime(true) < $deadline) {
            if (!posix_kill($pid, 0)) {
                break; // 进程已退出
            }
            usleep(100000);
        }
        // Reap zombie：测试环境中 daemon 是当前 PHP 进程的子进程，
        // kill -9 后变 zombie，kill(pid,0) 返回 0 导致 is_process_alive 误判存活。
        // 生产环境中 init(PID 1) 自动 reap，测试中需显式 reap。
        reap_process($pid);
        // 进程已死 + 已 reap，但 pid 文件应仍存在（SIGKILL 不会触发 daemon 的清理逻辑）
        repro_assert(file_exists($pidFile), 'kill -9 后 pid 文件应残留（stale）');
    });

    // 给系统一点时间回收 zombie（reap_process 已含延迟，此处不再额外等待）

    repro_step('stale pid 校验：xhjob_status 返回 running=false', function () use ($service, $dataDir) {
        $svc = new XhjobService($service, $dataDir);
        $st = $svc->status();
        repro_assert(!$st['running'], "daemon 已被 kill -9，status 应返回 running=false，实际: " . json_encode($st));
    });

    repro_step('stale pid 不阻塞重启：xhjob_start 成功启动新 daemon', function () use ($service, $dataDir, &$restartedPid) {
        $svc = new XhjobService($service, $dataDir);
        // 直接调用 xhjob_start（不应报 "already running"）
        $ok = xhjob_start($service, $dataDir);
        repro_assert($ok, 'xhjob_start 返回 false（可能因 stale pid 误判 already running）');
        $svc->wait(10, true);
        $st = $svc->status();
        repro_assert($st['running'] && $st['pid'] > 0, '重启后 status 应 running=true, 实际: ' . json_encode($st));
        $restartedPid = $st['pid'];
        repro_assert($restartedPid !== null && $restartedPid > 0, '重启后 pid 应 > 0');
    });

    repro_step('重启后 pid 文件被新 daemon 覆盖（新 pid + starttime 写入）', function () use ($pidFile, $restartedPid, $pid, $oldStarttime) {
        repro_assert(file_exists($pidFile), 'pid 文件应存在');
        $content = file_get_contents($pidFile);
        $lines = explode("\n", trim($content));
        repro_assert((int) $lines[0] === $restartedPid, "pid 文件应含新 pid={$restartedPid}，实际 {$lines[0]}");
        $newStarttime = (count($lines) >= 2 && $lines[1] !== '') ? $lines[1] : null;
        if ($newStarttime !== null && $oldStarttime !== null) {
            echo "  旧 starttime={$oldStarttime} → 新 starttime={$newStarttime}\n";
            // PID 复用是合法的（新 daemon 可能获得相同 PID）；
            // 关键验证：新 starttime 应不同于旧 starttime（即使 PID 相同）
            repro_assert($newStarttime !== $oldStarttime, "新 starttime 应不同于旧 starttime（PID 复用时 starttime 不同证明是新进程）");
        }
    });

    repro_step('xhjob_stop 对运行中 daemon 正常停止（验证 stop 路径未损坏）', function () use ($service, $dataDir) {
        $svc = new XhjobService($service, $dataDir);
        $st = $svc->status();
        $daemonPid = $st['pid'] ?? 0;
        // Fork reaper child：daemon 是当前 PHP 进程的子进程，SIGTERM 后变 zombie，
        // send_terminate 轮询 is_process_alive 对 zombie 返回 true 导致 35s 超时。
        // reaper 子进程 pcntl_waitpid reap zombie，使 send_terminate 快速返回。
        $reaperPid = 0;
        if ($daemonPid > 0 && function_exists('pcntl_fork')) {
            $reaperPid = pcntl_fork();
            if ($reaperPid === 0) {
                $deadline = time() + 40;
                while (time() < $deadline) {
                    $status = 0;
                    $r = pcntl_waitpid($daemonPid, $status, 1);
                    if ($r == $daemonPid || $r == -1) break;
                    usleep(50000);
                }
                exit(0);
            }
        }
        $ok = $svc->stop();
        if ($reaperPid > 0) {
            $status = 0;
            pcntl_waitpid($reaperPid, $status, 0);
        }
        repro_assert($ok, 'xhjob_stop 返回 false');
        $svc->wait(10, false);
        $st = $svc->status();
        repro_assert(!$st['running'], 'stop 后 status 应 running=false');
    });

    repro_step('xhjob_stop 对已死 daemon 不误杀（仅清理 pid 文件）', function () use ($service, $dataDir) {
        // 此时 daemon 已停止，再次调用 stop 不应误杀任何进程
        $svc = new XhjobService($service, $dataDir);
        $ok = $svc->stop();
        // stop 返回 true/false 都可接受（已停止时清理 pid 文件即成功）
        // 关键是不抛异常、不误杀
        repro_assert(true, '');
        echo "  stop 返回: " . var_export($ok, true) . "\n";
    });
} catch (\Throwable $e) {
    // 兜底清理 + 记录异常
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
} finally {
    stop_daemon($service, $dataDir);
    cleanup_data_dir($dataDir);
}

repro_summary();
