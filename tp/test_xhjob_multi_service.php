<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 多服务隔离测试
// +----------------------------------------------------------------------
// | 验证第三方库在同一 PHP 进程内可同时操作多个独立 daemon 服务，
// | 服务之间任务 / 状态 / 生命周期完全隔离，互不影响。
// |
// | 场景：
// |   - cron-svc：定时任务服务（cron 周期任务）
// |   - queue-svc：后台任务队列服务（chain / group / 一次性任务）
// |   - 客户端：单独连接这 2 个服务进行操作
// +----------------------------------------------------------------------
declare(strict_types=1);

require __DIR__ . '/vendor/autoload.php';

use Xhjob\TaskManager;
use Xhjob\TaskBuilder;
use Xhjob\XhjobService;
use Xhjob\Exception\ServiceNotRunningException;

// 两个服务的独立数据目录，确保文件层完全隔离
const CRON_SVC  = 'cron-svc';
const QUEUE_SVC = 'queue-svc';
const CRON_DIR  = '/tmp/xhjob-multi-cron';
const QUEUE_DIR = '/tmp/xhjob-multi-queue';

$passes = 0;
$fails  = 0;
$failed = [];

function step(string $title, bool $ok, string $detail = ''): void
{
    global $passes, $fails, $failed;
    $tag = $ok ? 'PASS' : 'FAIL';
    echo "[" . $tag . "] {$title}";
    if ($detail !== '') echo " :: {$detail}";
    echo "\n";
    if ($ok) {
        $passes++;
    } else {
        $fails++;
        $failed[] = $title;
    }
}

// 清理旧数据，确保测试环境干净
function cleanup(): void
{
    foreach ([CRON_DIR, QUEUE_DIR] as $dir) {
        if (is_dir($dir)) {
            $files = glob($dir . '/*');
            foreach ($files ?: [] as $f) {
                @unlink($f);
            }
            @rmdir($dir);
        }
    }
    @mkdir(CRON_DIR, 0777, true);
    @mkdir(QUEUE_DIR, 0777, true);
}

cleanup();

echo "=== 多服务隔离测试 ===\n";
echo "cron-svc  数据目录: " . CRON_DIR . "\n";
echo "queue-svc 数据目录: " . QUEUE_DIR . "\n\n";

// ---------------------------------------------------------------------
// Step 1：启动两个独立 daemon 服务
// ---------------------------------------------------------------------
echo "--- Step 1：启动两个独立服务 ---\n";

$cronService = new XhjobService(CRON_SVC, CRON_DIR);
$queueService = new XhjobService(QUEUE_SVC, QUEUE_DIR);

try {
    $cronPid = $cronService->start();
    step('启动 cron-svc', $cronPid > 0, "pid={$cronPid}");
} catch (ServiceNotRunningException $e) {
    step('启动 cron-svc', false, $e->getMessage());
}

try {
    $queuePid = $queueService->start();
    step('启动 queue-svc', $queuePid > 0, "pid={$queuePid}");
} catch (ServiceNotRunningException $e) {
    step('启动 queue-svc', false, $e->getMessage());
}

// ---------------------------------------------------------------------
// Step 2：验证两个服务 PID 不同（独立进程）
// ---------------------------------------------------------------------
echo "\n--- Step 2：验证进程隔离 ---\n";
$cronStatus  = $cronService->status();
$queueStatus = $queueService->status();
step(
    '两个服务 PID 不同（独立进程）',
    $cronStatus['pid'] !== $queueStatus['pid'] && $cronStatus['pid'] > 0 && $queueStatus['pid'] > 0,
    "cron-pid={$cronStatus['pid']}, queue-pid={$queueStatus['pid']}"
);
step(
    '两个服务状态都为 running',
    $cronStatus['running'] === true && $queueStatus['running'] === true,
    "cron.running={$cronStatus['running']}, queue.running={$queueStatus['running']}"
);

// ---------------------------------------------------------------------
// Step 3：构造两个 TaskManager 实例（客户端）
// ---------------------------------------------------------------------
echo "\n--- Step 3：构造两个 TaskManager 客户端 ---\n";
$cronMgr  = new TaskManager(CRON_SVC, CRON_DIR);
$queueMgr = new TaskManager(QUEUE_SVC, QUEUE_DIR);
step('cron-svc TaskManager 构造', $cronMgr->getName() === CRON_SVC, "name={$cronMgr->getName()}");
step('queue-svc TaskManager 构造', $queueMgr->getName() === QUEUE_SVC, "name={$queueMgr->getName()}");

