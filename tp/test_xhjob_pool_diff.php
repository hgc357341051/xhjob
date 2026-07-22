#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 两种池模式真实差异验证
// +----------------------------------------------------------------------
// | 通过 /proc/{pid}/task 读取 daemon 实际线程名 + 并发时间戳分析，
// | 证明 coroutine（tokio async）与 thread（std::thread）是两种本质不同的并发模型。
// |
// | 验证维度：
// |   1. 线程名：coroutine 模式有 xhjob-tokio，thread 模式有 xhjob-worker-N
// |   2. 并发行为：coroutine 模式 4 任务全部同时执行；thread 模式(pool=2) 分 2 批
// |   3. 总耗时：coroutine ≈ 1×sleep；thread ≈ 2×sleep（分批排队）
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

if (!extension_loaded('xhjob')) {
    $so = '/workspace/releases/xhjob-php8.2-linux-x86_64.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

$SERVICE    = 'pool-diff-test';
$DATA_DIR   = '/tmp/xhjob-pool-diff';
$MARKER_DIR = '/tmp/xhjob-pool-diff-markers';
$TASK_COUNT = 4;
$SLEEP_SEC  = 3;

$pass = 0;
$fail = 0;

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail;
    echo "  [{$n}] {$desc} ... ";
    try {
        $fn();
        $pass++;
        echo "PASS\n";
    } catch (\Throwable $e) {
        $fail++;
        echo "FAIL: " . $e->getMessage() . "\n";
    }
}

function assertTrue(bool $cond, string $msg): void
{
    if (!$cond) {
        throw new \Exception($msg);
    }
}

/**
 * 读取 daemon 进程的所有线程名
 *
 * @param int $pid daemon PID
 * @return array tid => thread_name
 */
function readDaemonThreads(int $pid): array
{
    $taskDir = "/proc/{$pid}/task";
    if (!is_dir($taskDir)) {
        return [];
    }
    $threads = [];
    $tids = array_diff(scandir($taskDir), ['.', '..']);
    foreach ($tids as $tid) {
        $comm = @file_get_contents("{$taskDir}/{$tid}/comm");
        if ($comm !== false) {
            $threads[$tid] = trim($comm);
        }
    }
    return $threads;
}

/**
 * 统计 xhjob 相关线程
 */
function countXhjobThreads(array $threads): array
{
    $tokio = [];
    $workers = [];
    foreach ($threads as $tid => $name) {
        if (strpos($name, 'xhjob-tokio') !== false) {
            $tokio[$tid] = $name;
        } elseif (strpos($name, 'xhjob-worker') !== false) {
            $workers[$tid] = $name;
        }
    }
    return ['tokio' => $tokio, 'workers' => $workers, 'all' => $threads];
}

/**
 * 并发测试：派发 N 个 sleep 任务，每个任务在开始时写入纳秒时间戳
 *
 * @return array{deltas:array, total_ms:float, ids:array}
 */
function concurrencyTest(TaskManager $mgr, string $markerDir, int $count, int $sleepSec): array
{
    @mkdir($markerDir, 0777, true);
    for ($i = 0; $i < $count; $i++) {
        @unlink("{$markerDir}/task-{$i}.start");
    }

    $dispatchStart = microtime(true);
    $ids = [];
    for ($i = 0; $i < $count; $i++) {
        $cmd = "date +%s.%N > {$markerDir}/task-{$i}.start && sleep {$sleepSec}";
        $ids[] = $mgr->create(TaskBuilder::shell($cmd)->timeout(30));
    }

    // 等待全部完成
    foreach ($ids as $i => $id) {
        $mgr->waitForState($id, 'success', 30);
    }
    $totalMs = (microtime(true) - $dispatchStart) * 1000.0;

    // 读取时间戳并计算 delta（毫秒）
    $timestamps = [];
    for ($i = 0; $i < $count; $i++) {
        $ts = @file_get_contents("{$markerDir}/task-{$i}.start");
        $timestamps[$i] = $ts ? (float) trim($ts) : 0.0;
    }
    $min = min($timestamps);
    $deltas = array_map(fn($t) => ($t - $min) * 1000.0, $timestamps);
    sort($deltas);

    return ['deltas' => $deltas, 'total_ms' => $totalMs, 'ids' => $ids];
}

echo "=== 两种池模式真实差异验证 ===\n";
echo "service={$SERVICE} data_dir={$DATA_DIR}\n";
echo "PHP extension: " . (extension_loaded('xhjob') ? 'loaded' : 'NOT loaded') . "\n";
echo "CPU cores: " . trim(@shell_exec('nproc')) . "\n";
echo "并发测试: {$TASK_COUNT} 个 sleep({$SLEEP_SEC}) 任务\n\n";

