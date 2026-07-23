#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 — 生产环境业务模拟测试（ThinkPHP 8 集成）
// +----------------------------------------------------------------------
// | 模拟真实生产环境业务逻辑，对 xhjob 扩展进行端到端集成验证。
// | 覆盖 8 个核心场景：
// |   1. 订单异步处理（chain 流水线）
// |   2. 定时报表生成（cron + maxExecutions + progress）
// |   3. 批量数据 ETL（chord 汇总）
// |   4. 定时清理任务（cron + retry 退避）
// |   5. 通知发送（HTTP + idempotent + 重试）
// |   6. 延迟任务（countdown）
// |   7. 限流与并发控制（rateLimit + maxInstances）
// |   8. 持久化与崩溃恢复（daemon 重启恢复）
// +----------------------------------------------------------------------
// | 用法：
// |   php -d extension=/workspace/target/release/libxhjob.so \
// |       /workspace/tp/test_xhjob_production.php
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

// CLI 测试时手动加载 xhjob 扩展（已通过 -d extension 加载则跳过）
if (!extension_loaded('xhjob')) {
    $so = '/workspace/target/release/libxhjob.so';
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

/**
 * 测试步骤封装
 *
 * @param int       $n    步骤编号
 * @param string    $desc 描述
 * @param callable  $fn   测试闭包，返回 'SKIP' 表示跳过
 */
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

function assertTrue(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new Exception($msg);
    }
}

function assertEq($a, $b, string $msg): void
{
    if ($a !== $b) {
        throw new Exception("$msg (expected=" . var_export($b, true) . ", got=" . var_export($a, true) . ")");
    }
}

// ============================================================
// 环境配置
// ============================================================

$SERVICE   = 'tp-production';
$DATA_DIR  = '/tmp/xhjob-tp-production';
$HTTP_PORT = 18088;

// 启用持久化（崩溃恢复场景需要）
putenv('XHJOB_PERSIST=1');

/**
 * 设置 PHP_INI_SCAN_DIR，让 daemon 子进程能加载 xhjob 扩展。
 *
 * daemon 通过 `php -d extension=xhjob.so` 启动子进程（bare filename），
 * 子进程在默认 extension_dir 中找不到 xhjob.so。通过 PHP_INI_SCAN_DIR
 * 注入一个额外的 .ini 文件（含 extension=<完整路径>），让子进程正确加载。
 * putenv 设置的 env var 会被 Rust Command 继承到 daemon 子进程。
 */
function setupExtensionScanDir(): void
{
    $so = '/workspace/target/release/libxhjob.so';
    if (!file_exists($so)) {
        $so = '/workspace/releases/xhjob-php8.2-linux-x86_64.so';
    }
    $iniDir = '/tmp/xhjob_prod_ini';
    if (!is_dir($iniDir)) {
        @mkdir($iniDir, 0777, true);
    }
    file_put_contents($iniDir . '/xhjob.ini', "extension={$so}\n");

    // 保留默认 scan dir（解析 php_ini_scanned_files 获取目录）
    $defaultScanDir = '';
    $scanned = php_ini_scanned_files();
    if (!empty($scanned)) {
        $files = array_filter(array_map('trim', explode(',', $scanned)));
        if (!empty($files)) {
            $defaultScanDir = dirname(reset($files));
        }
    }
    $scanDir = $defaultScanDir !== '' ? "{$defaultScanDir}:{$iniDir}" : $iniDir;
    putenv("PHP_INI_SCAN_DIR={$scanDir}");
}

// 清理上一次测试残留
@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);
foreach ((array) glob('/tmp/xhjob_prod_*') as $f) {
    @unlink($f);
}

// 设置扩展扫描目录（必须在 xhjob_start 之前）
setupExtensionScanDir();

/**
 * 启动本地 HTTP 服务器（记录请求数到文件）
 */
function startHttpServer(int $port): int
{
    $scriptPath = '/tmp/xhjob_prod_http_server.php';
    $countFile  = '/tmp/xhjob_prod_http_count.txt';
    file_put_contents($countFile, '0');
    $code = '<?php'
        . ' $f="' . $countFile . '";'
        . ' $n=(int)@file_get_contents($f);'
        . ' $n++;'
        . ' file_put_contents($f,(string)$n);'
        . ' http_response_code(200);'
        . ' header("Content-Type: text/plain");'
        . ' echo "OK ".$n;';
    file_put_contents($scriptPath, $code);
    $cmd = "php -S 127.0.0.1:$port $scriptPath > /dev/null 2>&1 & echo $!";
    $pid = trim((string) shell_exec($cmd));
    usleep(800000); // 等待服务器启动
    return (int) $pid;
}

