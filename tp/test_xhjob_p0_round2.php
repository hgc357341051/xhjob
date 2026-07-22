#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 — 第二轮 P0 修复验证测试
// +----------------------------------------------------------------------
// | 验证以下修复：
// |   P0-1:  xhjob_dispatch 错误返回带 "error:" 前缀
// |   P0-2:  XhjobTaskBuilder::id() 字符集校验
// |   P0-3:  daemon 端 handle_connection timeout（间接验证 daemon 健康）
// |   P0-6:  SQLite busy_timeout 已设置
// |   P0-16: withProxy() 写入顶层 proxy（非 payload.proxy）
// |   P0-18: withEncoding() 方法存在且写入正确位置
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

$SERVICE  = 'tp-p0-r2';
$DATA_DIR = '/tmp/xhjob-tp-p0-r2';

@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);

// -----------------------------------------------------------------
// P0-1: xhjob_dispatch 服务名错误应返回 "error:" 前缀
// -----------------------------------------------------------------
step(1, 'P0-1: xhjob_dispatch 非法服务名返回 "error:" 前缀', function () {
    // 服务名含非法字符 "."，validate 应失败
    // 直接调 xhjob_dispatch 函数（不通过 TaskBuilder，避免依赖 daemon）
    $taskJson = json_encode([
        'task_type' => 'shell',
        'payload' => ['cmd' => 'echo hi'],
    ]);
    $r = xhjob_dispatch($taskJson, 'invalid.name', null);
    if (!str_starts_with($r, 'error:')) {
        throw new Exception("期望以 'error:' 开头，实际: " . substr($r, 0, 80));
    }
    echo "(" . substr($r, 0, 60) . ") ";
});

// -----------------------------------------------------------------
// P0-2: withId 字符集校验
// -----------------------------------------------------------------
step(2, 'P0-2a: withId 合法字符被接受', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $svc->start();
    $svc->wait(10, true);

    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 合法 id：字母+数字+下划线+连字符
    $id = $mgr->create(
        TaskBuilder::shell('echo p0-2a')
            ->withId('my-task-001_ABC')
    );
    if (!str_starts_with($id, 'my-task-001_ABC')) {
        throw new Exception("合法 id 应被接受，实际 task_id: $id");
    }
});

step(3, 'P0-2b: withId 非法字符被拒绝（返回 error）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 非法字符：含 ":" 会破坏 PHP 端 "error:" 前缀探测契约
    // daemon 端 build() 应拒绝并返回 error
    try {
        $id = $mgr->create(
            TaskBuilder::shell('echo p0-2b')
                ->withId('error: malicious-id')
        );
        // 如果没抛异常，检查返回值不应是恶意 id
        if ($id === 'error: malicious-id') {
            throw new Exception("非法 id 不应被接受");
        }
        // 也不应是其他 error 前缀（说明被接受了但出错）
        throw new Exception("非法 id 应被 daemon 拒绝，实际返回: $id");
    } catch (Xhjob\Exception\InvalidTaskConfigException $e) {
        // 期望的路径：daemon 返回 error:..., PHP 端抛 InvalidTaskConfigException
        $msg = $e->getMessage();
        if (strpos($msg, 'invalid') === false && strpos($msg, 'malicious') === false) {
            throw new Exception("错误消息不含 invalid/malicious: $msg");
        }
        echo "(" . substr($msg, 0, 50) . "... rejected) ";
    }
});

step(4, 'P0-2c: withId 含 SQL 注入字符被拒绝', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    try {
        $id = $mgr->create(
            TaskBuilder::shell('echo p0-2c')
                ->withId("'; DROP TABLE tasks; --")
        );
        throw new Exception("SQL 注入 id 应被拒绝，实际返回: $id");
    } catch (Xhjob\Exception\InvalidTaskConfigException $e) {
        // 期望路径：daemon 拒绝
        $msg = $e->getMessage();
        if (strpos($msg, 'invalid') === false) {
            throw new Exception("错误消息不含 invalid: $msg");
        }
        echo "(rejected) ";
    }
    // 验证 tasks 表还在（没被注入删除）
    $list = $mgr->list();
    if (!is_array($list)) {
        throw new Exception("tasks 表查询失败，可能被注入");
    }
});

