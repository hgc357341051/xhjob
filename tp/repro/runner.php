<?php
// +----------------------------------------------------------------------
// | Xhjob 复现脚本批量执行器（Task 9 / SubTask 9.1）
// +----------------------------------------------------------------------
// | 扫描 tp/repro/repro_*.php（排除 _bootstrap.php 和 runner.php 自身），
// | 依次用 `php -d extension=<so> repro_XX.php` 子进程执行每个脚本，
// | 捕获输出，统计 PASS/FAIL/SKIP，输出汇总。
// |
// | 用法：
// |   php tp/repro/runner.php                 # 运行全部
// |   php tp/repro/runner.php --only=01       # 只运行 repro_01
// |   php tp/repro/runner.php --only=01,03    # 只运行 repro_01 + repro_03
// +----------------------------------------------------------------------

$reproDir = __DIR__;
$so = '/workspace/target/release/libxhjob.so';
$php = PHP_BINARY;

// 解析 --only 参数
$only = null;
foreach ($argv as $arg) {
    if (strpos($arg, '--only=') === 0) {
        $only = substr($arg, strlen('--only='));
    } elseif ($arg === '--help' || $arg === '-h') {
        echo "Usage: php tp/repro/runner.php [--only=<num[,num...]>]\n";
        echo "  默认运行全部 repro_*.php\n";
        echo "  --only=01      只运行 repro_01\n";
        echo "  --only=01,03   只运行 repro_01 + repro_03\n";
        exit(0);
    }
}

// 扫描 repro_*.php
$files = glob($reproDir . '/repro_*.php');
sort($files);

// 过滤 --only
if ($only !== null && $only !== '') {
    $nums = array_map('trim', explode(',', $only));
    $nums = array_filter($nums, function ($n) { return $n !== ''; });
    $filtered = [];
    foreach ($files as $f) {
        $base = basename($f, '.php'); // repro_01_task_hang_watchdog
        if (preg_match('/^repro_(\d+)/', $base, $m)) {
            if (in_array($m[1], $nums, true)) {
                $filtered[] = $f;
            }
        }
    }
    $files = $filtered;
}

if (empty($files)) {
    echo "没有找到匹配的 repro 脚本。\n";
    exit(1);
}

echo "=== Xhjob 复现脚本批量执行 ===\n";
echo "PHP: {$php}\n";
echo "扩展: {$so}\n";
echo "脚本数: " . count($files) . "\n";
echo str_repeat('=', 70) . "\n\n";

$totalPass = 0;
$totalFail = 0;
$totalSkip = 0;
$scriptResults = []; // ['name' => 'PASS'/'FAIL'/'CRASH']
$failedScripts = [];

foreach ($files as $i => $script) {
    $name = basename($script);
    echo "--- [{$i}/" . count($files) . "] {$name} ---\n";

    $cmd = escapeshellarg($php)
        . ' -d extension=' . escapeshellarg($so)
        . ' ' . escapeshellarg($script) . ' 2>&1';

    $output = shell_exec($cmd);
    echo $output;

    // 解析单脚本汇总行：=== repro: N PASS / M FAIL / K SKIP ... ===
    $scriptPass = 0;
    $scriptFail = 0;
    $scriptSkip = 0;
    $crashed = false;

    if (preg_match('/=== repro: (\d+) PASS \/ (\d+) FAIL \/ (\d+) SKIP/', $output, $m)) {
        $scriptPass = (int) $m[1];
        $scriptFail = (int) $m[2];
        $scriptSkip = (int) $m[3];
    } else {
        // 没有汇总行 → 脚本崩溃
        $crashed = true;
    }

    $totalPass += $scriptPass;
    $totalFail += $scriptFail;
    $totalSkip += $scriptSkip;

    if ($crashed) {
        $scriptResults[] = sprintf("%-50s CRASH", $name);
        $failedScripts[] = $name;
    } elseif ($scriptFail > 0) {
        $scriptResults[] = sprintf("%-50s FAIL (P=%d F=%d S=%d)", $name, $scriptPass, $scriptFail, $scriptSkip);
        $failedScripts[] = $name;
    } else {
        $scriptResults[] = sprintf("%-50s PASS (P=%d S=%d)", $name, $scriptPass, $scriptSkip);
    }

    echo "\n";
}

// 汇总
echo str_repeat('=', 70) . "\n";
echo "=== 汇总 ===\n";
foreach ($scriptResults as $r) {
    echo "  {$r}\n";
}
echo "\n";
echo sprintf(
    "=== 结果: %d PASS / %d FAIL / %d SKIP ===\n",
    $totalPass,
    $totalFail,
    $totalSkip
);
if (!empty($failedScripts)) {
    echo "失败脚本: [" . implode(', ', $failedScripts) . "]\n";
}
echo str_repeat('=', 70) . "\n";

exit($totalFail > 0 || !empty($failedScripts) ? 1 : 0);
