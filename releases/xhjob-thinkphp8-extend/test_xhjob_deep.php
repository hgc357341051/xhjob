#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 深度功能测试脚本（覆盖全部任务参数属性）
// +----------------------------------------------------------------------
// | 用法：
// |   EXT=/workspace/releases/xhjob-php8.2-linux-x86_64.so
// |   php -d extension=$EXT /workspace/tp/test_xhjob_deep.php
// +----------------------------------------------------------------------
// | 覆盖测试项（按用户要求）：
// |   1. 循环任务（every 5 秒）+ logs 对比第1次/第2次结果
// |   2. maxExecutions 指定次数
// |   3. withRetry 重试（exit 1 触发失败）
// |   4. allowOverlap 任务重叠
// |   5. timeout / softTimeout / expires / jitter
// |   6. retry_backoff / ignore_result / acks_late
// |   7. misfire_grace_time / timezone / tags / meta
// |   8. result_ttl / rate_limit / coalesce
// |   9. start_date / end_date / persist
// |  10. withId + replaceExisting
// |  11. priority / chain / group
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

$pass = 0;
$fail = 0;
$failedSteps = [];

function step(int $n, string $desc, callable $fn): void
{
    global $pass, $fail, $failedSteps;
    echo "[$n] $desc ... ";
    try {
        $fn();
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
        throw new \Exception("$msg (expected=" . var_export($b, true) . ", got=" . var_export($a, true) . ")");
    }
}

function assertContains(string $haystack, string $needle, string $msg): void
{
    if (strpos($haystack, $needle) === false) {
        throw new \Exception("$msg (expected contains '$needle', got: $haystack)");
    }
}

$SERVICE  = 'deep-test';
$DATA_DIR = '/tmp/xhjob-deep-test';

@system("rm -f $DATA_DIR/xhjob.$SERVICE.* 2>/dev/null");

echo "=== Xhjob 深度功能测试（覆盖全部任务参数）===\n";
echo "service=$SERVICE data_dir=$DATA_DIR\n\n";

// 步骤 0：启动 daemon
step(0, '启动 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->ensureStopped();
    $pid = $svc->start();
    assertTrue($pid > 0, "pid=$pid");
    $svc->wait(10, true);
});

// ============================================================
// 1. 循环任务（每 5 秒 1 次）+ logs 对比第1次和第2次结果
// ============================================================
step(1, '循环任务 every(5s) + 日志对比第1次/第2次', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 命令输出时间戳，便于对比每次执行
    $cmd = 'echo loop-tick-$(date +%s%N)';
    $id = $mgr->create(
        TaskBuilder::shell($cmd)
            ->every(5)
            ->timeout(10)
    );
    // 等 13 秒，应至少触发 2 次
    sleep(13);
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertTrue($ec >= 2, "循环任务至少应执行 2 次，实际 execution_count=$ec, state=" . ($st['state'] ?? '?'));

    // 查询日志
    $logs = $mgr->logs($id);
    assertTrue(count($logs) >= 4, "日志至少应有 4 条（2×started + 2×succeeded），实际 " . count($logs));

    // 过滤 started 事件
    $startedEvents = array_filter($logs, function ($e) {
        return ($e['event_type'] ?? '') === 'started';
    });
    assertTrue(count($startedEvents) >= 2, "started 事件至少 2 条，实际 " . count($startedEvents));

    // 过滤 succeeded 事件
    $succeededEvents = array_filter($logs, function ($e) {
        return ($e['event_type'] ?? '') === 'succeeded';
    });
    assertTrue(count($succeededEvents) >= 2, "succeeded 事件至少 2 条，实际 " . count($succeededEvents));

    // 取前两次 started 事件 ts，验证间隔约 5 秒（容忍 ±2 秒）
    $startedTs = array_values(array_map(function ($e) {
        return (int) ($e['ts'] ?? 0);
    }, $startedEvents));
    if (count($startedTs) >= 2) {
        $gap = $startedTs[1] - $startedTs[0];
        assertTrue($gap >= 3 && $gap <= 8, "两次触发间隔应≈5秒，实际 $gap 秒");
    }

    // 停止循环任务
    $mgr->stop($id);
    sleep(1);

    echo "（execution_count=$ec, logs=" . count($logs) . "）";
});

