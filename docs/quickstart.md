# 快速开始

## 概述

本篇帮助你在 5 分钟内完成 xhjob 扩展加载、daemon 启动，并投递第一个 shell 任务与第一个 cron 任务。

xhjob 由两部分组成：

- **PHP 扩展（.so）**：提供 27 个顶层函数（snake_case）与 `Xhjob` 链式 builder 类（方法名 camelCase），是 PHP 进程与 daemon 通信的 IPC 客户端。
- **daemon（独立 Rust 进程）**：由 `xhjob_start()` 拉起，负责调度、执行、持久化。PHP 进程（CLI 或 FPM）只连同一个 daemon。

## 函数签名 / 方法签名

快速开始用到的核心 API：

```php
// 顶层函数（snake_case）
xhjob_start(?string $name = null, ?string $data_dir = null): bool
xhjob_status(?string $name = null, ?string $data_dir = null): array
xhjob_dispatch(string $task_json, ?string $name = null, ?string $data_dir = null): string
xhjob_state(string $id, ?string $name = null, ?string $data_dir = null): array
xhjob_result(string $id, ?string $name = null, ?string $data_dir = null): array
xhjob_get(string $id, ?string $name = null, ?string $data_dir = null): ?string
xhjob_stop(?string $name = null, ?string $data_dir = null): bool
```

```php
// Xhjob 类（链式 builder，方法名 camelCase，全部返回 &mut Self，除 task() 与 dispatch()）
Xhjob::task(): Xhjob                 // 静态构造
    ->service(string $name): Xhjob
    ->viaShell(string $cmd): Xhjob
    ->cron(string $expr): Xhjob
    ->persist(bool $val): Xhjob
    ->timeout(int $secs): Xhjob
    ->tag(string $tag): Xhjob
    ->dispatch(): string             // 终结，返回 task_id
```

## 参数说明

| API | 参数 | 说明 |
|-----|------|------|
| `xhjob_start` | `$name` | 服务名，命名空间化 sock/pid/log/db；省略为 `default` |
| `xhjob_start` | `$data_dir` | 统一数据目录；省略则走各 `XHJOB_*_DIR` env 与 fallback |
| `xhjob_dispatch` | `$task_json` | 任务 JSON 字符串（由 builder 序列化） |
| `xhjob_dispatch` | 返回 | 成功返回 `task_id`，失败返回 `error: ...` 前缀字符串 |
| `xhjob_state` | `$id` | 任务 ID |
| `xhjob_get` | 返回 | 结果就绪返回字符串，未就绪返回 `null` |
| `Xhjob::viaShell` | `$cmd` | shell 命令字符串 |
| `Xhjob::cron` | `$expr` | 标准 5 字段 cron 表达式 |
| `Xhjob::persist` | `bool` | `true` 持久化到 store，daemon 重启不丢调度 |

## 返回值

- `xhjob_start` / `xhjob_stop`：`bool`，`true` 表示操作成功。
- `xhjob_status`：键值对数组，包含 `running`（bool），运行时含 `pid`，非法时含 `error`。
- `xhjob_dispatch`：`string`，`task_id` 或 `error: <msg>`。
- `xhjob_state`：键值对数组。
- `xhjob_result`：键值对数组。
- `xhjob_get`：`?string`，结果字符串或 `null`。
- `Xhjob::dispatch()`：`string`，`task_id`（失败时同样以 `error:` 前缀返回）。

## 注意事项

- **daemon 是独立进程**：`xhjob_start()` 会拉起一个 Rust daemon，PHP 进程本身不执行任务，只通过 Unix socket IPC 下发。
- **CLI 与 FPM 共享同一 daemon**：只要服务名与数据目录一致，CLI 脚本与 FPM 请求连的是同一个 daemon，任务互通。
- **扩展加载失败排查**：用 `php -m` 确认；常见原因是 .so 与 PHP API/ABI 版本不匹配，必须按 PHP 8.x 次版本选 .so（如 8.2 选 `xhjob-php8.2-linux-x86_64.so`）。
- **IPC 超时**：默认 `XHJOB_IPC_TIMEOUT_SECS=5`；daemon 未启动或 socket 不可达时，dispatch 会在超时后返回 `error: ...`。

