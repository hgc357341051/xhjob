<?php
/**
 * php-fpm 业务场景模拟 - HTTP handler
 *
 * 模拟 php-fpm 环境：每个 HTTP 请求由独立的 PHP worker 进程处理，
 * 请求结束后 worker 进程退出（或复用但上下文重置）。
 *
 * 本脚本作为 PHP 内置 server 的 router，根据 URL 路径路由到不同业务：
 *   GET  /start            启动服务（部署时一次性调用）
 *   GET  /status           查询服务状态
 *   GET  /dispatch/shell   投递 shell 任务
 *   GET  /dispatch/http    投递 HTTP 任务
 *   GET  /state?id=XXX     查询任务状态
 *   GET  /result?id=XXX    查询任务结果
 *   POST /restart           重启服务
 *   DELETE /stop            停止服务
 *
 * 每个 HTTP 请求 = 一个独立 PHP 进程，daemon 必须跨请求存活。
 */

header('Content-Type: application/json; charset=utf-8');

$method = $_SERVER['REQUEST_METHOD'];
$path = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);
$serviceName = $_GET['service'] ?? 'fpm-prod';

// 路由
try {
    switch (true) {
        case $method === 'GET' && $path === '/start':
            handleStart($serviceName);
            break;

        case $method === 'GET' && $path === '/status':
            handleStatus($serviceName);
            break;

        case $method === 'GET' && $path === '/dispatch/shell':
            handleDispatchShell($serviceName);
            break;

        case $method === 'GET' && $path === '/dispatch/http':
            handleDispatchHttp($serviceName);
            break;

        case $method === 'GET' && $path === '/state':
            handleState($serviceName);
            break;

        case $method === 'GET' && $path === '/result':
            handleResult($serviceName);
            break;

        case $method === 'POST' && $path === '/restart':
            handleRestart($serviceName);
            break;

        case $method === 'DELETE' && $path === '/stop':
            handleStop($serviceName);
            break;

        default:
            http_response_code(404);
            echo json_encode(['error' => 'not found', 'path' => $path, 'method' => $method]);
    }
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode(['error' => $e->getMessage(), 'file' => $e->getFile(), 'line' => $e->getLine()]);
}

// ============================================================
// Handlers
// ============================================================

function handleStart(string $serviceName): void {
    // 清理旧实例
    @xhjob_stop($serviceName);
    usleep(300000);

    $ok = xhjob_start($serviceName);
    if (!$ok) {
        http_response_code(500);
        echo json_encode(['error' => 'xhjob_start failed', 'service' => $serviceName]);
        return;
    }

    // 等待就绪
    $pid = null;
    for ($i = 0; $i < 50; $i++) {
        $s = xhjob_status($serviceName);
        if ($s['running'] === 'true' && isset($s['pid'])) {
            $pid = $s['pid'];
            break;
        }
        usleep(100000);
    }

    echo json_encode([
        'ok' => true,
        'service' => $serviceName,
        'pid' => $pid,
        'worker_pid' => posix_getpid(),
        'message' => 'daemon started, independent of worker process',
    ]);
}

function handleStatus(string $serviceName): void {
    $s = xhjob_status($serviceName);
    echo json_encode([
        'service' => $serviceName,
        'running' => $s['running'] === 'true',
        'pid' => $s['pid'] ?? null,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleDispatchShell(string $serviceName): void {
    $marker = $_GET['marker'] ?? ('fpm-shell-' . uniqid());
    $cmd = PHP_OS_FAMILY === 'Windows'
        ? "cmd /C echo {$marker}"
        : "echo {$marker}";

    $id = Xhjob::task()
        ->service($serviceName)
        ->viaShell($cmd)
        ->withRetry(2, 1)
        ->timeout(10)
        ->dispatch();

    if (strpos($id, 'error') === 0 || strlen($id) < 10) {
        http_response_code(500);
        echo json_encode(['error' => 'dispatch failed', 'detail' => $id]);
        return;
    }

    echo json_encode([
        'ok' => true,
        'task_id' => $id,
        'marker' => $marker,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleDispatchHttp(string $serviceName): void {
    $url = $_GET['url'] ?? 'https://httpbin.org/get';
    $id = Xhjob::task()
        ->service($serviceName)
        ->viaHttp('GET', $url)
        ->timeout(15)
        ->dispatch();

    if (strpos($id, 'error') === 0 || strlen($id) < 10) {
        http_response_code(500);
        echo json_encode(['error' => 'dispatch failed', 'detail' => $id]);
        return;
    }

    echo json_encode([
        'ok' => true,
        'task_id' => $id,
        'url' => $url,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleState(string $serviceName): void {
    $id = $_GET['id'] ?? '';
    if (!$id) {
        http_response_code(400);
        echo json_encode(['error' => 'missing id parameter']);
        return;
    }
    $s = xhjob_state($id, $serviceName);
    echo json_encode([
        'task_id' => $id,
        'state' => $s['state'] ?? 'UNKNOWN',
        'attempts' => $s['attempts'] ?? null,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleResult(string $serviceName): void {
    $id = $_GET['id'] ?? '';
    if (!$id) {
        http_response_code(400);
        echo json_encode(['error' => 'missing id parameter']);
        return;
    }
    $r = xhjob_result($id, $serviceName);
    echo json_encode([
        'task_id' => $id,
        'body' => $r['body'] ?? null,
        'status_code' => isset($r['status_code']) ? (int)$r['status_code'] : null,
        'stdout' => $r['stdout'] ?? null,
        'stderr' => $r['stderr'] ?? null,
        'exit_code' => isset($r['exit_code']) ? (int)$r['exit_code'] : null,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleRestart(string $serviceName): void {
    $before = xhjob_status($serviceName);
    $pidBefore = $before['pid'] ?? null;

    $ok = xhjob_restart($serviceName);
    if (!$ok) {
        http_response_code(500);
        echo json_encode(['error' => 'restart failed']);
        return;
    }

    // 等待新进程就绪
    $pidAfter = null;
    for ($i = 0; $i < 80; $i++) {
        $s = xhjob_status($serviceName);
        if ($s['running'] === 'true' && isset($s['pid'])) {
            $pidAfter = $s['pid'];
            if ($pidAfter !== $pidBefore) {
                break;
            }
        }
        usleep(100000);
    }

    echo json_encode([
        'ok' => true,
        'pid_before' => $pidBefore,
        'pid_after' => $pidAfter,
        'pid_changed' => $pidAfter !== $pidBefore,
        'worker_pid' => posix_getpid(),
    ]);
}

function handleStop(string $serviceName): void {
    $ok = xhjob_stop($serviceName);
    // 等待真正停止
    for ($i = 0; $i < 50; $i++) {
        $s = xhjob_status($serviceName);
        if ($s['running'] !== 'true') break;
        usleep(100000);
    }
    $s = xhjob_status($serviceName);
    echo json_encode([
        'ok' => $ok,
        'service' => $serviceName,
        'running' => $s['running'] === 'true',
        'worker_pid' => posix_getpid(),
    ]);
}