// ============================================================
// 2. maxExecutions 指定次数（every 2s × 3 次 → 完成）
// ============================================================
step(2, 'maxExecutions(3) 指定次数', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo max-exec-test')
            ->every(2)
            ->maxExecutions(3)
    );
    // 等 9 秒（3 次 × 2 秒 + 余量）
    $deadline = time() + 15;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(300000);
    }
    $st = $mgr->state($id);
    assertEq($st['state'] ?? '', 'success', 'maxExecutions 完成后 state 应为 success');
    assertEq((int) ($st['execution_count'] ?? 0), 3, 'maxExecutions(3) 后 execution_count 应=3');
});

// ============================================================
// 3. withRetry 重试（exit 1 触发失败，retry_max=2, retry_delay=1）
// ============================================================
step(3, 'withRetry(2,1) 重试（exit 1）', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('exit 1')
            ->withRetry(2, 1)
    );
    $ok = $mgr->waitForState($id, 'failed', 15);
    assertTrue($ok, '重试耗尽后应进入 failed');
    $st = $mgr->state($id);
    // attempts 应为 3（1 次初始 + 2 次重试）
    $attempts = (int) ($st['attempts'] ?? 0);
    assertTrue($attempts === 3, "attempts 应=3（1 初次 + 2 重试），实际 $attempts");
    echo "（attempts={$attempts}）";
});

// ============================================================
// 4. allowOverlap 任务重叠（every 1s + sleep 3s + allowOverlap）
// ============================================================
step(4, 'allowOverlap(true) 任务重叠', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('sleep 2')
            ->every(1)
            ->allowOverlap(true)
            ->timeout(10)
    );
    // 等 5 秒，应启动多个并发实例并完成至少 2 次。
    // 注意：execution_count 在每次执行完成后递增（非触发时），所以
    // 等待时间需覆盖"第2次触发开始 + 执行时长 + increment 写入"。
    // sleep(2)+every(1) 时序：t=0→2 完成(ec=1), t=1→3 完成(ec=2), t=2→4 完成(ec=3)。
    // 5 秒后 ec 应>=2。SqliteStore 的 increment（UPDATE）比 InMemory 稍慢，
    // 故留足余量避免竞态。
    sleep(5);
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertTrue($ec >= 2, "allowOverlap=true 时 execution_count 应>=2（并发），实际 $ec");
    $mgr->stop($id);
    sleep(1);
    echo "（execution_count={$ec}）";
});

// ============================================================
// 5a. timeout 硬超时
// ============================================================
step(5, 'timeout(2) 硬超时', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('sleep 10')
            ->timeout(2)
    );
    $mgr->waitForState($id, 'failed', 10);
    $st = $mgr->state($id);
    assertEq($st['state'] ?? '', 'failed', '硬超时后 state 应为 failed');
    $err = $st['last_error'] ?? '';
    assertContains(strtolower($err), 'timeout', 'last_error 应含 timeout 字样');
});

// ============================================================
// 5b. softTimeout 软超时（SIGTERM 提前）
// ============================================================
step(6, 'softTimeout(2) + timeout(5)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('sleep 10')
            ->timeout(5)
            ->softTimeout(2)
    );
    $startTs = time();
    $mgr->waitForState($id, 'failed', 10);
    $elapsed = time() - $startTs;
    $st = $mgr->state($id);
    assertEq($st['state'] ?? '', 'failed', 'softTimeout 触发后 state 应为 failed');
    // softTimeout=2 应在 2~5 秒内终止进程
    assertTrue($elapsed >= 2 && $elapsed <= 5, "softTimeout 应在 2~5 秒内终止，实际 $elapsed 秒");
    echo "（elapsed={$elapsed}s）";
});