/**
 * 停止本地 HTTP 服务器
 */
function stopHttpServer(int $pid): void
{
    if ($pid > 0) {
        @posix_kill($pid, 15);
        usleep(100000);
        @posix_kill($pid, 9);
    }
    @unlink('/tmp/xhjob_prod_http_server.php');
}

echo "=== Xhjob 生产环境业务模拟测试 ===\n";
echo "service=$SERVICE data_dir=$DATA_DIR\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n";
echo "函数检查: chord=" . (function_exists('xhjob_chord') ? 'OK' : 'MISSING')
   . " report_progress=" . (function_exists('xhjob_report_progress') ? 'OK' : 'MISSING')
   . " inspect=" . (function_exists('xhjob_inspect') ? 'OK' : 'MISSING') . "\n\n";

// ============================================================
// Step 1: 准备 daemon
// ============================================================
step(1, '准备 daemon（启动 + 健康检查）', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "daemon 启动失败 pid=$pid");
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    assertTrue($h['healthy'], "daemon 不健康: " . json_encode($h));
    echo "（pid={$h['pid']}）";
});

// ============================================================
// Step 2: 场景1 — 订单异步处理（chain 流水线）
// ============================================================
step(2, '场景1 订单异步处理（chain 流水线）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_order.txt');
    @unlink('/tmp/xhjob_prod_invoice.txt');
    @unlink('/tmp/xhjob_prod_notify.txt');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $chainId = $mgr->createChain([
        TaskBuilder::shell('echo "ORDER:$(date +%s)" > /tmp/xhjob_prod_order.txt')->withRetry(0, 0),
        TaskBuilder::shell('echo "INVOICE:$(cat /tmp/xhjob_prod_order.txt)" > /tmp/xhjob_prod_invoice.txt')->withRetry(0, 0),
        TaskBuilder::shell('echo "NOTIFY:$(cat /tmp/xhjob_prod_invoice.txt)" > /tmp/xhjob_prod_notify.txt')->withRetry(0, 0),
    ]);
    assertTrue(!empty($chainId) && strpos($chainId, 'error') === false, "chain 创建失败: $chainId");

    // 等待 chain 完成
    $deadline = time() + 30;
    $finalState = null;
    while (time() < $deadline) {
        $cs = $mgr->chainState($chainId);
        $finalState = $cs['state'] ?? null;
        if ($finalState === 'success' || $finalState === 'failed') {
            break;
        }
        usleep(500000);
    }
    assertEq($finalState, 'success', 'chain 应 success');

    // 验证 3 个文件都生成且内容正确串联
    assertTrue(file_exists('/tmp/xhjob_prod_order.txt'), 'order.txt 未生成');
    assertTrue(file_exists('/tmp/xhjob_prod_invoice.txt'), 'invoice.txt 未生成');
    assertTrue(file_exists('/tmp/xhjob_prod_notify.txt'), 'notify.txt 未生成');

    $order   = trim((string) file_get_contents('/tmp/xhjob_prod_order.txt'));
    $invoice = trim((string) file_get_contents('/tmp/xhjob_prod_invoice.txt'));
    $notify  = trim((string) file_get_contents('/tmp/xhjob_prod_notify.txt'));

    assertTrue(strpos($order, 'ORDER:') === 0, "order.txt 内容异常: $order");
    assertTrue(strpos($invoice, 'INVOICE:ORDER:') === 0, "invoice 未串联 order: $invoice");
    assertTrue(strpos($notify, 'NOTIFY:INVOICE:ORDER:') === 0, "notify 未串联 invoice: $notify");

    echo "（notify={$notify}）";
});

// ============================================================
// Step 3: 场景2 — 定时报表生成（cron + maxExecutions + progress）
// ============================================================
step(3, '场景2 定时报表生成（cron + maxExecutions + progress）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_report.log');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo "REPORT:$(date +%s)" >> /tmp/xhjob_prod_report.log')
            ->cron('* * * * *')
            ->maxExecutions(1)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error') === false, "任务创建失败: $id");

    // 上报进度（测试 reportProgress API）
    $mgr->reportProgress($id, 50);

    // 等待任务执行一次（cron 每分钟触发，最多等 75 秒）
    $ok = $mgr->waitForState($id, 'success', 75);
    if (!$ok) {
        $st = $mgr->state($id);
        throw new Exception("任务未在 75s 内 success，状态: " . json_encode($st));
    }

    // 验证 report.log 非空
    assertTrue(file_exists('/tmp/xhjob_prod_report.log'), 'report.log 未生成');
    $content = (string) file_get_contents('/tmp/xhjob_prod_report.log');
    assertTrue(strlen(trim($content)) > 0, 'report.log 为空');
    assertTrue(strpos($content, 'REPORT:') === 0, "report.log 内容异常: $content");

    echo "（report.log 非空）";
});