// -----------------------------------------------------------------
// P0-3: daemon 端 timeout（间接验证）
// -----------------------------------------------------------------
step(5, 'P0-3: daemon 端 connection timeout（间接验证 daemon 健康）', function () use ($SERVICE, $DATA_DIR) {
    // daemon 端 15s timeout 是被动的，无法直接测试。
    // 间接验证：daemon 在多个 IPC 请求后仍稳定响应。
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    for ($i = 0; $i < 5; $i++) {
        $id = $mgr->create(TaskBuilder::shell("echo p0-3-$i")->withRetry(0, 0));
        $mgr->waitForState($id, 'success', 10);
        $st = $mgr->state($id);
        if (($st['state'] ?? '') !== 'success') {
            throw new Exception("第 $i 个请求后 daemon 不健康: " . json_encode($st));
        }
    }
    echo "(5 requests OK) ";
});

// -----------------------------------------------------------------
// P0-6: SQLite busy_timeout 已设置
// -----------------------------------------------------------------
step(6, 'P0-6: SQLite busy_timeout 已设置（间接验证）', function () use ($SERVICE, $DATA_DIR) {
    $dbFile = "$DATA_DIR/xhjob.$SERVICE.db";
    if (!file_exists($dbFile)) {
        throw new Exception("DB 文件不存在: $dbFile");
    }
    // busy_timeout 是 daemon 连接的 pragma，无法从外部直接读取。
    // 间接验证：快速串行发起多个 dispatch + state 查询（交替读写），
    // 如果 busy_timeout 未设置，并发写入会立即 SQLITE_BUSY 报错。
    // 串行快速请求也会触发锁竞争（daemon 内部多个 tokio task）。
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    for ($i = 0; $i < 10; $i++) {
        $id = $mgr->create(
            TaskBuilder::shell("echo p0-6-busy-$i")->withRetry(0, 0)
        );
        // 立即查询 state（读写交替）
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === '') {
            throw new Exception("第 $i 个请求 state 查询失败: " . json_encode($st));
        }
    }
    echo "(10 rapid dispatch+state OK) ";
});

// -----------------------------------------------------------------
// P0-16: withProxy() 写入顶层 proxy（非 payload.proxy）
// -----------------------------------------------------------------
step(7, 'P0-16: withProxy() 写入顶层 proxy 字段', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 构造一个 http 任务并设置 proxy
    // 注意：我们不实际执行这个任务（没有真实 proxy），
    // 只验证 proxy 字段被正确写入顶层位置
    $builder = TaskBuilder::http('GET', 'http://example.com/')
        ->withProxy('http://proxy.local:8080');

    // 通过 get 获取任务定义，检查 proxy 字段位置
    $id = $mgr->create($builder->withRetry(0, 0));
    $raw = $mgr->get($id);
    $task = $raw['data'] ?? $raw;

    // 顶层应有 proxy 字段
    if (($task['proxy'] ?? null) !== 'http://proxy.local:8080') {
        throw new Exception(
            "顶层 proxy 字段缺失或错误: " . json_encode($task['proxy'] ?? null)
        );
    }
    // payload.proxy 不应存在（旧 bug 的位置）
    if (isset($task['payload']['proxy'])) {
        throw new Exception(
            "payload.proxy 仍存在（旧 bug 未修复）: " . json_encode($task['payload']['proxy'])
        );
    }
    echo "(proxy at top-level) ";
});

// -----------------------------------------------------------------
// P0-18: withEncoding() 方法存在且写入正确位置
// -----------------------------------------------------------------
step(8, 'P0-18: withEncoding() 方法存在且写入 encoding 字段', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 构造一个 shell 任务并设置 encoding
    $builder = TaskBuilder::shell('echo p0-18-test')
        ->withEncoding('GBK');

    // 检查方法存在
    if (!method_exists($builder, 'withEncoding')) {
        throw new Exception("withEncoding 方法不存在");
    }

    $id = $mgr->create($builder->withRetry(0, 0));
    $raw = $mgr->get($id);
    $task = $raw['data'] ?? $raw;

    // 顶层应有 encoding 字段
    if (($task['encoding'] ?? null) !== 'GBK') {
        throw new Exception(
            "encoding 字段缺失或错误: " . json_encode($task['encoding'] ?? null)
        );
    }
    echo "(encoding=GBK) ";
});

// -----------------------------------------------------------------
// 清理
// -----------------------------------------------------------------
step(9, '清理', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    @system("rm -rf $DATA_DIR");
});

// -----------------------------------------------------------------
echo "\n========================================\n";
echo "结果: $pass PASS, $fail FAIL, $skipped SKIP\n";
echo "========================================\n";
exit($fail > 0 ? 1 : 0);