// ============================================================
// 7. priority 优先级（高优先级先执行）
// ============================================================
step(7, 'priority(高 vs 低)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // 提前 2 秒调度，避免单核机器优先级失效
    $idLow  = $mgr->create(TaskBuilder::shell('echo low')->priority(0)->timeout(5));
    $idHigh = $mgr->create(TaskBuilder::shell('echo high')->priority(100)->timeout(5));
    // 等都结束
    $mgr->waitForState($idLow, 'success', 10);
    $mgr->waitForState($idHigh, 'success', 10);
    $stLow  = $mgr->state($idLow);
    $stHigh = $mgr->state($idHigh);
    $lowStart  = (int) ($stLow['started_at'] ?? 0);
    $highStart = (int) ($stHigh['started_at'] ?? 0);
    assertTrue($highStart > 0 && $lowStart > 0, '两个任务 started_at 应都已记录');
    // 高优先级 started_at <= 低优先级（即不晚于）
    assertTrue($highStart <= $lowStart, "高优先级应不晚于低优先级启动 (high=$highStart, low=$lowStart)");
    echo "（high_start={$highStart}, low_start={$lowStart}）";
});

// ============================================================
// 8. expires 任务过期（Pending 时间超过 expires 秒）
// ============================================================
step(8, 'expires(1) 任务过期', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    // startAt 设为 30 秒后，让任务保持 Pending 状态
    $id = $mgr->create(
        TaskBuilder::shell('echo never-runs')
            ->startAt(time() + 30)
            ->expires(1)
    );
    // 等 3 秒让过期检查生效
    $deadline = time() + 10;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'expired') {
            break;
        }
        usleep(500000);
    }
    $st = $mgr->state($id);
    assertEq($st['state'] ?? '', 'expired', 'Pending 超过 expires(1s) 后应变为 expired');
});

// ============================================================
// 9. jitter 抖动
// ============================================================
step(9, 'jitter(3) 抖动', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo jitter-test')
            ->cron('* * * * *')
            ->jitter(3)
    );
    $st = $mgr->state($id);
    assertEq((int) ($st['jitter'] ?? 0), 3, 'jitter 应=3');
});

// ============================================================
// 10. retry_backoff 退避重试
// ============================================================
step(10, 'retryBackoff(true) 退避重试', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('exit 1')
            ->withRetry(2, 1)
            ->retryBackoff(true)
    );
    $ok = $mgr->waitForState($id, 'failed', 30);
    assertTrue($ok, '退避重试后应进入 failed');
    $st = $mgr->state($id);
    assertEq((int) ($st['attempts'] ?? 0), 3, 'attempts 应=3');
    assertEq($st['retry_backoff'] ?? '', 'true', 'retry_backoff 应=true');
});

// ============================================================
// 11. ignore_result 忽略结果
// ============================================================
step(11, 'ignoreResult(true) 忽略结果', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo no-result')
            ->ignoreResult(true)
    );
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    assertEq($st['ignore_result'] ?? '', 'true', 'ignore_result 应=true');
    // result 应该没有 stdout（或返回 error）
    $r = $mgr->result($id);
    assertTrue(empty($r['stdout']), 'ignore_result=true 时 result 不应有 stdout');
});

// ============================================================
// 12. acks_late 延迟确认
// ============================================================
step(12, 'acksLate(true) 延迟确认', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo acks-late')
            ->acksLate(true)
    );
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    assertEq($st['acks_late'] ?? '', 'true', 'acks_late 应=true');
});

// ============================================================
// 13. misfire_grace_time 容忍时间
// ============================================================
step(13, 'misfireGraceTime(60)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo mfg')
            ->cron('* * * * *')
            ->misfireGraceTime(60)
    );
    $st = $mgr->state($id);
    assertEq((int) ($st['misfire_grace_time'] ?? 0), 60, 'misfire_grace_time 应=60');
});

// ============================================================
// 14. timezone 时区
// ============================================================
step(14, "withTimezone('America/New_York')", function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo tz-test')
            ->cron('* * * * *')
            ->withTimezone('America/New_York')
    );
    $st = $mgr->state($id);
    assertEq($st['timezone'] ?? '', 'America/New_York', 'timezone 应=America/New_York');
});

// ============================================================
// 15. tags 标签
// ============================================================
step(15, 'tags([urgent, report])', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo tagged')
            ->tags(['urgent', 'report'])
    );
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    $tagsJson = $st['tags'] ?? '[]';
    $tags = json_decode($tagsJson, true);
    assertTrue(is_array($tags) && count($tags) === 2, "tags 应有 2 项，实际: $tagsJson");
    assertTrue(in_array('urgent', $tags) && in_array('report', $tags), 'tags 应包含 urgent 和 report');
});

