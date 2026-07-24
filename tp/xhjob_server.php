#!/usr/bin/env php
<?php
// +----------------------------------------------------------------------
// | Xhjob 跨进程功能测试 - 服务启动器（Task 6）
// +----------------------------------------------------------------------
// | 真实模拟生产环境 PHP-FPM 启动 daemon 后退出的场景：
// |   1. 设置 PHP_INI_SCAN_DIR 让 daemon 子进程通过 ini scan dir 加载扩展
// |   2. 启动 daemon（独立 Rust 进程，Unix double-fork + re-exec PHP）
// |   3. 等待健康检查通过
// |   4. 打印 READY 后退出 —— daemon 必须在脚本退出后仍独立运行
// |
// | 用法：
// |   php -d extension=/workspace/target/release/libxhjob.so tp/xhjob_server.php
// |   php -d extension=/workspace/target/release/libxhjob.so tp/xhjob_server.php --stop
// |   php -d extension=/workspace/target/release/libxhjob.so tp/xhjob_server.php --status
// |   php -d extension=/workspace/target/release/libxhjob.so tp/xhjob_server.php --service=foo --data-dir=/tmp/foo
// +----------------------------------------------------------------------

require __DIR__ . '/vendor/autoload.php';

// 扩展加载兜底：父进程若未通过 -d extension 加载，尝试 dl() 加载
if (!extension_loaded('xhjob')) {
    $candidates = [
        '/workspace/target/release/libxhjob.so',
        '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so',
    ];
    foreach ($candidates as $so) {
        if (file_exists($so) && function_exists('dl')) {
            @dl($so);
            if (extension_loaded('xhjob')) {
                break;
            }
        }
    }
}

use Xhjob\XhjobService;

/**
 * 注入 PHP_INI_SCAN_DIR，让 daemon 子进程能通过 ini scan dir 加载 xhjob 扩展。
 *
 * 背景：daemon 子进程通过 re-exec 当前 PHP binary + `-d extension=xhjob.so` + `-r <code>`
 * 启动。`-d extension=xhjob.so` 仅在 extension_dir 中查找 xhjob.so，若不存在则失败。
 * 通过 PHP_INI_SCAN_DIR 注入一个临时 ini 文件，内容为 `extension=<so 完整路径>`，
 * PHP 启动时会扫描该目录并加载完整路径的扩展，从而保证 daemon 子进程能正确加载扩展。
 *
 * 该函数在 server 与 client 脚本顶部调用：
 *   - server：daemon 子进程需加载扩展
 *   - client：client 自身需加载扩展做 IPC（虽然父进程已 -d 加载，但保险起见）
 */
function setupExtensionScanDir(): void
{
    $so = '/workspace/target/release/libxhjob.so';
    if (!file_exists($so)) {
        // 兜底：尝试 phpenv 路径
        $alt = '/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so';
        if (file_exists($alt)) {
            $so = $alt;
        }
    }

    // 清理过期的 ini scan dir，防止 /tmp 无限累积。
    //
    // 背景：每次运行本脚本都会在 sys_get_temp_dir() 下创建
    //   xhjob_ini_scan_<pid>/xhjob.ini 并通过 putenv 注入 PHP_INI_SCAN_DIR，
    //   供 daemon 子进程（re-exec PHP binary 加载 xhjob 扩展）启动时使用。
    //   原实现既不在退出时删除该目录，也不清理历史残留，导致每次运行
    //   （含 cron / 重启）都在 /tmp 留下一个孤儿目录，长期运行下无限增长。
    //
    // 为什么采用「启动时清理过期目录」而非「退出时删除当前目录」：
    //   - 本脚本启动 daemon 后即 exit(0)，daemon 仍独立运行，并继承了
    //     PHP_INI_SCAN_DIR 环境变量。daemon 自身启动时已读取该 ini scan
    //     dir 加载扩展，但若它在退出后被外部重启、或派生需要读取该目录
    //     的子进程，退出时删除当前目录存在破坏正在运行的 daemon 的风险。
    //   - 因此仅在启动时扫描并删除「过期」（mtime 距今 > 3600s）的
    //     xhjob_ini_scan_* 目录。过期目录必然属于早已退出的历史运行
    //     （其 daemon 早已停止），删除安全；当前运行即将创建的目录 mtime
    //     为最新，不会被误删。这样既避免无限累积，又不影响运行中的 daemon。
    $tmpBase = sys_get_temp_dir();
    $staleThreshold = 3600; // 1 小时
    $now = time();
    foreach ((glob($tmpBase . '/xhjob_ini_scan_*', GLOB_ONLYDIR) ?: []) as $oldDir) {
        if (!is_dir($oldDir)) {
            continue;
        }
        if (($now - (int) @filemtime($oldDir)) <= $staleThreshold) {
            continue; // 未过期，跳过（含当前运行即将创建的目录）
        }
        @unlink($oldDir . '/xhjob.ini');
        @rmdir($oldDir);
    }

    $tmpDir = $tmpBase . '/xhjob_ini_scan_' . posix_getpid();
    if (!is_dir($tmpDir)) {
        @mkdir($tmpDir, 0700, true);
    }
    $iniPath = $tmpDir . '/xhjob.ini';
    file_put_contents($iniPath, "extension={$so}\n");
    // putenv 让 Rust Command 继承该 env var，daemon 子进程通过 ini scan dir 加载扩展
    putenv('PHP_INI_SCAN_DIR=' . $tmpDir);
}