$coroutineResult = null;
$threadResult    = null;

// ============================================================
// 阶段 1：协程模式
// ============================================================
echo "--- 阶段 1：协程模式（coroutine）---\n";
echo "    底层：tokio async runtime + Semaphore(1024)\n";
echo "    任务通过 tokio::spawn 调度，async/await 协作式并发\n";

putenv('XHJOB_POOL_MODE=coroutine');
putenv('XHJOB_COROUTINE_POOL_SIZE=1024');
@system("rm -rf {$DATA_DIR}");
@mkdir($DATA_DIR, 0777, true);

step(1, '启动 daemon (coroutine)', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "pid 应>0，实际 {$pid}");
    $svc->wait(10, true);
});

$mgr = new TaskManager($SERVICE, $DATA_DIR);

step(2, '触发池初始化（派发 echo 任务）', function () use ($mgr) {
    $id = $mgr->create(TaskBuilder::shell('echo init'));
    assertTrue($mgr->waitForState($id, 'success', 10), '初始化任务应成功');
});

step(3, '线程名验证：xhjob-tokio 存在，xhjob-worker 不存在', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $pid = $svc->status()['pid'] ?? 0;
    assertTrue($pid > 0, 'daemon pid 应>0');

    $threads = readDaemonThreads($pid);
    $counts  = countXhjobThreads($threads);

    $xhjobNames = array_merge($counts['tokio'], $counts['workers']);
    echo "\n       线程总数: " . count($threads) . ", xhjob 相关: " . count($xhjobNames);
    echo "\n       xhjob 线程: " . implode(', ', array_slice($xhjobNames, 0, 8));

    assertTrue(count($counts['tokio']) > 0, '协程模式应存在 xhjob-tokio 线程');
    assertTrue(count($counts['workers']) === 0,
        '协程模式不应存在 xhjob-worker 线程，实际有 ' . count($counts['workers']));
    echo "（tokio=" . count($counts['tokio']) . ', workers=0）';
});

step(4, "并发测试：{$TASK_COUNT} 个 sleep({$SLEEP_SEC}) 时间戳分布", function () use ($mgr, $MARKER_DIR, $TASK_COUNT, $SLEEP_SEC, &$coroutineResult) {
    $result = concurrencyTest($mgr, $MARKER_DIR, $TASK_COUNT, $SLEEP_SEC);
    $coroutineResult = $result;

    $deltas   = $result['deltas'];
    $totalSec  = $result['total_ms'] / 1000.0;
    $earlyCnt  = count(array_filter($deltas, fn($d) => $d < 1000));

    echo "\n       delta(ms): [" . implode(', ', array_map(fn($d) => round($d, 1), $deltas)) . "]";
    echo "\n       总耗时: " . round($totalSec, 2) . "s";

    assertTrue($earlyCnt === $TASK_COUNT,
        "协程模式应全部 {$TASK_COUNT} 个任务在 1s 内开始，实际 {$earlyCnt}");
    assertTrue($totalSec < $SLEEP_SEC * 2,
        "总耗时应 < " . ($SLEEP_SEC * 2) . "s，实际 " . round($totalSec, 2) . "s");
    echo "（全部同时执行）";
});

step(5, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
});

// ============================================================
// 阶段 2：线程模式
// ============================================================
echo "\n--- 阶段 2：线程模式（thread, pool_size=2）---\n";
echo "    底层：std::thread + crossbeam-channel\n";
echo "    每个任务在独立 OS 线程中 block_on 执行，并发度=线程数\n";

putenv('XHJOB_POOL_MODE=thread');
putenv('XHJOB_THREAD_POOL_SIZE=2');
@system("rm -rf {$DATA_DIR}");
@mkdir($DATA_DIR, 0777, true);

step(1, '启动 daemon (thread, 2 workers)', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "pid 应>0，实际 {$pid}");
    $svc->wait(10, true);
});

$mgr2 = new TaskManager($SERVICE, $DATA_DIR);

step(2, '触发池初始化（派发 echo 任务）', function () use ($mgr2) {
    $id = $mgr2->create(TaskBuilder::shell('echo init'));
    assertTrue($mgr2->waitForState($id, 'success', 10), '初始化任务应成功');
});

