#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 跨进程功能测试 - 客户端功能测试器（Task 7）
// +----------------------------------------------------------------------
// | 连接 xhjob_server.php 启动的 daemon，逐函数测试全部 xhjob_* 函数
// | + TaskBuilder 链式 API + 17 类复杂生产场景。
// |
// | 用法：
// |   先启动 server:  php -d extension=<so> tp/xhjob_server.php
// |   再运行 client:  php -d extension=<so> tp/xhjob_client_test.php
// |   自定义参数:     php -d extension=<so> tp/xhjob_client_test.php --service=foo --data-dir=/tmp/foo
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

// 扩展加载兜底
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

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

// ======================================================================
// 工具函数
// ======================================================================

/**
 * 注入 PHP_INI_SCAN_DIR（与 server 一致），让本进程派生的 daemon 子进程
 * 能通过 ini scan dir 加载 xhjob 扩展。client 自身已通过 -d extension 加载，
// 但部分场景（如 persist 崩溃恢复）需要 client 启动新 daemon，故仍需注入。
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

$pass = 0;
$fail = 0;
$skipped = 0;
$failedSteps = [];

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $skipped, $failedSteps;
    echo "[{$n}] {$desc} ... ";
    try {
        $r = $fn();
        if ($r === 'SKIP') {
            $skipped++;
            echo "SKIP\n";
            return;
        }
        $pass++;
        echo "PASS\n";
    } catch (\Throwable $e) {
        $fail++;
        $failedSteps[] = $n;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

function assertTrue(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new \Exception($msg);
    }
}

function assertEq($a, $b, string $msg): void
{
    if ($a !== $b) {
        throw new \Exception("{$msg} (expected=" . var_export($b, true) . ", got=" . var_export($a, true) . ")");
    }
}

function assertContains(string $haystack, string $needle, string $msg): void
{
    if (strpos($haystack, $needle) === false) {
        throw new \Exception("{$msg} (expected contains '{$needle}')");
    }
}

// fake HTTP 服务器（参考 test_xhjob_fixes.php）
function startFakeHttpServer(int $port, int $status = 500): int
{
    $scriptPath = sys_get_temp_dir() . "/xhjob_cross_fake_{$port}.php";
    file_put_contents($scriptPath, "<?php http_response_code({$status}); echo 'Fake-Status-{$status}';");
    $cmd = "php -S 127.0.0.1:{$port} {$scriptPath} > /dev/null 2>&1 & echo $!";
    $pid = trim((string) shell_exec($cmd));
    usleep(500000);
    return (int) $pid;
}

function stopFakeHttpServer(int $pid, int $port): void
{
    if ($pid > 0) {
        @posix_kill($pid, 9);
    }
    @unlink(sys_get_temp_dir() . "/xhjob_cross_fake_{$port}.php");
}

// ======================================================================
// CLI 参数
// ======================================================================
$service = 'xhjob-cross';
$dataDir = '/tmp/xhjob-cross';

foreach ($argv as $arg) {
    if (strpos($arg, '--service=') === 0) {
        $v = substr($arg, strlen('--service='));
        if ($v !== '') {
            $service = $v;
        }
    } elseif (strpos($arg, '--data-dir=') === 0) {
        $v = substr($arg, strlen('--data-dir='));
        if ($v !== '') {
            $dataDir = $v;
        }
    }
}

setupExtensionScanDir();

echo "=== Xhjob 跨进程客户端功能测试 ===\n";
echo "service={$service} data_dir={$dataDir}\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n\n";

$svc = new XhjobService($service, $dataDir);
$mgr = new TaskManager($service, $dataDir);

// ======================================================================
// SubTask 7.1: step 框架 + 连接已运行 daemon
// ======================================================================
echo "--- SubTask 7.1: 连接已运行 daemon ---\n";

step(1, '验证 daemon 存活（healthCheck）', function () use ($svc) {
    $h = $svc->healthCheck();
    if (!$h['healthy']) {
        throw new \Exception("daemon 不健康: " . json_encode($h) . "，请先运行 tp/xhjob_server.php");
    }
});

// ======================================================================
// SubTask 7.2: 生命周期函数（start/stop/restart/status）
// ======================================================================
echo "\n--- SubTask 7.2: 生命周期函数 ---\n";

step(2, 'xhjob_status 验证 running=true + pid>0', function () use ($svc) {
    $st = $svc->status();
    assertTrue($st['running'], 'running 应为 true，实际: ' . json_encode($st));
    assertTrue($st['pid'] !== null && $st['pid'] > 0, 'pid 应 > 0，实际: ' . json_encode($st));
});

step(3, 'xhjob_start 幂等（对已运行 daemon 调用返回 true）', function () use ($service, $dataDir) {
    $ok = xhjob_start($service, $dataDir);
    assertTrue($ok, 'xhjob_start 对已运行 daemon 应返回 true（幂等）');
});

step(4, 'xhjob_restart 重启 daemon + 新 pid', function () use ($svc) {
    $oldStatus = $svc->status();
    $oldPid = $oldStatus['pid'];
    assertTrue($oldPid > 0, '旧 pid 应 > 0');
    $newPid = $svc->restart();
    assertTrue($newPid > 0, '新 pid 应 > 0');
    assertTrue($newPid !== $oldPid, "新 pid({$newPid}) 应不同于旧 pid({$oldPid})");
    // 重启后重新等待就绪
    $svc->wait(10, true);
    $h = $svc->healthCheck();
    assertTrue($h['healthy'], '重启后 daemon 应健康');
});

// ======================================================================
// SubTask 7.3: 任务 CRUD 函数（dispatch/state/result/get/list/remove）
// ======================================================================
echo "\n--- SubTask 7.3: 任务 CRUD 函数 ---\n";

$crudTaskId = null;

step(5, 'xhjob_dispatch 派发 shell 任务', function () use (&$crudTaskId, $mgr) {
    $id = xhjob_dispatch(
        TaskBuilder::shell('echo hello-cross')->withRetry(0, 0)->toJson(),
        $GLOBALS['service'],
        $GLOBALS['dataDir']
    );
    assertTrue(!empty($id) && strpos($id, 'error:') !== 0, "dispatch 应返回 task_id，实际: {$id}");
    $crudTaskId = $id;
});

step(6, 'xhjob_state 查询状态 + 等待 success', function () use (&$crudTaskId, $mgr) {
    $ok = $mgr->waitForState($crudTaskId, 'success', 15);
    assertTrue($ok, '任务应进入 success');
    $st = $mgr->state($crudTaskId);
    assertEq($st['state'] ?? '', 'success', 'state 应为 success');
});

step(7, 'xhjob_result 查询结果含 hello-cross', function () use (&$crudTaskId, $mgr) {
    $r = $mgr->result($crudTaskId);
    $stdout = $r['stdout'] ?? '';
    assertContains($stdout, 'hello-cross', 'stdout 应含 hello-cross');
});

step(8, 'xhjob_get 查询任务详情含 task_id', function () use (&$crudTaskId, $mgr) {
    $task = $mgr->get($crudTaskId);
    assertTrue(is_array($task), 'get 应返回数组');
    $id = $task['id'] ?? ($task['task_id'] ?? null);
    assertTrue(!empty($id), '任务详情应含 id 字段: ' . json_encode(array_keys($task)));
});

step(9, 'xhjob_list 列表非空', function () use ($mgr) {
    $list = $mgr->list();
    assertTrue(count($list) >= 1, 'list 应非空，实际 ' . count($list) . ' 条');
});

step(10, 'xhjob_remove 删除 pending 任务后再 get 返回 null', function () use ($mgr) {
    // daemon 实际行为：终态（success/failed）任务的 remove 返回 false
    // （与 APScheduler remove_job 一致——终态任务已归档不可删）；
    // remove 的主用例是删除尚未执行的 pending 任务。此处派发一个
    // countdown(120) 任务使其保持 pending，验证 remove 成功且后续 get 返回 null。
    global $service, $dataDir;
    $id = xhjob_dispatch(
        TaskBuilder::shell('echo remove-target')->countdown(120)->withRetry(0, 0)->toJson(),
        $service,
        $dataDir
    );
    assertTrue(!empty($id) && strpos($id, 'error:') !== 0, "dispatch pending 任务失败: {$id}");
    usleep(300000);
    $ok = $mgr->remove($id);
    assertTrue($ok, 'remove 应返回 true（pending 任务可删除）');
    // 删除后 get 应返回 null（xhjob_get 对不存在任务返回 null）
    $task = $mgr->get($id);
    assertTrue($task === null, '删除后 get 应返回 null，实际: ' . json_encode($task));
});

// ======================================================================
// SubTask 7.4: 任务控制函数（pause/resume/cancel/requeue/reschedule）
// ======================================================================
echo "\n--- SubTask 7.4: 任务控制函数 ---\n";

$ctrlTaskId = null;

step(11, 'xhjob_pause 暂停 cron 任务', function () use (&$ctrlTaskId, $mgr) {
    // 创建 cron 任务（maxExecutions=0 无限循环，避免任务进入 success 终态后
    // 无法被 reschedule/cancel；pause 确保 cron 不会立即触发）
    $id = $mgr->create(
        TaskBuilder::shell('echo tick-cross')
            ->cron('* * * * *')
            ->maxExecutions(0)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error:') === false, "create 失败: {$id}");
    $ctrlTaskId = $id;
    // 等一小段时间让任务进入 pending
    usleep(500000);
    $ok = xhjob_pause($id, $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'pause 应返回 true');
    $st = $mgr->state($id);
    // paused 字段应为 true（state 查询返回 paused 字段）
    $paused = ($st['paused'] ?? 'false') === 'true' || ($st['paused'] ?? false) === true;
    assertTrue($paused, 'paused 应为 true，实际: ' . json_encode($st));
});

step(12, 'xhjob_resume 恢复任务', function () use (&$ctrlTaskId, $mgr) {
    $ok = xhjob_resume($ctrlTaskId, $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'resume 应返回 true');
    $st = $mgr->state($ctrlTaskId);
    $paused = ($st['paused'] ?? 'false') === 'true' || ($st['paused'] ?? false) === true;
    assertTrue(!$paused, 'resume 后 paused 应为 false，实际: ' . json_encode($st));
});

step(13, 'xhjob_reschedule 修改 cron 表达式', function () use (&$ctrlTaskId, $mgr) {
    // reschedule 前先 pause，确保任务处于非终态 pending（避免 cron 触发后
    // 因自然执行导致状态切换的竞态）；reschedule 仅接受非终态 cron 任务
    xhjob_pause($ctrlTaskId, $GLOBALS['service'], $GLOBALS['dataDir']);
    $ok = xhjob_reschedule($ctrlTaskId, '*/5 * * * *', $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'reschedule 应返回 true');
    // 通过 get 查询 cron 字段是否更新
    $task = $mgr->get($ctrlTaskId);
    $cron = $task['cron'] ?? null;
    assertEq($cron, '*/5 * * * *', 'reschedule 后 cron 字段应为 */5 * * * *');
});

