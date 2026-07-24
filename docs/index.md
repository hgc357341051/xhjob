---
title: 首页
nav_order: 1
permalink: /
---

# Xhjob 使用文档

**Xhjob** 是一个用 Rust + [ext-php-rs](https://github.com/davidpdrsn/ext-php-rs) 实现的 PHP 异步任务调度扩展，提供常驻 daemon、Unix IPC、SQLite 持久化、双线程池、cron/interval/一次性触发器、链/组/chord 编排、崩溃恢复与任务假死检测等生产级能力。

PHP 进程（CLI 或 FPM）通过导出的 `xhjob_*` 函数与独立 daemon 通信，任务在 daemon 进程内调度执行，不阻塞 Web 请求。

## 核心特性

- **27 个 PHP 导出函数** + `Xhjob` 链式 builder 类，覆盖生命周期/派发/查询/控制/编排/事件
- **双线程池模式**：`async`（tokio M:N，IO 密集）与 `thread`（std::thread 1:1，CPU 密集），运行时切换
- **触发器**：cron（5/6 字段）、interval、runAt、countdown，支持 or_cron / skip_dates / workdays_only / jitter / misfire_grace_time
- **可靠性**：retry + 指数退避、hard/soft timeout（SIGTERM→SIGKILL 升级链）、persist（SQLite+WAL）、acksLate 崩溃恢复
- **崩溃恢复**：execution_lease（worker_pid + starttime）防重复执行、watchdog 假死检测、PID 复用防护、SQLite 完整性检查
- **编排**：chain（顺序）、group（并行）、chord（并行+回调）
- **CLI / FPM 共用**：同一 service_name + data_dir 让 Web 请求与命令行连同一 daemon
- **ThinkPHP 8 集成**：ServiceProvider 自动注册、Facade、helper、25 个 HTTP 端点

## 快速开始

```php
<?php
require 'vendor/autoload.php';

use Xhjob\TaskBuilder;
use Xhjob\XhjobService;

// 启动 daemon（幂等）
(new XhjobService())->start();

// 派发一个 shell 任务
$taskId = TaskBuilder::shell('echo hello')
    ->timeout(10)
    ->withRetry(2, 3)
    ->dispatch();

// 查询状态
$state = xhjob_state($taskId);
echo $state['state']; // pending / running / success / failed
```

完整安装步骤见 [快速开始](quickstart/)。

## 文档导航

左侧导航按以下分组组织：

| 分组 | 内容 |
|------|------|
| **入门** | 快速开始、架构概览、安装与配置 |
| **API 参考** | PHP 函数（27 个）、TaskBuilder（40+ 方法）、TaskManager |
| **核心能力** | 触发器、重试与超时、并发控制、持久化与崩溃恢复、编排、进度事件 |
| **进阶** | 双线程池模式对比（async vs thread） |
| **生产实战** | 后台任务队列、定时任务、CLI 与 FPM 共用、ThinkPHP 8 集成 |
| **排障** | 常见错误与解决方案 |

## 资源

- [GitHub 仓库](https://github.com/hgc357341051/xhjob)
- [Releases 下载](https://github.com/hgc357341051/xhjob/releases)（预编译 `.so` 扩展）
