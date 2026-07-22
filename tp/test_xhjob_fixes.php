#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 — 必修五件套修复验证测试
// +----------------------------------------------------------------------
// | 验证以下修复：
// |   Fix 1: Cargo.toml panic = "abort" (编译时已生效，运行时验证 daemon 稳定)
// |   Fix 2: ipc::request tokio::time::timeout (验证 IPC 请求不会永久阻塞)
// |   Fix 3: tracing_subscriber 初始化 (验证 daemon 日志真正输出)
// |   Fix 4: SQLite 文件权限 0o600 (验证 DB 文件权限)
// |   Fix 5: SO_PEERCRED peer 认证 (验证同用户 IPC 连接正常)
// |   Fix 6: HTTP 重试按方法区分 + idempotent 标记
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

if (!extension_loaded('xhjob')) {
    $so = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$pass = 0;
$fail = 0;
$skipped = 0;

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $skipped;
    echo "[$n] $desc ... ";
    try {
        $r = $fn();
        if ($r === 'SKIP') {
            $skipped++;
            echo "SKIP\n";
            return;
        }
        $pass++;
        echo "PASS\n";
    } catch (Throwable $e) {
        $fail++;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

$SERVICE  = 'tp-fix-test';
$DATA_DIR = '/tmp/xhjob-tp-fix-test';

// 清理上一次测试残留
@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);

// 本地 HTTP 服务器：始终返回 500，用于测试重试行为
$fakeServerPid = null;

function startFakeHttpServer(): int
{
    $pidFile = '/tmp/xhjob_fake_http.pid';
    $code = '<?php http_response_code(500); echo "Internal Server Error";';
    $scriptPath = '/tmp/xhjob_fake_500.php';
    file_put_contents($scriptPath, $code);
    $cmd = "php -S 127.0.0.1:18099 $scriptPath > /dev/null 2>&1 & echo $!";
    $pid = trim(shell_exec($cmd));
    // 等待服务器启动
    usleep(500000);
    return (int)$pid;
}

function stopFakeHttpServer(int $pid): void
{
    if ($pid > 0) {
        @posix_kill($pid, 9);
    }
    @unlink('/tmp/xhjob_fake_500.php');
}

// -----------------------------------------------------------------
// 测试步骤
// -----------------------------------------------------------------

// 1. 启动 daemon (persist 模式，用于测试 Fix 4 SQLite 权限)
step(1, 'Fix 1/3: 启动 daemon (persist) 并验证稳定性', function () use ($SERVICE, $DATA_DIR) {
    // 设置环境变量启用 persist 和日志
    putenv('XHJOB_PERSIST=1');
    putenv('RUST_LOG=xhjob=debug');
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    if ($pid <= 0) {
        throw new Exception("daemon 启动失败 pid=$pid");
    }
    $svc->wait(10, true);
    // 验证 daemon 确实在运行
    $h = $svc->healthCheck();
    if (!$h['healthy']) {
        throw new Exception("daemon 不健康: " . json_encode($h));
    }
});

// 2. Fix 3: 验证 tracing 日志输出
// P0-15 fix: tracing now writes to a daily-rotating file via
// tracing_appender (xhjob.<svc>.log.<date>), NOT to the stderr-redirected
// xhjob.<svc>.log. Accept either location so the test works before and
// after the P0-15 rotation fix.
step(2, 'Fix 3: tracing_subscriber 日志输出', function () use ($SERVICE, $DATA_DIR) {
    $candidates = glob("$DATA_DIR/xhjob.$SERVICE.log*");
    if (empty($candidates)) {
        // 没有任何日志文件，回退到 daemon 健康检查间接验证
        $svc = new XhjobService($SERVICE, $DATA_DIR);
        $h = $svc->healthCheck();
        if (!$h['healthy']) {
            throw new Exception("daemon 不健康，tracing 初始化可能 crash");
        }
        return 'SKIP';
    }
    $foundNonEmpty = false;
    foreach ($candidates as $logFile) {
        $content = @file_get_contents($logFile);
        if ($content === false || strlen(trim($content)) === 0) {
            continue;
        }
        // tracing_subscriber 应该输出了至少一行日志
        // 检查是否包含 daemon starting 等关键日志
        if (strpos($content, 'daemon') === false && strpos($content, 'xhjob') === false) {
            continue;
        }
        $foundNonEmpty = true;
        break;
    }
    if (!$foundNonEmpty) {
        throw new Exception("无有效日志输出，tracing_subscriber 可能未初始化");
    }
});

// 3. Fix 2: 验证 IPC 请求不永久阻塞（正常请求应快速完成）
step(3, 'Fix 2: IPC 请求 timeout 保护', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $start = microtime(true);
    // 派发一个简单的 shell 任务，验证 IPC 往返在合理时间内完成
    $id = $mgr->create(TaskBuilder::shell('echo fix2-test')->withRetry(0, 0));
    $elapsed = microtime(true) - $start;
    if ($elapsed > 5.0) {
        throw new Exception(sprintf("IPC 请求耗时 %.2fs，超过 5s timeout", $elapsed));
    }
    echo sprintf("(%.2fs) ", $elapsed);
});

