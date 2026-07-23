#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | 临时调试脚本（非正式回归测试套件）
// +----------------------------------------------------------------------
// | 用途：手动验证 P0-2c 修复——xhjob_dispatch / TaskBuilder::withId
// |       对含特殊字符（SQL 注入串、'error:' 前缀、shell 元字符）的非法 id
// |       的拒绝行为，并打印 daemon 原始返回值供人工核对。
// | 说明：本脚本为一次性调试用途，输出为 var_export 原始返回值，无 pass/fail
// |       断言汇总，不纳入回归套件；正式覆盖见 test_xhjob_p0_round2.php。
// | 清理：脚本末尾会停止 daemon 并删除临时数据目录 /tmp/xhjob-tp-debug-2c。
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