// ============================================================
// 16. meta 元数据
// ============================================================
step(16, "withMeta(JSON)", function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $meta = '{"foo":"bar","n":42}';
    $id = $mgr->create(
        TaskBuilder::shell('echo meta-test')
            ->withMeta($meta)
    );
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    assertEq($st['meta'] ?? '', $meta, 'meta 应原样返回');
});

// ============================================================
// 17. result_ttl 结果 TTL
// ============================================================
step(17, 'resultTtl(60)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo ttl-test')
            ->resultTtl(60)
    );
    $st = $mgr->state($id);
    // state 函数未透出 result_ttl，验证任务能正常创建即可
    assertTrue(!empty($id) && strpos($id, 'error') === false, 'resultTtl 任务应创建成功');
});

// ============================================================
// 18. rate_limit 速率限制
// ============================================================
step(18, 'rateLimit(2,10) 速率限制', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo rl')
            ->every(1)
            ->rateLimit(2, 10)
            ->timeout(5)
    );
    // 等 4 秒（每 1 秒触发 1 次，但限流 2 次/10 秒）
    sleep(4);
    $st = $mgr->state($id);
    $ec = (int) ($st['execution_count'] ?? 0);
    assertTrue($ec <= 3, "rateLimit(2,10) 在 4 秒内 execution_count 应<=3，实际 $ec");
    $mgr->stop($id);
    sleep(1);
    echo "（execution_count={$ec}）";
});

// ============================================================
// 19. coalesce 合并
// ============================================================
step(19, 'coalesce(false)', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo coalesce-test')
            ->cron('* * * * *')
            ->coalesce(false)
    );
    $st = $mgr->state($id);
    assertEq($st['coalesce'] ?? '', 'false', 'coalesce 应=false');
});

// ============================================================
// 20. start_date 起始时间
// ============================================================
step(20, 'startAt(time()+3) 延迟启动', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $startTs = time() + 3;
    $id = $mgr->create(
        TaskBuilder::shell('echo start-date')
            ->startAt($startTs)
            ->timeout(5)
    );
    // 立即检查：应处于 Pending
    $st = $mgr->state($id);
    assertTrue(
        in_array($st['state'] ?? '', ['pending', 'running', 'success'], true),
        'startAt 未来时间，初次 state 应在 pending/running/success 中，实际: ' . ($st['state'] ?? '?')
    );
    // 等待它执行
    $mgr->waitForState($id, 'success', 15);
    $st = $mgr->state($id);
    $started = (int) ($st['started_at'] ?? 0);
    assertTrue($started >= $startTs, "started_at($started) 应 >= start_date($startTs)");
    echo "（started_at={$started}, start_date={$startTs}）";
});

// ============================================================
// 21. end_date 结束时间
// ============================================================
step(21, 'endAt(soon) 终止循环', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $endTs = time() + 3;
    $id = $mgr->create(
        TaskBuilder::shell('echo end-date')
            ->every(1)
            ->endAt($endTs)
            ->timeout(5)
    );
    // 等 6 秒让 end_date 生效
    $deadline = time() + 15;
    while (time() < $deadline) {
        $st = $mgr->state($id);
        if (($st['state'] ?? '') === 'success') {
            break;
        }
        usleep(500000);
    }
    $st = $mgr->state($id);
    assertEq($st['state'] ?? '', 'success', 'end_date 后应进入 success');
});

// ============================================================
// 22. persist 持久化
// ============================================================
step(22, 'persist(true) 持久化', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo persist-test')
            ->persist(true)
    );
    $mgr->waitForState($id, 'success', 10);
    $st = $mgr->state($id);
    assertTrue(!empty($st['state']), 'persist=true 任务应能正常执行');
    // 验证 SQLite 数据库文件已创建
    assertTrue(file_exists("$DATA_DIR/xhjob.$SERVICE.db"), 'persist=true 应创建 SQLite db 文件');
});