// ---------------------------------------------------------------------
// Step 4：cron-svc 派发 cron 周期任务
// ---------------------------------------------------------------------
echo "\n--- Step 4：cron-svc 派发 cron 周期任务 ---\n";
try {
    $cronTaskId = $cronMgr->create(
        TaskBuilder::shell('echo cron-tick')
            ->cron('*/1 * * * * *')  // 每秒触发
            ->tag('cron-svc')
            ->persist(true)
    );
    step('cron-svc 派发 cron 任务', !str_starts_with($cronTaskId, 'error:'), "id={$cronTaskId}");
} catch (\Throwable $e) {
    $cronTaskId = null;
    step('cron-svc 派发 cron 任务', false, $e->getMessage());
}

// ---------------------------------------------------------------------
// Step 5：queue-svc 派发 chain 流水线
// ---------------------------------------------------------------------
echo "\n--- Step 5：queue-svc 派发 chain 流水线 ---\n";
try {
    $chainId = $queueMgr->createChain([
        TaskBuilder::shell('echo step-1 > /tmp/xhjob-multi-chain.log'),
        TaskBuilder::shell('echo step-2 >> /tmp/xhjob-multi-chain.log'),
        TaskBuilder::shell('echo step-3 >> /tmp/xhjob-multi-chain.log'),
    ]);
    step('queue-svc 派发 chain', !str_starts_with($chainId, 'error:'), "chain_id={$chainId}");
} catch (\Throwable $e) {
    $chainId = null;
    step('queue-svc 派发 chain', false, $e->getMessage());
}

// ---------------------------------------------------------------------
// Step 6：queue-svc 派发 group 并行批处理
// ---------------------------------------------------------------------
echo "\n--- Step 6：queue-svc 派发 group 并行批处理 ---\n";
try {
    $groupId = $queueMgr->createGroup([
        TaskBuilder::shell('echo group-a'),
        TaskBuilder::shell('echo group-b'),
        TaskBuilder::shell('echo group-c'),
    ]);
    step('queue-svc 派发 group', !str_starts_with($groupId, 'error:'), "group_id={$groupId}");
} catch (\Throwable $e) {
    $groupId = null;
    step('queue-svc 派发 group', false, $e->getMessage());
}

// ---------------------------------------------------------------------
// Step 7：验证任务隔离——cron-svc 看不到 queue-svc 的任务
// ---------------------------------------------------------------------
echo "\n--- Step 7：验证任务隔离（跨服务不可见）---\n";

// 等待任务执行
usleep(1500000);

if ($cronTaskId !== null) {
    $cronList = $cronMgr->list();
    $cronHasCronTask = false;
    $cronHasQueueTask = false;
    foreach ($cronList as $t) {
        if (($t['id'] ?? '') === $cronTaskId) {
            $cronHasCronTask = true;
        } else {
            // cron-svc 仅创建过一个 cron 任务（$cronTaskId），
            // 列表中任何其它任务都说明发生了跨服务隔离泄漏。
            // （原实现用 in_array('queue-svc', tags) 判断，但
            // queue-svc 的 chain/group 子任务并未打 'queue-svc' 标签，
            // 导致该断言恒为 true、无法检出泄漏；同时若 list 返回的
            // tags 为 JSON 字符串还会触发 in_array 的 TypeError。）
            $cronHasQueueTask = true;
        }
    }
    step('cron-svc 能看到自己的 cron 任务', $cronHasCronTask, "count=" . count($cronList));
    step('cron-svc 看不到 queue-svc 的 chain/group 任务', !$cronHasQueueTask, "queue-task-in-cron=" . ($cronHasQueueTask ? 'yes' : 'no'));
}

if (isset($chainId) && $chainId !== null && !str_starts_with($chainId, 'error:')) {
    $queueList = $queueMgr->list();
    $queueHasChain = false;
    $queueHasCronTask = false;
    foreach ($queueList as $t) {
        // chain 子任务的 meta 中携带 xhjob_chain_id
        $meta = json_decode($t['meta'] ?? '{}', true);
        if (isset($meta['xhjob_chain_id']) && $meta['xhjob_chain_id'] === $chainId) {
            $queueHasChain = true;
        }
        if (($t['id'] ?? '') === $cronTaskId) $queueHasCronTask = true;
    }
    step('queue-svc 能看到自己的 chain 子任务', $queueHasChain, "count=" . count($queueList));
    step('queue-svc 看不到 cron-svc 的 cron 任务', !$queueHasCronTask, "cron-task-in-queue=" . ($queueHasCronTask ? 'yes' : 'no'));
}

