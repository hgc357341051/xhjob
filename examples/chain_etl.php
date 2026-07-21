<?php
/**
 * Example: Task chain 顺序流水线（C15）.
 *
 * 演示 xhjob_chain 创建 3 步 ETL 流水线：extract → transform → load。
 * daemon 按序执行，每步成功后才执行下一步，任一步失败则中断。
 *
 * Reference: Celery chain (C15)
 *
 * 用法：
 *   php -d extension=xhjob.so examples/chain_etl.php
 */

$data_dir = '/tmp/xhjob-ex-chain';

if (!xhjob_start('default', $data_dir)) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

echo "=== Task chain 顺序流水线 demo (C15) ===\n\n";

// 3 步 ETL：extract → transform → load
// 使用 shell 命令模拟 ETL 各阶段
$tasks = [
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'echo \'{"items":[{"id":1,"name":"alice"},{"id":2,"name":"bob"}]}\' > /tmp/xhjob-etl-raw.json'],
    ],
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'jq "[.items[] | {id, name_upper: (.name | ascii_upcase)}]" /tmp/xhjob-etl-raw.json > /tmp/xhjob-etl-clean.json'],
    ],
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'cp /tmp/xhjob-etl-clean.json /tmp/xhjob-etl-final.json && echo load-done'],
    ],
];

$chain_id = xhjob_chain(json_encode($tasks));
echo "Chain dispatched: {$chain_id}\n";

if (str_starts_with($chain_id, 'error:')) {
    fwrite(STDERR, "Failed to create chain: {$chain_id}\n");
    xhjob_stop('default', $data_dir);
    exit(1);
}

// 轮询链状态：pending → running → succeeded / failed
echo "\nPolling chain state:\n";
$final_state = null;
for ($i = 0; $i < 60; $i++) {
    $json = xhjob_chain_state($chain_id);
    if ($json === null) {
        echo "  tick {$i}: chain_state returned null\n";
        sleep(1);
        continue;
    }
    $state = json_decode($json, true);
    $s = $state['state'];
    $step = $state['current_step'];
    $total = count($state['tasks']);
    echo "  tick {$i}: state={$s} step={$step}/{$total}\n";
    $final_state = $s;
    if ($s === 'succeeded' || $s === 'failed') break;
    sleep(1);
}

echo "\nFinal chain state: {$final_state}\n";

// 验证最终产物
echo "\n=== Final ETL output (/tmp/xhjob-etl-final.json) ===\n";
if (file_exists('/tmp/xhjob-etl-final.json')) {
    $content = file_get_contents('/tmp/xhjob-etl-final.json');
    echo $content . "\n";
} else {
    echo "(file not found — chain may not have completed)\n";
}

// 清理临时文件
@unlink('/tmp/xhjob-etl-raw.json');
@unlink('/tmp/xhjob-etl-clean.json');
@unlink('/tmp/xhjob-etl-final.json');

// 清理 daemon
xhjob_stop('default', $data_dir);
echo "\nDone.\n";