// 解析 CLI 参数
$service = 'xhjob-cross';
$dataDir = '/tmp/xhjob-cross';
$doStop = false;
$doStatus = false;

foreach ($argv as $arg) {
    if (strpos($arg, '--service=') === 0) {
        $v = substr($arg, strlen('--service='));
        if ($v !== '') {
            $service = $v;
        }
    } elseif (strpos($arg, '--data-dir=') === 0) {
        $v = substr($arg, strlen('--data-dir='));
        if ($v !== '') {
            $dataDir = $v;
        }
    } elseif ($arg === '--stop') {
        $doStop = true;
    } elseif ($arg === '--status') {
        $doStatus = true;
    } elseif ($arg === '--help' || $arg === '-h') {
        echo "Usage: php -d extension=<so> tp/xhjob_server.php [--service=<name>] [--data-dir=<dir>] [--stop|--status]\n";
        echo "  default: clean + start daemon + health check + print READY + exit\n";
        echo "  --stop    : stop daemon and exit\n";
        echo "  --status  : print daemon status JSON and exit\n";
        exit(0);
    }
}

// 注入 ini scan dir，让 daemon 子进程能加载扩展
setupExtensionScanDir();

try {
    $svc = new XhjobService($service, $dataDir);

    if ($doStop) {
        $svc->ensureStopped();
        echo "STOPPED\n";
        exit(0);
    }

    if ($doStatus) {
        $st = $svc->status();
        echo json_encode($st), "\n";
        exit(0);
    }

    // 默认行为：清理 + 启动 + 等待 + 健康检查 + 打印 READY 后退出
    // 1. 清理上一次残留
    @system('rm -rf ' . escapeshellarg($dataDir));
    if (!is_dir($dataDir)) {
        @mkdir($dataDir, 0777, true);
    }

    // 2. 确保无残留 daemon
    $svc->ensureStopped();

    // 3. 启动 daemon
    $pid = $svc->start();
    if ($pid <= 0) {
        fwrite(STDERR, "ERROR: daemon 启动失败 pid={$pid}\n");
        exit(1);
    }

    // 4. 等待就绪（start 内部已 wait，这里再补一次确保）
    if (!$svc->wait(10, true)) {
        // 等待失败也尝试停止残留进程，避免 partially-started daemon 残留
        $svc->ensureStopped();
        fwrite(STDERR, "ERROR: daemon 在 10s 内未进入 running 状态\n");
        exit(1);
    }

    // 5. 健康检查
    $h = $svc->healthCheck();
    if (!$h['healthy']) {
        fwrite(STDERR, "ERROR: daemon 不健康: " . json_encode($h) . "\n");
        // 启动失败也尝试停止残留进程
        $svc->ensureStopped();
        exit(1);
    }

    // 6. 打印 READY 后退出（daemon 必须仍独立运行）
    echo "READY pid={$pid} service={$service} data_dir={$dataDir}\n";
    exit(0);
} catch (\Throwable $e) {
    fwrite(STDERR, "ERROR: " . $e->getMessage() . "\n");
    fwrite(STDERR, $e->getTraceAsString() . "\n");
    exit(1);
}