// ---------------------------------------------------------------------
// Step 8：验证状态隔离——cron-svc 的任务状态在 queue-svc 中查询失败
// ---------------------------------------------------------------------
echo "\n--- Step 8：验证状态隔离（跨服务查询返回空）---\n";
if ($cronTaskId !== null) {
    // 用 queue-svc 客户端查询 cron-svc 的任务 ID，应返回 null/空
    $crossState = $queueMgr->get($cronTaskId);
    step('queue-svc 查询 cron-svc 的任务返回 null', $crossState === null, "result=" . ($crossState === null ? 'null' : 'not-null'));
}

// ---------------------------------------------------------------------
// Step 9：验证文件隔离——PID/sock/db 文件分开存放
// ---------------------------------------------------------------------
echo "\n--- Step 9：验证文件层隔离 ---\n";
$cronFiles  = glob(CRON_DIR . '/*') ?: [];
$queueFiles = glob(QUEUE_DIR . '/*') ?: [];
$cronHasPid  = count(array_filter($cronFiles, fn($f) => str_contains(basename($f), CRON_SVC))) > 0;
$queueHasPid = count(array_filter($queueFiles, fn($f) => str_contains(basename($f), QUEUE_SVC))) > 0;
step('cron-svc 数据目录包含自己的文件', $cronHasPid, "files=" . count($cronFiles));
step('queue-svc 数据目录包含自己的文件', $queueHasPid, "files=" . count($queueFiles));

// 验证 PID 文件名包含服务名（不同服务不同文件）
$cronPidFile  = CRON_DIR . '/xhjob.' . CRON_SVC . '.pid';
$queuePidFile = QUEUE_DIR . '/xhjob.' . QUEUE_SVC . '.pid';
step('cron-svc PID 文件独立存在', file_exists($cronPidFile), $cronPidFile);
step('queue-svc PID 文件独立存在', file_exists($queuePidFile), $queuePidFile);

// ---------------------------------------------------------------------
// Step 10：验证生命周期隔离——停止 cron-svc 不影响 queue-svc
// ---------------------------------------------------------------------
echo "\n--- Step 10：验证生命周期隔离（停 cron-svc 不影响 queue-svc）---\n";

// 停止 cron-svc
$cronStopped = $cronService->stop();
$cronService->wait(5, false);
$cronStatusAfterStop = $cronService->status();
step('停止 cron-svc 成功', $cronStopped && !$cronStatusAfterStop['running'], "running={$cronStatusAfterStop['running']}");

// queue-svc 仍应运行
$queueStatusAfterCronStop = $queueService->status();
step('停止 cron-svc 后 queue-svc 仍在运行', $queueStatusAfterCronStop['running'] === true, "queue.running={$queueStatusAfterCronStop['running']}, pid={$queueStatusAfterCronStop['pid']}");

// queue-svc 仍能派发新任务
try {
    $newTaskId = $queueMgr->create(TaskBuilder::shell('echo queue-still-alive'));
    step('cron-svc 停止后 queue-svc 仍能派发新任务', !str_starts_with($newTaskId, 'error:'), "id={$newTaskId}");
} catch (\Throwable $e) {
    step('cron-svc 停止后 queue-svc 仍能派发新任务', false, $e->getMessage());
}

// ---------------------------------------------------------------------
// Step 11：清理——停止 queue-svc
// ---------------------------------------------------------------------
echo "\n--- Step 11：清理——停止 queue-svc ---\n";
$queueStopped = $queueService->stop();
$queueService->wait(5, false);
$queueStatusFinal = $queueService->status();
step('停止 queue-svc 成功', $queueStopped && !$queueStatusFinal['running'], "running={$queueStatusFinal['running']}");

// ---------------------------------------------------------------------
// 结果汇总
// ---------------------------------------------------------------------
echo "\n=== 测试结果 ===\n";
echo "passed: {$passes}, failed: {$fails}\n";
if ($fails > 0) {
    echo "失败步骤: [" . implode(', ', $failed) . "]\n";
    exit(1);
}
exit(0);