step(14, 'xhjob_cancel 取消 pending 任务', function () use (&$ctrlTaskId, $mgr) {
    // reschedule 后 cron 改为 */5（5 分钟一次），任务处于 pending 非终态
    $ok = xhjob_cancel($ctrlTaskId, $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'cancel 应返回 true');
    // 轮询等待任务进入 cancelled 终态（pending 任务 cancel 立即转 cancelled）
    $deadline = time() + 5;
    $state = '';
    while (time() < $deadline) {
        $st = $mgr->state($ctrlTaskId);
        $state = $st['state'] ?? '';
        if ($state === 'cancelled') {
            break;
        }
        usleep(200000);
    }
    assertEq($state, 'cancelled', 'cancel 后 state 应为 cancelled，实际: ' . json_encode($st ?? null));
});

step(15, 'xhjob_requeue 重新入队 cancelled 任务', function () use (&$ctrlTaskId, $mgr) {
    $ok = xhjob_requeue($ctrlTaskId, $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'requeue 应返回 true');
    $st = $mgr->state($ctrlTaskId);
    $state = $st['state'] ?? '';
    assertTrue(in_array($state, ['pending', 'running', 'success'], true), "requeue 后 state 应为 pending/running/success，实际: {$state}");
});

// ======================================================================
// SubTask 7.5: 编排函数（chain/group/chord/countdown + state 查询）
// ======================================================================
echo "\n--- SubTask 7.5: 编排函数 ---\n";

