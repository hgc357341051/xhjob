<?php
/**
 * Example: Shell 输出编码转换。
 *
 * 演示如何通过 withEncoding() 将 Shell 任务的 stdout/stderr 从非 UTF-8
 * 编码（如 GBK / Big5）解码为 UTF-8。典型场景为 Windows 中文系统下
 * `cmd /C echo 中文` 输出 GBK 字节流。
 *
 * 用法（Windows）：
 *   php -d extension=target/release/libxhjob.so examples/encoding.php
 *
 * 用法（Unix）：示例可运行，但默认输出为 UTF-8，编码转换可视作 no-op。
 */

if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// 1. 显式指定 GBK 解码（Windows 中文系统典型场景）
$cmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C echo 中文测试'
    : 'echo encoding-test';

$id1 = Xhjob::task()
    ->viaShell($cmd)
    ->withEncoding('GBK')
    ->timeout(10)
    ->dispatch();
echo "GBK encoding task dispatched: {$id1}\n";

// 2. auto 模式：Windows 自动检测 OEM 代码页，Unix 默认 UTF-8
$id2 = Xhjob::task()
    ->viaShell($cmd)
    ->withEncoding('auto')
    ->timeout(10)
    ->dispatch();
echo "auto encoding task dispatched: {$id2}\n";

// 轮询结果，验证 stdout 为合法 UTF-8
foreach ([$id1, $id2] as $id) {
    for ($i = 0; $i < 50; $i++) {
        $s = xhjob_state($id);
        $state = $s['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            echo "Task {$id}: state={$state}\n";
            $r = xhjob_result($id);
            if (isset($r['stdout'])) {
                echo "  stdout={$r['stdout']}\n";
                echo "  stdout is valid UTF-8: " . (mb_check_encoding($r['stdout'], 'UTF-8') ? 'yes' : 'no') . "\n";
            }
            if (isset($r['exit_code'])) echo "  exit_code={$r['exit_code']}\n";
            break;
        }
        usleep(100000);
    }
}

xhjob_stop();
echo "Done.\n";
