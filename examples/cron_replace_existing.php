<?php
/**
 * Example: replace_existing 幂等 dispatch（A14）.
 *
 * 演示 id(string $id) + replaceExisting(true) 实现部署脚本幂等：
 * 重复 dispatch 同一 id 时直接替换老任务，不报冲突错误。
 *
 * Reference: APScheduler replace_existing (A14)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/cron_replace_existing.php
 */

$data_dir = '/tmp/xhjob-ex-repexisting';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== replace_existing 幂等 dispatch demo (A14) ===\n\n";

// 固定 task id，模拟 CI/CD 部署脚本反复执行
$fixed_id = 'deploy-health-check-prod';

// 第一次 dispatch：创建新任务
$id1 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?version=v1')
    ->cron('*/5 * * * *')
    ->id($fixed_id)
    ->replaceExisting(true)
    ->persist(true)
    ->dispatch();
echo "First dispatch returned id: {$id1}\n";
echo "Matches fixed id: " . ($id1 === $fixed_id ? 'YES' : 'NO') . "\n";

// 等待几秒，让 cron 触发几次
sleep(3);
$state1 = xhjob_state($id1);
echo "After 3s: state={$state1['state']} execution_count={$state1['execution_count']}\n";

// 第二次 dispatch：相同 id + replaceExisting(true)，老任务被替换
// 用新 URL 模拟「升级部署」
$id2 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?version=v2')
    ->cron('*/2 * * * *')  // 也改了 cron 表达式
    ->id($fixed_id)
    ->replaceExisting(true)
    ->persist(true)
    ->dispatch();
echo "\nSecond dispatch returned id: {$id2}\n";
echo "Same id returned: " . ($id2 === $fixed_id ? 'YES' : 'NO') . "\n";

// 验证：state/attempts/execution_count 已重置
$state2 = xhjob_state($id2);
echo "After re-dispatch: state={$state2['state']} execution_count={$state2['execution_count']} attempts={$state2['attempts']}\n";

if ($state2['execution_count'] < $state1['execution_count']) {
    echo "\n✓ Task was replaced (execution_count reset)\n";
}

// 对比：不用 replaceExisting 时，相同 id 会报错
$id_err = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?version=v3')
    ->cron('*/5 * * * *')
    ->id($fixed_id)
    ->replaceExisting(false)  // 显式不替换
    ->dispatch();
echo "\nWithout replaceExisting, same id returns: {$id_err}\n";
echo "Started with 'error:': " . (str_starts_with($id_err, 'error:') ? 'YES (expected)' : 'NO') . "\n";

// 清理
xhjob_cancel($fixed_id);
xhjob_stop('default', $data_dir);
echo "Done.\n";
