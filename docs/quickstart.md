---
title: 快速开始
parent: 入门
nav_order: 11
---

# 快速开始

本页带你用最短路径跑通 xhjob：下载扩展 → 加载 → 启动 daemon → 派发第一个 shell 任务 → 派发第一个 cron 任务。

## 前置条件

| 项 | 要求 | 说明 |
|----|------|------|
| PHP | 8.0 及以上 | 扩展基于 ext-php-rs，需要 PHP 8.x ABI |
| 系统 | Linux x86_64 | 预编译 `.so` 仅提供 linux-x86_64；其他平台需自行从源码编译 |
| Shell | bash | shell 任务通过 `/bin/sh -c` 执行，bash 是默认环境 |
| 权限 | 对运行目录可读写 | 默认 `/run/xhjob`（root 拥有，0o700）；普通用户请通过 `XHJOB_DATA_DIR` 指向可写目录 |

确认 PHP 版本与架构：

```bash
# 查看 PHP 版本（需 8.0+）
php -v

# 确认是 x86_64 架构
uname -m   # 输出应为 x86_64
```

## 下载预编译扩展

预编译的 `.so` 文件随仓库发布，按 PHP 版本选择对应文件。例如 PHP 8.2 对应：

```
releases/xhjob-php8.2-linux-x86_64.so
```

下载或拷贝到本地任意目录（例如 `/opt/xhjob/xhjob.so`），然后通过 `-d extension=` 临时加载并验证：

```bash
# 用 -d extension= 临时加载扩展，-m 列出已加载模块，grep 过滤 xhjob
php -d extension=/opt/xhjob/xhjob.so -m | grep xhjob
# 预期输出：xhjob
```

如果看到 `xhjob` 字样，说明扩展已成功加载。

> 永久加载请参考 [安装与配置](install-config/) 的「扩展加载三种方式」一节，把 `.so` 写入 `php.ini` 或 `PHP_INI_SCAN_DIR` 扫描目录。

## 启动 daemon

xhjob 的任务调度由**独立的 daemon 进程**完成（Rust 实现，通过 Unix socket 与 PHP 进程通信）。派发任务前必须先启动 daemon。

最简单的方式是用全局函数 `xhjob_start()`，它会在返回 `true` 前等待 **PID 文件写入 AND IPC socket 可连接**双条件就绪：

```php
<?php
// 启动 daemon（幂等：已运行则直接返回 true）
$ok = xhjob_start('default');   // 参数：服务名，默认 'default'
var_dump($ok);                  // bool(true) 表示 daemon 已就绪
```

或者使用 `Xhjob\XhjobService` 封装类（推荐，自带等待与异常处理）：

```php
<?php
require 'vendor/autoload.php';

use Xhjob\XhjobService;

// 构造时传入服务名（默认 'default'）与可选数据目录
$svc = new XhjobService('default');
$pid = $svc->start();   // 返回 daemon 进程的 PID（>0 表示成功）
echo "daemon pid = {$pid}\n";
```

确认状态：

```bash
# 通过 PHP 查询 daemon 状态
php -d extension=/opt/xhjob/xhjob.so -r 'var_dump(xhjob_status("default"));'
```

## 第一个 shell 任务

下面派发一个一次性 shell 任务，并轮询查询其状态。

```php
<?php
require 'vendor/autoload.php';

use Xhjob\TaskBuilder;
use Xhjob\XhjobService;

// 1. 确保 daemon 已运行（未运行则启动）
(new XhjobService('default'))->ensureRunning();

// 2. 派发一个 shell 任务：执行 echo hi，超时 10 秒
$id = TaskBuilder::shell('echo hi')
    ->timeout(10)
    ->dispatch();
echo "task id = {$id}\n";

// 3. 轮询查询状态，直到进入终态
while (true) {
    $state = xhjob_state($id);          // 返回状态信息数组
    $s = $state['state'] ?? 'UNKNOWN';  // pending / running / success / failed
    echo "state = {$s}\n";
    if (in_array($s, ['success', 'failed'], true)) {
        break;
    }
    usleep(500000); // 500ms 轮询一次
}

// 4. 读取最终结果
$result = xhjob_result($id);
var_dump($result);
```