// ============================================================
// Step 4: 场景3 — 批量数据 ETL（chord 汇总）
// ============================================================
step(4, '场景3 批量数据 ETL（chord 汇总）', function () use ($SERVICE, $DATA_DIR) {
    foreach ([1, 2, 3] as $i) {
        @unlink("/tmp/xhjob_prod_part_$i.txt");
    }
    @unlink('/tmp/xhjob_prod_merged.txt');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);

    // 3 个 header 任务并行抽取
    $headers = [
        TaskBuilder::shell('echo "DATA_A:1" > /tmp/xhjob_prod_part_1.txt')->withRetry(0, 0),
        TaskBuilder::shell('echo "DATA_A:2" > /tmp/xhjob_prod_part_2.txt')->withRetry(0, 0),
        TaskBuilder::shell('echo "DATA_A:3" > /tmp/xhjob_prod_part_3.txt')->withRetry(0, 0),
    ];
    // 1 个 callback 任务汇总
    $callback = TaskBuilder::shell('cat /tmp/xhjob_prod_part_*.txt > /tmp/xhjob_prod_merged.txt')->withRetry(0, 0);

    $chordId = $mgr->createChord($headers, $callback);
    assertTrue(!empty($chordId) && strpos($chordId, 'error') === false, "chord 创建失败: $chordId");

    // 等待 chord 完成
    $deadline = time() + 30;
    $finalState = null;
    $cs = null;
    while (time() < $deadline) {
        $cs = $mgr->chordState($chordId);
        $finalState = $cs['state'] ?? null;
        if ($finalState === 'success' || $finalState === 'partial_failed') {
            break;
        }
        usleep(500000);
    }
    assertEq($finalState, 'success', 'chord 应 success');

    // 验证 3 个 part 文件 + 1 个 merged 文件都生成
    foreach ([1, 2, 3] as $i) {
        assertTrue(file_exists("/tmp/xhjob_prod_part_$i.txt"), "part_$i.txt 未生成");
    }
    assertTrue(file_exists('/tmp/xhjob_prod_merged.txt'), 'merged.txt 未生成');

    $merged = (string) file_get_contents('/tmp/xhjob_prod_merged.txt');
    assertTrue(strpos($merged, 'DATA_A:') !== false, "merged.txt 内容异常: $merged");

    echo "（merged 行数=" . count(file('/tmp/xhjob_prod_merged.txt', FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES)) . "）";
});

// ============================================================
// Step 5: 场景4 — 定时清理任务（cron + maxExecutions + retry 退避）
// ============================================================
step(5, '场景4 定时清理任务（cron + retry 退避）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_clean_trigger');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell("sh -c 'test -f /tmp/xhjob_prod_clean_trigger && rm /tmp/xhjob_prod_clean_trigger || (touch /tmp/xhjob_prod_clean_trigger && exit 1)'")
            ->cron('* * * * *')
            ->maxExecutions(1)
            ->withRetry(2, 1)
            ->retryBackoff(true)
    );
    assertTrue(!empty($id) && strpos($id, 'error') === false, "任务创建失败: $id");

    // 等待任务最终 success（cron 触发 + 重试后成功，最多 80 秒）
    $ok = $mgr->waitForState($id, 'success', 80);
    if (!$ok) {
        $st = $mgr->state($id);
        throw new Exception("任务未在 80s 内 success，状态: " . json_encode($st));
    }

    // 验证 trigger 文件已被清理（第二次执行 rm 成功）
    assertTrue(!file_exists('/tmp/xhjob_prod_clean_trigger'), 'trigger 文件应已被清理（重试成功后删除）');

    echo "（trigger 已清理，重试后 success）";
});

