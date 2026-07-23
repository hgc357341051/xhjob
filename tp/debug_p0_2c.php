#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | 【临时调试脚本】debug_p0_2c.php
// +----------------------------------------------------------------------
// | 说明：这是 P0-2c（withId 字符集校验）修复过程中用于人工排查的
// |       一次性临时调试脚本，仅打印 daemon 对恶意 id 的原始返回，
// |       不含 step 框架与 pass/fail 断言，不参与自动化测试回归。
// |       正式回归用例请使用 test_xhjob_p0_round2.php 的 step 3/4。
// |       可保留以便复现问题，但不应在 CI / 自动化流程中执行。
// +----------------------------------------------------------------------
require __DIR__ . '/vendor/autoload.php';

if (!extension_loaded('xhjob')) {
    $so = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
    if (file_exists($so) && function_exists('dl')) {
        @dl($so);
    }
}

use Xhjob\TaskBuilder;

$SERVICE  = 'tp-debug-2c';
$DATA_DIR = '/tmp/xhjob-tp-debug-2c';
@system("rm -rf $DATA_DIR");
@mkdir($DATA_DIR, 0777, true);

// 启动 daemon
$svc = new Xhjob\XhjobService($SERVICE, $DATA_DIR);
$svc->ensureStopped();
$svc->start();
$svc->wait(10, true);

// 测试 1: 直接构造 JSON 并 dispatch
$badId = "'; DROP TABLE tasks; --";
$json = json_encode([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo debug'],
    'id' => $badId,
]);
echo "发送的 JSON: $json\n";
$result = xhjob_dispatch($json, $SERVICE, $DATA_DIR);
echo "daemon 返回: " . var_export($result, true) . "\n";

// 测试 2: 通过 TaskBuilder
$b = TaskBuilder::shell('echo debug2')->withId($badId);
$json2 = $b->toJson();
echo "\nTaskBuilder JSON: $json2\n";
$result2 = xhjob_dispatch($json2, $SERVICE, $DATA_DIR);
echo "daemon 返回: " . var_export($result2, true) . "\n";

// 测试 3: error: 前缀的 id
$badId3 = 'error: malicious';
$json3 = json_encode([
    'task_type' => 'shell',
    'payload' => ['cmd' => 'echo debug3'],
    'id' => $badId3,
]);
echo "\n发送的 JSON: $json3\n";
$result3 = xhjob_dispatch($json3, $SERVICE, $DATA_DIR);
echo "daemon 返回: " . var_export($result3, true) . "\n";

// 清理
$svc->ensureStopped();
@system("rm -rf $DATA_DIR");