状态取值说明：

| state | 含义 |
|-------|------|
| `pending` | 已入队，等待调度 |
| `running` | 正在执行 |
| `success` | 执行成功（终态） |
| `failed` | 执行失败或超时（终态） |

## 第一个 cron 任务

cron 任务需要 `persist(true)`，使其写入 SQLite 持久化存储，daemon 重启后仍能继续按计划触发。

```php
<?php
require 'vendor/autoload.php';

use Xhjob\TaskBuilder;
use Xhjob\XhjobService;

(new XhjobService('default'))->ensureRunning();

// 每分钟执行一次 echo cron，启用持久化
$id = TaskBuilder::shell('echo cron')
    ->cron('*/1 * * * *')   // 标准 5 字段 cron 表达式
    ->persist(true)         // 写入 SQLite，daemon 重启不丢
    ->dispatch();
echo "cron task id = {$id}\n";

// 查看 cron 任务的累计执行次数
$state = xhjob_state($id);
echo "execution_count = {$state['execution_count']}\n";
```

> `persist(true)` 依赖 `--all-features` 编译的 SQLite 支持。若使用的预编译 `.so` 未开启该 feature，请参考 [安装与配置](install-config/) 中 `XHJOB_PERSIST` 一节。

## 注意事项

### daemon 是独立进程

daemon 由 `xhjob_start` 通过 Unix double-fork + setsid 守护化为独立 Rust 进程，**不随 PHP 请求结束而退出**。派发任务前必须确保其已启动：

- 函数式：`xhjob_start('default')` / `xhjob_stop('default')` / `xhjob_restart('default')` / `xhjob_status('default')`
- 面向对象：`XhjobService::start()` / `stop()` / `restart()` / `status()` / `healthCheck()` / `ensureRunning()` / `ensureStopped()`

`xhjob_start` 返回 `true` 之前会等待 **PID 文件写入 AND socket 可连接**双条件，避免「PID 已写但 socket 未就绪」的竞态导致首次 dispatch 失败。

### CLI 与 FPM 都可调用

同一个 daemon 可被 CLI 脚本和 PHP-FPM Web 请求共用——只要它们使用**相同的服务名 + 数据目录**，就会连到同一个 daemon。例如：

- CLI 脚本：`php -d extension=xhjob.so cron_runner.php`
- FPM 请求：在 Web 控制器里调用 `TaskBuilder::shell(...)->dispatch()`

两者派发的任务都进入同一 daemon 的调度队列。

### 扩展加载失败排查

如果 `php -m | grep xhjob` 没有输出，常见原因与对策：

| 现象 | 原因 | 对策 |
|------|------|------|
| `Warning: dl(): ...` | `dl()` 在新 SAPI（如 PHP-FPM、新 CLI）下被禁用 | 不要用 `dl()`，改用 `extension=xhjob.so` ini 指令或 `php -d extension=xhjob.so` |
| `PHP Startup: Unable to load dynamic library` | ABI 版本不匹配 | `.so` 的 PHP 版本必须与运行时一致（PHP 8.2 的 `.so` 只能用于 PHP 8.2） |
| 路径找不到 | `extension=` 用了相对路径 | 使用绝对路径，或把 `.so` 放进 `extension_dir` |
| `undefined symbol` | 依赖库缺失 | 预编译 `.so` 已静态链接核心依赖；若仍报错请确认 glibc 版本满足要求 |

**优先用以下两种方式加载**（均不依赖 `dl()`）：

1. `php.ini` 中写 `extension=xhjob.so`（或绝对路径）
2. 命令行临时加载：`php -d extension=/path/to/xhjob.so ...`

`dl()` 仅在部分旧 CLI SAPI 下可用，且在新 SAPI 受限，**不要在生产环境依赖它**。

## 下一步

- 了解整体设计：[架构概览](architecture/)
- 完整环境变量与配置：[安装与配置](install-config/)