## 代码演示

### 0. 前置条件

- PHP **8.0+**（按次版本选 .so，如 8.2 选 `xhjob-php8.2-linux-x86_64.so`）
- Linux **x86_64**
- `bash`

### 1. 下载并加载扩展

从 `releases/` 目录下载对应 PHP 版本的 .so，临时加载验证：

```bash
# 临时加载（不改 php.ini），确认模块出现
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so -m | grep xhjob

# 验证函数存在
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so \
    -r 'echo function_exists("xhjob_dispatch") ? "ok" : "no";'
# 输出: ok
```

### 2. 第一个 shell 任务

```php
<?php
// 1) 启动 daemon（独立进程）
if (!xhjob_start()) {
    fwrite(STDERR, "failed to start daemon\n");
    exit(1);
}

$status = xhjob_status();
if (empty($status['running'])) {
    fwrite(STDERR, "daemon not running: " . json_encode($status) . "\n");
    exit(1);
}
echo "daemon started, pid=" . ($status['pid'] ?? '-') . "\n";

// 2) 用 Xhjob builder 构建并投递一个 shell 任务
$taskId = Xhjob::task()
    ->service('default')
    ->viaShell('echo hello && date -Is')
    ->timeout(30)
    ->tag('demo')
    ->dispatch();

if (str_starts_with($taskId, 'error:')) {
    fwrite(STDERR, "dispatch failed: {$taskId}\n");
    exit(1);
}
echo "task_id = {$taskId}\n";

// 3) 轮询直到结果就绪（xhjob_get 返回 null 表示尚未完成）
$deadline = time() + 30;
$result = null;
while (time() < $deadline) {
    $result = xhjob_get($taskId);
    if ($result !== null) {
        break;
    }
    // 也可顺便观察状态变化：print_r(xhjob_state($taskId));
    usleep(200_000);
}

if ($result === null) {
    fwrite(STDERR, "timeout waiting for result\n");
    exit(1);
}
echo "result = {$result}\n";

// 4) 取结构化结果
print_r(xhjob_result($taskId));
```

运行：

```bash
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so first_task.php
```

### 3. 第一个 cron 任务

```php
<?php
// 每分钟执行一次健康检查，持久化（daemon 重启后调度不丢）
$cronId = Xhjob::task()
    ->viaShell('curl -fsS https://example.com/health')
    ->cron('*/1 * * * *')
    ->persist(true)
    ->timeout(30)
    ->tag('healthcheck')
    ->dispatch();

if (str_starts_with($cronId, 'error:')) {
    fwrite(STDERR, "cron dispatch failed: {$cronId}\n");
    exit(1);
}
echo "cron task_id = {$cronId}\n";

print_r(xhjob_state($cronId));

// 不再需要时停止 daemon
// xhjob_stop();
```

> ThinkPHP8 扩展包提供独立的 `TaskBuilder` PHP 类（PHP 端独立实现，方法名用 `withId` 等），用法与 `Xhjob` 类一致，可按需替换。注意：`Xhjob` 类的方法是 `id()`（Rust snake_case 自动转 camelCase），`TaskBuilder` 是 `withId()`，两者来源不同，不要混淆。

## 生产建议

- **常驻 daemon**：生产环境用 systemd / supervisor 托管 daemon（或由首个 FPM 请求触发 `xhjob_start()`），避免每次请求拉起。
- **显式服务名**：多租户/多业务用 `XHJOB_SERVICE_NAME` 隔离 sock/pid/log/db，避免相互干扰。
- **数据目录落盘**：设 `XHJOB_DATA_DIR` 或 `XHJOB_DB_DIR` 到持久磁盘，SQLite store 才能在重启后恢复。
- **结果轮询用 `xhjob_get`**：它返回 `null` 表示未就绪，比反复解析 `xhjob_state` 更轻量；不要在 FPM 请求里忙等长任务，改用事件流 `xhjob_events` 异步通知。
- **加载方式**：生产用 `php.ini` 的 `extension=` 永久加载，而非 `php -d`。