step(3, '线程名验证：xhjob-worker-* 存在且数量=2', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $pid = $svc->status()['pid'] ?? 0;
    assertTrue($pid > 0, 'daemon pid 应>0');

    $threads = readDaemonThreads($pid);
    $counts  = countXhjobThreads($threads);

    $xhjobNames = array_merge($counts['tokio'], $counts['workers']);
    echo "\n       线程总数: " . count($threads) . ", xhjob 相关: " . count($xhjobNames);
    echo "\n       xhjob 线程: " . implode(', ', array_slice($xhjobNames, 0, 8));

    assertTrue(count($counts['workers']) === 2,
        '线程模式应有 2 个 xhjob-worker 线程，实际 ' . count($counts['workers']));
    echo '（workers=' . implode(',', array_values($counts['workers'])) . '）';
});

step(4, "并发测试：{$TASK_COUNT} 个 sleep({$SLEEP_SEC}) 时间戳分布", function () use ($mgr2, $MARKER_DIR, $TASK_COUNT, $SLEEP_SEC, &$threadResult) {
    $result = concurrencyTest($mgr2, $MARKER_DIR, $TASK_COUNT, $SLEEP_SEC);
    $threadResult = $result;

    $deltas   = $result['deltas'];
    $totalSec  = $result['total_ms'] / 1000.0;
    $earlyCnt  = count(array_filter($deltas, fn($d) => $d < 1000));
    $lateCnt   = $TASK_COUNT - $earlyCnt;

    echo "\n       delta(ms): [" . implode(', ', array_map(fn($d) => round($d, 1), $deltas)) . "]";
    echo "\n       总耗时: " . round($totalSec, 2) . "s";

    assertTrue($earlyCnt === 2 && $lateCnt === 2,
        "线程模式(2 workers)应 2 个立即开始、2 个延迟，实际 early={$earlyCnt} late={$lateCnt}");
    assertTrue($totalSec >= $SLEEP_SEC * 2 * 0.8 && $totalSec < $SLEEP_SEC * 3,
        "总耗时应约 " . ($SLEEP_SEC * 2) . "s，实际 " . round($totalSec, 2) . "s");
    echo '（分 2 批：前 2 个立即，后 2 个延迟 ' . round($deltas[2] / 1000, 1) . 's）';
});

step(5, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
});

// ============================================================
// 对比总结
// ============================================================
echo "\n=== 对比总结 ===\n";

$corDelta = $coroutineResult
    ? round($coroutineResult['deltas'][0]) . '~' . round(end($coroutineResult['deltas'])) . 'ms'
    : 'N/A';
$thrDelta = $threadResult
    ? round($threadResult['deltas'][0]) . '~' . round(end($threadResult['deltas'])) . 'ms'
    : 'N/A';
$corTotal = $coroutineResult ? round($coroutineResult['total_ms'] / 1000, 2) . 's' : 'N/A';
$thrTotal = $threadResult ? round($threadResult['total_ms'] / 1000, 2) . 's' : 'N/A';

$line = function (string $a, string $b, string $c) {
    printf("%-16s | %-28s | %s\n", $a, $b, $c);
};

$line('维度', '协程模式(coroutine)', '线程模式(thread,pool=2)');
echo str_repeat('-', 78) . "\n";
$line('执行线程名', 'xhjob-tokio', 'xhjob-worker-N');
$line('线程来源', 'tokio async runtime', 'std::thread + crossbeam');
$line('并发控制', 'Semaphore(1024)', '线程数(2)');
$line('并发模型', '协作式(async/await yield)', '抢占式(每任务独占线程)');
$line('delta 分布', $corDelta, $thrDelta);
$line('总耗时', $corTotal, $thrTotal);
$line("{$TASK_COUNT} 任务并发", '全部同时', '分 2 批(每批 2 个)');

echo "\n结论：\n";
echo "  协程模式：任务在 tokio async runtime 上调度，通过 Semaphore 控制并发。\n";
echo "    4 个 sleep(3) 全部同时执行（async/await 在 sleep 时 yield），总耗时 ≈ 3s。\n";
echo "  线程模式：每个任务在独立 OS 线程中 block_on 执行，并发度受限于线程数。\n";
echo "    4 个 sleep(3) 分 2 批执行（2 个线程 × 2 批），总耗时 ≈ 6s。\n";
echo "  两种模式功能行为一致，但并发模型本质不同：\n";
echo "    协程 = 单线程高并发(IO 密集型最优)；\n";
echo "    线程 = 真并行受限于线程数(CPU 密集型或需严格隔离时使用)。\n";

exit($fail > 0 ? 1 : 0);