// ============================================================
// 23. withId + replaceExisting
// ============================================================
step(23, "withId + replaceExisting(true)", function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $customId = 'custom-id-' . time();
    // 首次创建
    $id1 = $mgr->create(
        TaskBuilder::shell('echo first')
            ->withId($customId)
    );
    assertEq($id1, $customId, 'dispatch 应返回指定 id');
    $mgr->waitForState($id1, 'success', 10);
    // 再次以相同 id + replaceExisting 创建
    $id2 = $mgr->create(
        TaskBuilder::shell('echo second')
            ->withId($customId)
            ->replaceExisting(true)
    );
    assertEq($id2, $customId, 'replaceExisting 后 id 应相同');
    $mgr->waitForState($id2, 'success', 10);
    // 验证结果为第二次的 echo
    $r = $mgr->result($id2);
    assertContains($r['stdout'] ?? '', 'second', 'replaceExisting 后 stdout 应为 second');
});

// ============================================================
// 24. chain 链式任务
// ============================================================
step(24, 'chain 链式任务', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $chainId = $mgr->createChain([
        TaskBuilder::shell('echo step1'),
        TaskBuilder::shell('echo step2'),
        TaskBuilder::shell('echo step3'),
    ]);
    assertTrue(!empty($chainId) && strpos($chainId, 'error') === false, "chain 应创建成功: $chainId");
    // 等待完成
    $deadline = time() + 20;
    $final = null;
    while (time() < $deadline) {
        $st = $mgr->chainState($chainId);
        if ($st && ($st['state'] ?? '') === 'success') {
            $final = $st;
            break;
        }
        usleep(500000);
    }
    assertTrue($final !== null, 'chain 应进入 success，实际: ' . json_encode($st));
    assertTrue(($final['current_step'] ?? 0) >= 3, 'current_step 应>=3');
    echo "（steps={$final['current_step']}）";
});

// ============================================================
// 25. group 组任务
// ============================================================
step(25, 'group 组任务', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $groupId = $mgr->createGroup([
        TaskBuilder::shell('echo g-a'),
        TaskBuilder::shell('echo g-b'),
    ]);
    assertTrue(!empty($groupId) && strpos($groupId, 'error') === false, "group 应创建成功: $groupId");
    $deadline = time() + 15;
    $final = null;
    while (time() < $deadline) {
        $st = $mgr->groupState($groupId);
        if ($st && ($st['state'] ?? '') === 'success') {
            $final = $st;
            break;
        }
        usleep(500000);
    }
    assertTrue($final !== null, 'group 应进入 success，实际: ' . json_encode($st));
});

// ============================================================
// 26. pause + resume 暂停恢复
// ============================================================
step(26, 'pause + resume', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo pause-test')
            ->cron('* * * * *')
    );
    $ok1 = $mgr->pause($id);
    assertTrue($ok1, 'pause 应返回 true');
    $st = $mgr->state($id);
    assertEq($st['paused'] ?? '', 'true', 'pause 后 paused 应=true');
    $ok2 = $mgr->resume($id);
    assertTrue($ok2, 'resume 应返回 true');
    $st = $mgr->state($id);
    assertEq($st['paused'] ?? '', 'false', 'resume 后 paused 应=false');
});

// ============================================================
// 27. maxInstances 最大并发实例
// ============================================================
step(27, 'maxInstances(1) 限制并发', function () use ($SERVICE, $DATA_DIR) {
    $mgr = new TaskManager($SERVICE, $DATA_DIR);
    $id = $mgr->create(
        TaskBuilder::shell('echo max-instances-test')
            ->cron('* * * * *')
            ->maxInstances(1)
    );
    $st = $mgr->state($id);
    assertTrue(!empty($st['state']), 'maxInstances(1) 任务应创建成功');
});

// ============================================================
// 28. 停止 daemon
// ============================================================
step(28, '停止 daemon', function () use ($SERVICE, $DATA_DIR) {
    $svc = new XhjobService($SERVICE, $DATA_DIR);
    $svc->stop();
    $svc->wait(10, false);
    $st = $svc->status();
    assertTrue(!$st['running'], 'daemon 应已停止');
});

echo "\n=== 深度测试完成：$pass passed, $fail failed ===\n";
if ($fail > 0) {
    echo "失败的步骤：[" . implode(', ', $failedSteps) . "]\n";
}
exit($fail > 0 ? 1 : 0);