// ============================================================
// Step 6: 场景5 — 通知发送（HTTP + idempotent + 重试）
// ============================================================
step(6, '场景5 通知发送（HTTP + idempotent + 重试）', function () use ($SERVICE, $DATA_DIR, $HTTP_PORT) {
    @unlink('/tmp/xhjob_prod_http_count.txt');

    // 启动本地 HTTP 服务器
    $httpPid = startHttpServer($HTTP_PORT);
    assertTrue($httpPid > 0, 'HTTP 服务器启动失败');

    try {
        // 验证服务器已就绪
        $test = @file_get_contents("http://127.0.0.1:$HTTP_PORT/health");
        assertTrue($test !== false, 'HTTP 服务器无响应');

        $mgr = new TaskManager($SERVICE, $DATA_DIR);
        $body = json_encode(['event' => 'order_completed', 'order_id' => 'PROD-001']);
        $id = $mgr->create(
            TaskBuilder::http('POST', "http://127.0.0.1:$HTTP_PORT/notify")
                ->withBody($body)
                ->withRetry(3, 1)
                ->idempotent(true)
        );
        assertTrue(!empty($id) && strpos($id, 'error') === false, "HTTP 任务创建失败: $id");

        // 等待 success
        $ok = $mgr->waitForState($id, 'success', 30);
        if (!$ok) {
            $st = $mgr->state($id);
            throw new Exception("HTTP 任务未在 30s 内 success，状态: " . json_encode($st));
        }

        // 验证 HTTP 服务器收到了请求
        $count = (int) file_get_contents('/tmp/xhjob_prod_http_count.txt');
        assertTrue($count >= 1, "HTTP 服务器应收到 >=1 个请求，实际 $count");

        echo "（请求数={$count}）";
    } finally {
        stopHttpServer($httpPid);
    }
});

// ============================================================
// Step 7: 场景6 — 延迟任务（countdown）
// ============================================================
step(7, '场景6 延迟任务（countdown 5s）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_delayed.txt');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $createTs = time();
    $id = $mgr->create(
        TaskBuilder::shell('echo DELAYED > /tmp/xhjob_prod_delayed.txt')
            ->countdown(5)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error') === false, "任务创建失败: $id");

    // 等待任务 success
    $ok = $mgr->waitForState($id, 'success', 20);
    if (!$ok) {
        $st = $mgr->state($id);
        throw new Exception("延迟任务未在 20s 内 success，状态: " . json_encode($st));
    }

    // 验证延迟生效：started_at >= createTs + 5（允许 1 秒误差）
    $st = $mgr->state($id);
    $startedAt = (int) ($st['started_at'] ?? 0);
    assertTrue($startedAt > 0, "started_at 未记录: " . json_encode($st));
    $elapsed = $startedAt - $createTs;
    assertTrue($elapsed >= 4, "延迟未生效，started_at - createTs = {$elapsed}s（应 >= 4s）");

    // 验证文件生成
    assertTrue(file_exists('/tmp/xhjob_prod_delayed.txt'), 'delayed.txt 未生成');
    $content = trim((string) file_get_contents('/tmp/xhjob_prod_delayed.txt'));
    assertEq($content, 'DELAYED', 'delayed.txt 内容应 为 DELAYED');

    echo "（延迟 {$elapsed}s）";
});

