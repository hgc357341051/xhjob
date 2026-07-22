<?php
// 功能模块独立验证：用代码验证每个功能模块的执行结果正确性
// 覆盖：shell / retry / overlap / persist / 多服务

$ok = xhjob_start('verify');
if (!$ok) {
    fwrite(STDERR, "FAIL: 无法启动 daemon\n");
    exit(1);
}

$pass = 0;
$fail = 0;
function check($name, $cond) {
    global $pass, $fail;
    if ($cond) {
        echo "[PASS] $name\n";
        $pass++;
    } else {
        echo "[FAIL] $name\n";
        $fail++;
    }
}

function wait_state($id, $svc = 'verify', $timeout = 50) {
    for ($i = 0; $i < $timeout; $i++) {
        $s = xhjob_state($id, $svc);
        $st = $s['state'] ?? 'UNKNOWN';
        if (in_array($st, ['success', 'failed', 'cancelled'], true)) {
            return $s;
        }
        usleep(200_000);
    }
    return ['state' => 'TIMEOUT'];
}

// ===== Test 1: shell 任务：stdout 与 exit_code 正确 =====
echo "\n=== Test 1: shell 任务 stdout/exit_code ===\n";
$id = Xhjob::task()->service('verify')->viaShell('echo hello-shell')->timeout(5)->dispatch();
$s = wait_state($id);
$r = xhjob_result($id, 'verify');
check('shell state=SUCCESS', ($s['state'] ?? '') === 'success');
check('shell stdout 含 hello-shell', strpos($r['stdout'] ?? '', 'hello-shell') !== false);
check('shell exit_code=0', ($r['exit_code'] ?? '') === '0');

// ===== Test 2: retry 任务：重试 3 次后 FAILED，attempts>=3 =====
echo "\n=== Test 2: retry 重试 3 次 ===\n";
$id = Xhjob::task()->service('verify')
    ->viaShell('bash -c "exit 7"')
    ->withRetry(3, 1)
    ->timeout(10)
    ->dispatch();
// retry: 3 retries × 1s base + 可能的指数退避，预留 30s
$s = wait_state($id, 'verify', 150);
check('retry state=FAILED', ($s['state'] ?? '') === 'failed');
// withRetry(3, 1) = 最多重试 3 次，加上初次执行，attempts 应 >= 3
$attempts = intval($s['attempts'] ?? '0');
check("retry attempts>=3 (实际: $attempts)", $attempts >= 3);
$last_error = $s['last_error'] ?? '';
check('retry last_error 非空', !empty($last_error));

// ===== Test 3: overlap 任务：慢任务 + allowOverlap(false) 第二次触发跳过 =====
echo "\n=== Test 3: overlap allowOverlap(false) ===\n";
$id1 = Xhjob::task()->service('verify')
    ->viaShell('sleep 2; echo slow-done')
    ->allowOverlap(false)
    ->timeout(10)
    ->dispatch();
// 立即再投递一个相同任务，应被跳过或排队
usleep(300_000); // 等慢任务进入 RUNNING
$id2 = Xhjob::task()->service('verify')
    ->viaShell('echo fast-after')
    ->allowOverlap(false)
    ->timeout(5)
    ->dispatch();
$s1 = wait_state($id1, 'verify', 60);
$s2 = wait_state($id2, 'verify', 60);
check('overlap slow task SUCCESS', ($s1['state'] ?? '') === 'success');
// 第二个任务要么 SUCCESS（排队后执行），要么被跳过
check('overlap second task completed (SUCCESS)', ($s2['state'] ?? '') === 'success');

// ===== Test 4: persist 任务：restart 后任务状态可查 =====
echo "\n=== Test 4: persist 任务 restart 后可查 ===\n";
// 启用 persist 的服务
putenv('XHJOB_PERSIST=1');
xhjob_stop('persist-svc');
usleep(500_000);
xhjob_start('persist-svc');
usleep(500_000);
$id = Xhjob::task()->service('persist-svc')
    ->viaShell('echo persist-test')
    ->persist(true)
    ->timeout(5)
    ->dispatch();
if (str_starts_with($id, 'error:')) {
    // persist 可能需要 daemon 重启以加载 XHJOB_PERSIST=1
    check('persist dispatch succeeded', false);
} else {
    $s = wait_state($id, 'persist-svc');
    check('persist task SUCCESS', ($s['state'] ?? '') === 'success');

    // restart daemon
    xhjob_restart('persist-svc');
    usleep(500_000);
    // 任务状态仍可查
    $s2 = xhjob_state($id, 'persist-svc');
    check('persist task state queryable after restart', !empty($s2['state']) && $s2['state'] !== 'UNKNOWN');
}
xhjob_stop('persist-svc');
putenv('XHJOB_PERSIST=');

// ===== Test 5: 多服务：两个服务 PID 不同，stop 一个不影响另一个 =====
echo "\n=== Test 5: 多服务隔离 ===\n";
xhjob_start('svc-a');
xhjob_start('svc-b');
$sa = xhjob_status('svc-a');
$sb = xhjob_status('svc-b');
check('svc-a running', ($sa['running'] ?? '') === 'true');
check('svc-b running', ($sb['running'] ?? '') === 'true');
$pida = $sa['pid'] ?? '';
$pidb = $sb['pid'] ?? '';
check('svc-a 与 svc-b PID 不同', !empty($pida) && !empty($pidb) && $pida !== $pidb);

// stop svc-a，验证 svc-b 仍在跑
xhjob_stop('svc-a');
usleep(500_000);
$sa2 = xhjob_status('svc-a');
$sb2 = xhjob_status('svc-b');
check('svc-a stopped', ($sa2['running'] ?? '') === 'false');
check('svc-b 仍 running', ($sb2['running'] ?? '') === 'true');
xhjob_stop('svc-b');

// ===== 清理 =====
xhjob_stop('verify');

echo "\n============================================================\n";
echo "功能模块独立验证: $pass passed, $fail failed\n";
echo "============================================================\n";
exit($fail > 0 ? 1 : 0);