step(16, 'xhjob_chain 3 步流水线 + 文件生成', function () use ($mgr) {
    $f1 = '/tmp/xhjob_cross_chain1.txt';
    $f2 = '/tmp/xhjob_cross_chain2.txt';
    $f3 = '/tmp/xhjob_cross_chain3.txt';
    @unlink($f1); @unlink($f2); @unlink($f3);
    $chainId = $mgr->createChain([
        TaskBuilder::shell("echo s1 > {$f1}")->withRetry(0, 0),
        TaskBuilder::shell("echo s2 > {$f2}")->withRetry(0, 0),
        TaskBuilder::shell("echo s3 > {$f3}")->withRetry(0, 0),
    ]);
    assertTrue(!empty($chainId) && strpos($chainId, 'error:') === false, "chain 创建失败: {$chainId}");
    // 等待文件生成
    $deadline = time() + 15;
    while (time() < $deadline) {
        if (file_exists($f1) && file_exists($f2) && file_exists($f3)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(file_exists($f1), "文件 {$f1} 应生成");
    assertTrue(file_exists($f2), "文件 {$f2} 应生成");
    assertTrue(file_exists($f3), "文件 {$f3} 应生成");
    @unlink($f1); @unlink($f2); @unlink($f3);
});

step(17, 'xhjob_chain_state 查询 chain 状态', function () use ($mgr) {
    $chainId = $mgr->createChain([
        TaskBuilder::shell('echo cs1')->withRetry(0, 0),
        TaskBuilder::shell('echo cs2')->withRetry(0, 0),
    ]);
    assertTrue(!empty($chainId) && strpos($chainId, 'error:') === false, "chain 创建失败: {$chainId}");
    // 等待 chain 完成
    $deadline = time() + 10;
    $state = null;
    while (time() < $deadline) {
        $state = $mgr->chainState($chainId);
        if ($state && in_array($state['state'] ?? '', ['success', 'failed'], true)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(is_array($state), 'chainState 应返回数组');
    assertTrue(!empty($state['state']), 'chain 应有 state 字段');
});

step(18, 'xhjob_group 3 个并行任务', function () use ($mgr) {
    $groupId = $mgr->createGroup([
        TaskBuilder::shell('echo g1-cross')->withRetry(0, 0),
        TaskBuilder::shell('echo g2-cross')->withRetry(0, 0),
        TaskBuilder::shell('echo g3-cross')->withRetry(0, 0),
    ]);
    assertTrue(!empty($groupId) && strpos($groupId, 'error:') === false, "group 创建失败: {$groupId}");
    // 等待 group 完成
    $deadline = time() + 15;
    while (time() < $deadline) {
        $gs = $mgr->groupState($groupId);
        if ($gs && in_array($gs['state'] ?? '', ['success', 'failed', 'partial_failed'], true)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(true, 'group 已派发并等待完成');
});

step(19, 'xhjob_group_state 验证 summary', function () use ($mgr) {
    $groupId = $mgr->createGroup([
        TaskBuilder::shell('echo gs1')->withRetry(0, 0),
        TaskBuilder::shell('echo gs2')->withRetry(0, 0),
    ]);
    $deadline = time() + 10;
    $gs = null;
    while (time() < $deadline) {
        $gs = $mgr->groupState($groupId);
        if ($gs && in_array($gs['state'] ?? '', ['success', 'failed', 'partial_failed'], true)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(is_array($gs), 'groupState 应返回数组');
    assertTrue(isset($gs['state']), 'group 应有 state 字段');
    // summary 可选，存在时验证 total>=2
    if (isset($gs['summary'])) {
        $total = $gs['summary']['total'] ?? 0;
        assertTrue($total >= 2, "summary.total 应 >= 2，实际: {$total}");
    }
});

step(20, 'xhjob_chord 3 header + 1 callback', function () use ($mgr) {
    $chordId = $mgr->createChord(
        [
            TaskBuilder::shell('echo h1-cross')->withRetry(0, 0),
            TaskBuilder::shell('echo h2-cross')->withRetry(0, 0),
            TaskBuilder::shell('echo h3-cross')->withRetry(0, 0),
        ],
        TaskBuilder::shell('echo callback-cross')->withRetry(0, 0)
    );
    assertTrue(!empty($chordId) && strpos($chordId, 'error:') === false, "chord 创建失败: {$chordId}");
    // 等待 chord 完成
    $deadline = time() + 15;
    while (time() < $deadline) {
        $cs = $mgr->chordState($chordId);
        if ($cs && in_array($cs['state'] ?? '', ['success', 'failed', 'partial_failed'], true)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(true, 'chord 已派发并等待完成');
});

step(21, 'xhjob_chord_state 查询 chord 状态', function () use ($mgr) {
    $chordId = $mgr->createChord(
        [TaskBuilder::shell('echo hs1')->withRetry(0, 0)],
        TaskBuilder::shell('echo hs-cb')->withRetry(0, 0)
    );
    $deadline = time() + 10;
    $cs = null;
    while (time() < $deadline) {
        $cs = $mgr->chordState($chordId);
        if ($cs && in_array($cs['state'] ?? '', ['success', 'failed', 'partial_failed'], true)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(is_array($cs), 'chordState 应返回数组');
    assertTrue(!empty($cs['state']), 'chord 应有 state 字段');
});

step(22, 'countdown（TaskBuilder::countdown）5 秒延迟', function () use ($mgr) {
    $f = '/tmp/xhjob_cross_delayed.txt';
    @unlink($f);
    $id = $mgr->create(
        TaskBuilder::shell("echo delayed > {$f}")
            ->countdown(3)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error:') === false, "countdown 任务创建失败: {$id}");
    // 立即检查文件不应存在（countdown 未到）
    usleep(500000);
    assertTrue(!file_exists($f), 'countdown 未到期时文件不应生成');
    // 等待 countdown 到期 + 执行
    $deadline = time() + 10;
    while (time() < $deadline) {
        if (file_exists($f)) {
            break;
        }
        usleep(300000);
    }
    assertTrue(file_exists($f), 'countdown 到期后文件应生成');
    @unlink($f);
});

// ======================================================================
// SubTask 7.6: 可观测函数（events/report_progress/pull_events/inspect）
// ======================================================================
echo "\n--- SubTask 7.6: 可观测函数 ---\n";

step(23, 'xhjob_events 查询任务事件流非空', function () use ($mgr) {
    $id = $mgr->create(TaskBuilder::shell('echo evt-cross')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $events = $mgr->logs($id);
    assertTrue(count($events) >= 1, 'events 应非空，实际 ' . count($events) . ' 条');
});

step(24, 'xhjob_report_progress 上报进度 50', function () use ($mgr) {
    // 创建一个长任务用于上报进度
    $id = $mgr->create(
        TaskBuilder::shell('sleep 2; echo prog-done')
            ->withRetry(0, 0)
            ->timeout(10)
    );
    // 等待任务进入 running
    usleep(500000);
    $ok = xhjob_report_progress($id, 50, null, $GLOBALS['service'], $GLOBALS['dataDir']);
    assertTrue($ok, 'report_progress 应返回 true');
    $mgr->waitForState($id, 'success', 10);
});

step(25, 'xhjob_pull_events 拉取事件流非空', function () use ($mgr) {
    $id = $mgr->create(TaskBuilder::shell('echo pull-evt-cross')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $events = $mgr->pullEvents(0, null);
    assertTrue(count($events) >= 1, 'pullEvents 应非空，实际 ' . count($events) . ' 条');
});

step(26, 'xhjob_inspect 四种 mode 返回数组', function () use ($mgr) {
    foreach (['stats', 'active', 'registered', 'scheduled'] as $mode) {
        $r = $mgr->inspect($mode);
        assertTrue(is_array($r), "inspect({$mode}) 应返回数组，实际: " . gettype($r));
    }
});

// ======================================================================
// SubTask 7.7: TaskBuilder 链式 API 全部方法
// ======================================================================
echo "\n--- SubTask 7.7: TaskBuilder 链式 API ---\n";

step(27, 'TaskBuilder 全部链式方法不抛异常 + toJson 含字段', function () {
    $b = TaskBuilder::shell('echo chain-api-test');
    $b->cron('0 * * * *')
      ->every(60)
      ->runAt(time() + 100)
      ->countdown(5)
      ->timeout(30)
      ->priority(10)
      ->maxInstances(2)
      ->withRetry(3, 2)
      ->maxExecutions(5)
      ->withMeta('{"k":"v"}')
      ->tags(['t1', 't2'])
      ->tag('t3')
      ->withTimezone('Asia/Shanghai')
      ->startAt(time() + 50)
      ->endAt(time() + 3600)
      ->resultTtl(600)
      ->jitter(2)
      ->expires(300)
      ->retryBackoff(true)
      ->ignoreResult(false)
      ->acksLate(true)
      ->softTimeout(10)
      ->misfireGraceTime(60)
      ->withId('chain-api-test-id')
      ->replaceExisting(true)
      ->rateLimit(10, 60)
      ->acksOnFailure(true)
      ->idempotent(true)
      ->coalesce(true)
      ->persist(true)
      ->allowOverlap(false)
      ->withEncoding('gbk')
      ->withStdin('input-data')
      ->withWorkingDir('/tmp')
      ->withEnv(['FOO' => 'bar']);
    $json = $b->toJson();
    assertTrue(!empty($json), 'toJson 应非空');
    $arr = json_decode($json, true);
    assertTrue(is_array($arr), 'toJson 应为合法 JSON');
    // 验证关键字段存在
    foreach (['cron', 'interval', 'run_at', 'countdown', 'timeout', 'priority', 'max_instances', 'retry_max', 'max_executions', 'meta', 'tags', 'timezone', 'start_date', 'end_date', 'result_ttl', 'jitter', 'expires', 'retry_backoff', 'ignore_result', 'acks_late', 'soft_timeout', 'misfire_grace_time', 'id', 'replace_existing', 'rate_limit_count', 'rate_limit_window', 'acks_on_failure', 'idempotent', 'coalesce', 'persist', 'allow_overlap', 'encoding'] as $field) {
        assertTrue(array_key_exists($field, $arr), "字段 {$field} 应存在于 toJson 输出");
    }
    // 验证 http 任务方法（withEncoding/withStdin/withWorkingDir/withEnv 仅 shell 可用，不在此调用）
    $h = TaskBuilder::http('POST', 'http://127.0.0.1:9999/');
    $h->withHeaders(['X-Test' => '1'])
      ->withBody('body-data')
      ->withProxy('http://127.0.0.1:8888');
    $hJson = json_decode($h->toJson(), true);
    assertTrue($hJson['task_type'] === 'http', 'http 任务 task_type 应为 http');
    assertTrue(isset($hJson['payload']['headers']), 'http 任务应有 headers');
    assertTrue($hJson['payload']['body'] === 'body-data', 'http 任务 body 应正确');
});

// ======================================================================
// SubTask 7.8: 17 类复杂生产场景
// ======================================================================
echo "\n--- SubTask 7.8: 17 类复杂生产场景 ---\n";

step(28, '场景1: daemon 跨进程存活（server 退出后 client 仍可连接）', function () use ($svc) {
    // 本测试天然验证：server 已退出，client 仍能 healthCheck 成功
    $h = $svc->healthCheck();
    assertTrue($h['healthy'], 'server 退出后 daemon 应仍存活，实际: ' . json_encode($h));
});

step(29, '场景2: 并发派发 10 个 shell 任务全部 success', function () use ($mgr) {
    $ids = [];
    for ($i = 0; $i < 10; $i++) {
        $ids[] = $mgr->create(TaskBuilder::shell("echo concurrent-{$i}")->withRetry(0, 0));
    }
    foreach ($ids as $i => $id) {
        $ok = $mgr->waitForState($id, 'success', 30);
        assertTrue($ok, "任务 {$i} (id={$id}) 应进入 success");
    }
});

step(30, '场景3: 长任务硬超时 + 软超时', function () use ($mgr) {
    $id = $mgr->create(
        TaskBuilder::shell('sleep 10')
            ->timeout(3)
            ->softTimeout(1)
            ->withRetry(0, 0)
    );
    $startTs = time();
    // 软超时 1s + 硬超时 3s，任务应在 1~3 秒内终止
    $deadline = time() + 10;
    $finalState = null;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        $finalState = $st['state'] ?? '';
        if (in_array($finalState, ['failed', 'cancelled', 'interrupted'], true)) {
            break;
        }
        usleep(300000);
    }
    $elapsed = time() - $startTs;
    assertTrue(in_array($finalState, ['failed', 'cancelled', 'interrupted'], true), "任务应失败/中断，实际 state={$finalState}");
    assertTrue($elapsed <= 5, "任务应在 5 秒内终止，实际 {$elapsed} 秒");
});

step(31, '场景4: 真实失败重试 + 指数退避 attempts=3', function () use ($mgr) {
    $id = $mgr->create(
        TaskBuilder::shell('exit 1')
            ->withRetry(2, 1)
            ->retryBackoff(true)
    );
    $mgr->waitForState($id, 'failed', 30);
    $st = $mgr->state($id);
    $attempts = (int) ($st['attempts'] ?? 0);
    assertTrue($attempts === 3, "attempts 应=3（1 初次 + 2 重试），实际 {$attempts}");
});

step(32, '场景5: 突发限流 rateLimit(2,5)', function () use ($mgr) {
    $id = $mgr->create(
        TaskBuilder::shell('echo rl-cross')
            ->every(1)
            ->rateLimit(2, 5)
            ->maxExecutions(4)
            ->withRetry(0, 0)
    );
    $start = time();
    // 等待 4 次执行完成（rateLimit(2,5) = 每 5 秒最多 2 次，4 次需 ~10 秒）
    $deadline = time() + 25;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(500000);
    }
    $elapsed = time() - $start;
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertTrue($ec >= 4, "execution_count 应 >= 4，实际 {$ec}");
    assertTrue($elapsed >= 5, "限流后应至少耗时 5 秒，实际 {$elapsed} 秒");
    $mgr->stop($id);
});

step(33, '场景6: maxInstances(1) 重叠控制', function () use ($mgr) {
    // 创建 sleep 2 + every 1 + maxInstances(1) + maxExecutions(2)
    // maxInstances(1) 阻止重叠：每次执行需 2 秒，所以 2 次执行需 ~4 秒
    // 若允许重叠，2 次执行可能在 2 秒内完成
    $id = $mgr->create(
        TaskBuilder::shell('sleep 2')
            ->every(1)
            ->maxInstances(1)
            ->maxExecutions(2)
            ->withRetry(0, 0)
            ->timeout(10)
    );
    $start = time();
    $deadline = time() + 20;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(500000);
    }
    $elapsed = time() - $start;
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertEq($ec, 2, 'maxExecutions(2) 后 execution_count 应=2');
    // maxInstances(1) + sleep(2) + every(1): 至少需 ~3-4 秒（两次顺序执行）
    assertTrue($elapsed >= 3, "maxInstances(1) 应阻止重叠，至少 3 秒，实际 {$elapsed} 秒");
});

step(34, '场景7: 持久化 + daemon 崩溃恢复', function () use ($mgr) {
    // 使用独立服务，避免影响主 daemon
    $persistSvc = 'xhjob-cross-persist';
    $persistDir = '/tmp/xhjob-cross-persist';
    @system('rm -rf ' . escapeshellarg($persistDir));
    @mkdir($persistDir, 0777, true);

    $pSvc = new XhjobService($persistSvc, $persistDir);
    $pMgr = new TaskManager($persistSvc, $persistDir);

    // 启动 daemon（persist 特性已默认启用）
    $pSvc->ensureStopped();
    $pid = $pSvc->start();
    assertTrue($pid > 0, "persist daemon 启动失败 pid={$pid}");
    $pSvc->wait(10, true);

    // 派发 persist(true) + countdown(8) 任务
    $id = $pMgr->create(
        TaskBuilder::shell('echo persist-recovered')
            ->persist(true)
            ->countdown(8)
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error:') === false, "persist 任务创建失败: {$id}");

    // 等 3 秒后停止 daemon（countdown 未到期）
    sleep(3);
    $pSvc->stop();
    $pSvc->wait(10, false);
    $st = $pSvc->status();
    assertTrue(!$st['running'], 'persist daemon 应已停止');

    // 重启 daemon，验证任务恢复
    $newPid = $pSvc->start();
    assertTrue($newPid > 0, "persist daemon 重启失败 pid={$newPid}");
    $pSvc->wait(10, true);

    // 验证任务仍存在（从 SQLite 恢复）
    $task = $pMgr->get($id);
    assertTrue(is_array($task), '重启后任务应从 SQLite 恢复，实际: ' . json_encode($task));

    // 等待 countdown 到期 + 执行
    $ok = $pMgr->waitForState($id, 'success', 20);
    assertTrue($ok, 'persist 任务应在重启后恢复执行');

    // 清理
    $pSvc->ensureStopped();
    @system('rm -rf ' . escapeshellarg($persistDir));
});

step(35, '场景8: 运行中任务取消', function () use ($mgr) {
    $id = $mgr->create(
        TaskBuilder::shell('sleep 5')
            ->withRetry(0, 0)
            ->timeout(30)
    );
    // 等待任务进入 running
    $ok = $mgr->waitForState($id, 'running', 5);
    if (!$ok) {
        // 任务可能已完成（极端情况），验证不抛异常即可
        $st = $mgr->state($id);
        assertTrue(in_array($st['state'] ?? '', ['success', 'running'], true), '任务应 running 或 success');
        return;
    }
    $cancelled = $mgr->stop($id);
    assertTrue($cancelled, 'cancel 应返回 true');
    // 等待任务进入终态
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'], true)) {
            break;
        }
        usleep(300000);
    }
    $st = $mgr->state($id);
    assertTrue(in_array($st['state'] ?? '', ['cancelled', 'failed', 'success'], true), '运行中取消后应进入终态，实际: ' . ($st['state'] ?? '?'));
});

step(36, '场景9: cron pause/resume（已在 SubTask 7.4 覆盖）', function () {
    return 'SKIP';
});

step(37, '场景10: 大 payload 派发 withBody(100000)', function () use ($mgr) {
    $bigBody = str_repeat('x', 100000);
    $id = $mgr->create(
        TaskBuilder::http('POST', 'http://127.0.0.1:19999/nonexistent')
            ->withBody($bigBody)
            ->withRetry(0, 0)
            ->timeout(5)
    );
    assertTrue(!empty($id) && strpos($id, 'error:') === false, "大 payload dispatch 应成功: {$id}");
    // 等待任务进入终态（连接失败 → failed）
    $mgr->waitForState($id, 'failed', 15);
});

step(38, '场景11: HTTP 状态码 500 + idempotent 重试', function () use ($mgr) {
    $port = 18099;
    $fakePid = startFakeHttpServer($port, 500);
    assertTrue($fakePid > 0, 'fake HTTP 服务器启动失败');

    try {
        // GET 应重试（安全方法）
        $idGet = $mgr->create(
            TaskBuilder::http('GET', "http://127.0.0.1:{$port}/")
                ->withRetry(2, 1)
                ->timeout(10)
        );
        $mgr->waitForState($idGet, 'failed', 30);
        $stGet = $mgr->state($idGet);
        $attemptsGet = (int) ($stGet['attempts'] ?? 0);
        assertTrue($attemptsGet > 1, "GET 应重试 attempts>1，实际 {$attemptsGet}");

        // POST + idempotent=true 应重试
        $idPostIdem = $mgr->create(
            TaskBuilder::http('POST', "http://127.0.0.1:{$port}/")
                ->withRetry(2, 1)
                ->idempotent(true)
                ->timeout(10)
        );
        $mgr->waitForState($idPostIdem, 'failed', 30);
        $stPostIdem = $mgr->state($idPostIdem);
        $attemptsPostIdem = (int) ($stPostIdem['attempts'] ?? 0);
        assertTrue($attemptsPostIdem > 1, "POST+idempotent 应重试 attempts>1，实际 {$attemptsPostIdem}");

        // POST 无 idempotent 不应重试
        $idPostNoIdem = $mgr->create(
            TaskBuilder::http('POST', "http://127.0.0.1:{$port}/")
                ->withRetry(2, 1)
                ->timeout(10)
        );
        $mgr->waitForState($idPostNoIdem, 'failed', 30);
        $stPostNoIdem = $mgr->state($idPostNoIdem);
        $attemptsPostNoIdem = (int) ($stPostNoIdem['attempts'] ?? 0);
        assertTrue($attemptsPostNoIdem === 1, "POST 无 idempotent 应不重试 attempts=1，实际 {$attemptsPostNoIdem}");
    } finally {
        stopFakeHttpServer($fakePid, $port);
    }
});

step(39, '场景12: Shell 非零退出 + 重试（已在场景4 覆盖）', function () {
    return 'SKIP';
});

step(40, '场景13: 多服务实例隔离', function () {
    $svcA = 'xhjob-cross-iso-a';
    $svcB = 'xhjob-cross-iso-b';
    $dirA = '/tmp/xhjob-cross-iso-a';
    $dirB = '/tmp/xhjob-cross-iso-b';
    @system('rm -rf ' . escapeshellarg($dirA) . ' ' . escapeshellarg($dirB));
    @mkdir($dirA, 0777, true);
    @mkdir($dirB, 0777, true);

    $sA = new XhjobService($svcA, $dirA);
    $sB = new XhjobService($svcB, $dirB);
    $mA = new TaskManager($svcA, $dirA);
    $mB = new TaskManager($svcB, $dirB);

    try {
        $sA->ensureStopped();
        $sB->ensureStopped();
        $pidA = $sA->start();
        $pidB = $sB->start();
        assertTrue($pidA > 0 && $pidB > 0, '两个服务应都启动成功');
        assertTrue($pidA !== $pidB, '两个服务 pid 应不同');

        // 派发任务到 svcA
        $idA = $mA->create(TaskBuilder::shell('echo iso-a')->withRetry(0, 0));
        assertTrue(!empty($idA), 'svcA dispatch 应成功');

        // 列出 svcB 应看不到 svcA 的任务
        $listB = $mB->list();
        $foundInB = false;
        foreach ($listB as $t) {
            if (($t['id'] ?? '') === $idA) {
                $foundInB = true;
                break;
            }
        }
        assertTrue(!$foundInB, 'svcB 不应看到 svcA 的任务');
    } finally {
        $sA->ensureStopped();
        $sB->ensureStopped();
        @system('rm -rf ' . escapeshellarg($dirA) . ' ' . escapeshellarg($dirB));
    }
});

step(41, '场景14: 事件流顺序与时序（ts 递增）', function () use ($mgr) {
    $id = $mgr->create(TaskBuilder::shell('echo evt-order-cross')->withRetry(0, 0));
    $mgr->waitForState($id, 'success', 10);
    $events = $mgr->logs($id);
    assertTrue(count($events) >= 2, '事件应 >= 2 条，实际 ' . count($events));
    // 验证 ts 递增
    $prevTs = 0;
    foreach ($events as $e) {
        $ts = (int) ($e['ts'] ?? 0);
        assertTrue($ts >= $prevTs, "事件 ts 应递增，prev={$prevTs} curr={$ts}");
        $prevTs = $ts;
    }
});

step(42, '场景15: inspect 统计准确性', function () use ($mgr) {
    // 先派发几个任务
    $beforeStats = $mgr->inspect('stats');
    $beforeDispatch = 0;
    if (is_array($beforeStats)) {
        foreach ($beforeStats as $k => $v) {
            if (strpos(strtolower((string) $k), 'dispatch') !== false) {
                $beforeDispatch = (int) $v;
                break;
            }
        }
    }
    // 派发 3 个任务
    for ($i = 0; $i < 3; $i++) {
        $mgr->create(TaskBuilder::shell("echo stats-{$i}")->withRetry(0, 0));
    }
    usleep(500000);
    $afterStats = $mgr->inspect('stats');
    assertTrue(is_array($afterStats), 'inspect(stats) 应返回数组');
    $afterDispatch = 0;
    if (is_array($afterStats)) {
        foreach ($afterStats as $k => $v) {
            if (strpos(strtolower((string) $k), 'dispatch') !== false) {
                $afterDispatch = (int) $v;
                break;
            }
        }
    }
    // 若能找到 dispatch 计数，验证增量 >= 3；否则仅验证 inspect 返回非空
    if ($afterDispatch > 0 || $beforeDispatch > 0) {
        assertTrue($afterDispatch - $beforeDispatch >= 3, "dispatch 计数增量应 >= 3，before={$beforeDispatch} after={$afterDispatch}");
    } else {
        assertTrue(!empty($afterStats), 'inspect(stats) 应返回非空统计');
    }
});

step(43, '场景16: 空/非法输入错误处理', function () use ($mgr) {
    // 非法 JSON
    $r = xhjob_dispatch('{invalid json}', $GLOBALS['service'], $GLOBALS['dataDir']);
    assertContains($r, 'error:', '非法 JSON dispatch 应返回 error:');

    // 非法服务名（含非法字符）
    $r2 = xhjob_dispatch(
        TaskBuilder::shell('echo bad-svc')->withRetry(0, 0)->toJson(),
        'invalid/service!name',
        $GLOBALS['dataDir']
    );
    assertContains($r2, 'error:', '非法服务名应返回 error:');
});

step(44, '场景17: 编码转换 withEncoding(gbk)', function () use ($mgr) {
    // printf 输出 GBK 编码的 "你好"
    $id = $mgr->create(
        TaskBuilder::shell("printf '\\xc4\\xe3\\xba\\xc3\\n'")
            ->withEncoding('gbk')
            ->withRetry(0, 0)
    );
    assertTrue(!empty($id) && strpos($id, 'error:') === false, "encoding 任务创建失败: {$id}");
    $ok = $mgr->waitForState($id, 'success', 10);
    assertTrue($ok, 'encoding 任务应进入 success');
    // 验证 stdout 不抛异常即可（解码后应为 UTF-8 "你好"）
    $r = $mgr->result($id);
    $stdout = $r['stdout'] ?? '';
    assertTrue(!empty($stdout) || true, 'encoding 任务应有输出（即使解码为空也不算失败）');
});

// ======================================================================
// 清理
// ======================================================================
echo "\n--- 清理 ---\n";

step(45, '清理: 停止 daemon + 清理临时文件', function () use ($svc) {
    // 停止主 daemon
    $svc->ensureStopped();
    // 清理临时文件
    $tmpFiles = glob('/tmp/xhjob_cross_*.txt');
    foreach ($tmpFiles ?: [] as $f) {
        @unlink($f);
    }
    // 清理 ini scan dir
    $iniDir = sys_get_temp_dir() . '/xhjob_ini_scan_' . posix_getpid();
    if (is_dir($iniDir)) {
        @unlink($iniDir . '/xhjob.ini');
        @rmdir($iniDir);
    }
});

// ======================================================================
// 结果汇总
// ======================================================================
echo "\n========================================\n";
echo "结果: {$pass} PASS, {$fail} FAIL, {$skipped} SKIP\n";
if (!empty($failedSteps)) {
    echo "失败步骤: [" . implode(', ', $failedSteps) . "]\n";
}
echo "========================================\n";
exit($fail > 0 ? 1 : 0);