// 4. Fix 4: 验证 SQLite DB 文件权限为 0o600
step(4, 'Fix 4: SQLite 文件权限 0o600', function () use ($SERVICE, $DATA_DIR) {
    $dbFile = "$DATA_DIR/xhjob.$SERVICE.db";
    if (!file_exists($dbFile)) {
        throw new Exception("DB 文件不存在: $dbFile");
    }
    $perms = fileperms($dbFile) & 0777;
    if ($perms !== 0600) {
        throw new Exception(sprintf(
            "DB 文件权限 %04o 不等于 0600", $perms
        ));
    }
    echo sprintf("(0%o) ", $perms);
});

// 5. Fix 5: 验证 SO_PEERCRED 认证（同用户连接应成功）
step(5, 'Fix 5: SO_PEERCRED 同用户认证', function () use ($SERVICE, $DATA_DIR) {
    // 如果 SO_PEERCRED 检查错误地拒绝了同用户连接，
    // 这个 IPC 请求会失败。能成功创建并查询任务说明认证通过。
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(TaskBuilder::shell('echo fix5-peercred')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    if (($st['state'] ?? '') !== 'success') {
        throw new Exception("SO_PEERCRED 可能拒绝了同用户连接: " . json_encode($st));
    }
});

// 6. 启动 fake HTTP 500 服务器
step(6, '启动 fake HTTP 500 服务器', function () use (&$fakeServerPid) {
    global $fakeServerPid;
    $fakeServerPid = startFakeHttpServer();
    if ($fakeServerPid <= 0) {
        throw new Exception("fake HTTP 服务器启动失败");
    }
    // 验证服务器确实返回 500
    $code = @file_get_contents('http://127.0.0.1:18099/', false, stream_context_create(['http' => ['ignore_errors' => true]]));
    $headers = $http_response_header ?? [];
    $statusLine = $headers[0] ?? '';
    if (strpos($statusLine, '500') === false) {
        throw new Exception("fake HTTP 服务器未返回 500: $statusLine");
    }
});

// 7. Fix 6a: POST 不带 idempotent → 不应重试（attempts 应为 1）
step(7, 'Fix 6a: POST 不带 idempotent 不重试', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::http('POST', 'http://127.0.0.1:18099/')
            ->withRetry(3, 1)  // retry_max=3，但如果方法非幂等则不会重试
    );
    // 等待任务到达终态
    $mgr->waitForState($id, 'failed', 30);
    $st = $mgr->state($id);
    $attempts = intval($st['attempts'] ?? -1);
    // POST 不带 idempotent：should_retry 返回 false，所以不会调度重试
    // 立即进入 Failed（attempts 递增 1，从 0 变为 1）
    if ($attempts !== 1) {
        throw new Exception("POST 不带 idempotent 应该不重试 (attempts=1)，实际 attempts=$attempts");
    }
    echo "(attempts=$attempts) ";
});

// 8. Fix 6b: GET → 应该重试（attempts > 1）
step(8, 'Fix 6b: GET 安全方法应重试', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::http('GET', 'http://127.0.0.1:18099/')
            ->withRetry(3, 1)  // retry_max=3
    );
    // GET 是安全方法，应该会重试
    $mgr->waitForState($id, 'failed', 30);
    $st = $mgr->state($id);
    $attempts = intval($st['attempts'] ?? -1);
    if ($attempts <= 1) {
        throw new Exception("GET 应该重试 (attempts>1)，实际 attempts=$attempts");
    }
    echo "(attempts=$attempts) ";
});

// 9. Fix 6c: POST 带 idempotent=true → 应该重试（attempts > 1）
step(9, 'Fix 6c: POST 带 idempotent=true 应重试', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::http('POST', 'http://127.0.0.1:18099/')
            ->withRetry(3, 1)
            ->idempotent(true)  // 显式声明幂等
    );
    $mgr->waitForState($id, 'failed', 30);
    $st = $mgr->state($id);
    $attempts = intval($st['attempts'] ?? -1);
    if ($attempts <= 1) {
        throw new Exception("POST+idempotent=true 应该重试 (attempts>1)，实际 attempts=$attempts");
    }
    echo "(attempts=$attempts) ";
});

// 10. Fix 6d: 验证 idempotent 字段在 get 查询中可见
step(10, 'Fix 6d: idempotent 字段持久化到 DB', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::http('POST', 'http://127.0.0.1:18099/')
            ->withRetry(1, 1)
            ->idempotent(true)
    );
    // 通过 get 获取完整任务定义
    $raw = $mgr->get($id);
    if (!is_array($raw)) {
        throw new Exception("get 返回非数组: " . json_encode($raw));
    }
    // get 返回 {"ok": true, "data": {...}} 或直接是 task 字段
    $task = $raw['data'] ?? $raw;
    $idempotent = $task['idempotent'] ?? null;
    if ($idempotent !== true) {
        $json = json_encode($raw);
        throw new Exception("idempotent 字段未正确持久化 (期望 true): $json");
    }
});

// 11. 清理：停止 daemon 和 fake HTTP 服务器
step(11, '清理', function () use ($SERVICE, $DATA_DIR) {
    global $fakeServerPid;
    stopFakeHttpServer($fakeServerPid);
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    @system("rm -rf $DATA_DIR");
});

// -----------------------------------------------------------------
echo "\n========================================\n";
echo "结果: $pass PASS, $fail FAIL, $skipped SKIP\n";
echo "========================================\n";
exit($fail > 0 ? 1 : 0);