// ============================================================
// Step 8: 场景7 — 限流与并发控制（rateLimit + maxInstances）
// ============================================================
step(8, '场景7 限流与并发控制（rateLimit + maxInstances）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_rate.log');
    @unlink('/tmp/xhjob_prod_rate_throttle.log');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);

    // 创建 5 个 shell 任务，每个带 rateLimit(2, 10) + maxInstances(1) + withRetry(0,0)
    $ids = [];
    for ($i = 1; $i <= 5; $i++) {
        $ids[] = $mgr->create(
            TaskBuilder::shell("echo \"RATE:$i\" >> /tmp/xhjob_prod_rate.log")
                ->rateLimit(2, 10)
                ->maxInstances(1)
                ->withRetry(0, 0)
        );
    }

    // 派发后立即查询，验证有部分任务处于 pending/running（非全部立即 success）
    $nonTerminalCount = 0;
    foreach ($ids as $tid) {
        $st = $mgr->state($tid);
        $s = $st['state'] ?? '?';
        if (!in_array($s, ['success'], true)) {
            $nonTerminalCount++;
        }
    }
    // 至少有 1 个任务未立即完成（证明非全部瞬间 success）
    assertTrue($nonTerminalCount >= 1, "派发后应至少 1 个任务非 success 状态");

    // 等待全部完成
    $deadline = time() + 30;
    while (time() < $deadline) {
        $allDone = true;
        foreach ($ids as $tid) {
            $st = $mgr->state($tid);
            if (!in_array($st['state'] ?? '', ['success', 'failed'], true)) {
                $allDone = false;
                break;
            }
        }
        if ($allDone) {
            break;
        }
        usleep(300000);
    }

    $successCount = 0;
    foreach ($ids as $tid) {
        $st = $mgr->state($tid);
        if (($st['state'] ?? '') === 'success') {
            $successCount++;
        }
    }
    assertTrue($successCount === 5, "5 个任务应全部 success，实际 $successCount/5");

    // 验证 rate.log 包含 5 行
    assertTrue(file_exists('/tmp/xhjob_prod_rate.log'), 'rate.log 未生成');
    $lines = file('/tmp/xhjob_prod_rate.log', FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
    assertTrue(count($lines) >= 5, "rate.log 应有 >=5 行，实际 " . count($lines));

    // 补充验证：单任务 rateLimit 实际节流效果
    // every(1) + rateLimit(1, 5) + maxExecutions(2)：第 1 次立即执行，
    // 第 2 次被限流推迟 ~5 秒，总耗时 >= 4 秒
    $throttleStart = time();
    $throttleId = $mgr->create(
        TaskBuilder::shell('echo THROTTLE >> /tmp/xhjob_prod_rate_throttle.log')
            ->every(1)
            ->rateLimit(1, 5)
            ->maxExecutions(2)
            ->withRetry(0, 0)
            ->timeout(10)
    );
    $ok = $mgr->waitForState($throttleId, 'success', 20);
    if (!$ok) {
        $st = $mgr->state($throttleId);
        throw new Exception("限流任务未在 20s 内 success，状态: " . json_encode($st));
    }
    $throttleElapsed = time() - $throttleStart;
    assertTrue($throttleElapsed >= 4, "rateLimit 节流应使 2 次执行耗时 >=4s，实际 {$throttleElapsed}s");

    echo "（5/5 success, rate.log=" . count($lines) . " 行, 节流耗时={$throttleElapsed}s）";
});

// ============================================================
// Step 9: 场景8 — 持久化与崩溃恢复（daemon 重启恢复）
// ============================================================
step(9, '场景8 持久化与崩溃恢复（daemon 重启恢复）', function () use ($SERVICE, $DATA_DIR) {
    @unlink('/tmp/xhjob_prod_recovered.txt');

    $mgr = new TaskManager($SERVICE, $DATA_DIR);

    // 创建持久化的延迟任务
    $createTs = time();
    $id = $mgr->create(
        TaskBuilder::shell('echo RECOVERED > /tmp/xhjob_prod_recovered.txt')
            ->persist(true)
            ->countdown(8)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error') === false, "持久化任务创建失败: $id");

    // 立即停止 daemon（模拟崩溃）
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');

    // 等待 2 秒后重启 daemon
    sleep(2);
    $pid = $svc->start();
    assertTrue($pid > 0, "daemon 重启失败 pid=$pid");
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    assertTrue($h['healthy'], "重启后 daemon 不健康: " . json_encode($h));

    // 等待任务 success（daemon 重启后恢复持久化任务）
    $ok = $mgr->waitForState($id, 'success', 30);
    if (!$ok) {
        $st2 = $mgr->state($id);
        throw new Exception("恢复任务未在 30s 内 success，状态: " . json_encode($st2));
    }

    // 验证 recovered.txt 文件生成
    assertTrue(file_exists('/tmp/xhjob_prod_recovered.txt'), 'recovered.txt 未生成（daemon 重启后未恢复持久化任务）');
    $content = trim((string) file_get_contents('/tmp/xhjob_prod_recovered.txt'));
    assertEq($content, 'RECOVERED', 'recovered.txt 内容应为 RECOVERED');

    $elapsed = time() - $createTs;
    echo "（崩溃恢复成功，耗时 {$elapsed}s）";
});

// ============================================================
// Step 10: 清理
// ============================================================
step(10, '清理（停止 daemon + 清理临时文件）', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');

    // 清理临时文件
    foreach ((array) glob('/tmp/xhjob_prod_*') as $f) {
        @unlink($f);
    }
    @system("rm -rf $DATA_DIR");

    echo "（daemon 已停止，临时文件已清理）";
});

// ============================================================
// 汇总报告
// ============================================================
echo "\n========================================\n";
echo "结果: $pass PASS, $fail FAIL, $skipped SKIP\n";
echo "========================================\n";
exit($fail > 0 ? 1 : 0);
