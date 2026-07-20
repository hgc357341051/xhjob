<?php
/**
 * Example: HTTP 代理（HTTP / HTTPS / SOCKS5）。
 *
 * 演示如何通过 withProxy() 让 HTTP 任务经由代理转发，包含 Basic Auth 示例。
 * 运行前请先在本机启动一个代理（例如 socks5 监听 127.0.0.1:1080）。
 *
 * 用法：
 *   php -d extension=target/release/libxhjob.so examples/proxy.php
 */

if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// 1. SOCKS5 代理（无认证）
$id1 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/ip')
    ->withProxy('socks5://127.0.0.1:1080')
    ->timeout(30)
    ->dispatch();
echo "SOCKS5 task dispatched: {$id1}\n";

// 2. SOCKS5h 代理（DNS 由代理解析）+ Basic Auth
$id2 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/ip')
    ->withProxy('socks5h://user:pass@127.0.0.1:1080')
    ->timeout(30)
    ->dispatch();
echo "SOCKS5h+auth task dispatched: {$id2}\n";

// 3. HTTP 代理 + Basic Auth
$id3 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/ip')
    ->withProxy('http://user:pass@127.0.0.1:8080')
    ->timeout(30)
    ->dispatch();
echo "HTTP proxy+auth task dispatched: {$id3}\n";

// 轮询结果
foreach ([$id1, $id2, $id3] as $id) {
    for ($i = 0; $i < 100; $i++) {
        $s = xhjob_state($id);
        $state = $s['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            echo "Task {$id}: state={$state}\n";
            $r = xhjob_result($id);
            if (isset($r['status_code'])) echo "  status_code={$r['status_code']}\n";
            if (isset($r['body']))        echo "  body={$r['body']}\n";
            if (isset($r['error']))       echo "  error={$r['error']}\n";
            break;
        }
        usleep(100000);
    }
}

xhjob_stop();
echo "Done.\n";
