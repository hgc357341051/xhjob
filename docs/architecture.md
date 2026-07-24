---
title: 架构概览
parent: 入门
nav_order: 12
---

# 架构概览

xhjob 由 **PHP 扩展**与**独立 Rust daemon**两部分组成，通过 Unix domain socket（Windows 为命名管道）通信。本页说明组件关系、请求流程、daemon 生命周期，并与 Celery / APScheduler 做概念映射。

## 组件图

PHP 进程（CLI 或 FPM）通过扩展导出的 `xhjob_*` 函数发起请求，经 IPC 帧到达独立 daemon 进程；daemon 内部由 scheduler 调度、pool 执行、store 持久化。

```
┌─────────────────────────────┐
│  PHP 进程（CLI / FPM）        │
│                             │
│  TaskBuilder / XhjobService │
│        │                    │
│        ▼                    │
│   xhjob_* 扩展函数           │   （27 个导出函数：dispatch / state / result / chain / group ...）
│        │                    │
└────────┼────────────────────┘
         │  IPC 帧（length-prefixed JSON）
         ▼
┌──────────────────────────────────────────────────────┐
│  Daemon（独立 Rust 进程，double-fork + setsid 守护化） │
│                                                      │
│   ┌──────────────┐    Unix socket / Named Pipe       │
│   │  IPC listener │◄──── PHP 进程连接                  │
│   └──────┬───────┘                                   │
│          │ handle                                     │
│          ▼                                            │
│   ┌──────────────┐   入队    ┌────────────────────┐   │
│   │  Scheduler    │─────────►│  TaskQueue         │   │
│   │ (cron/interval│          │ (优先级队列，上限    │   │
│   │  /runAt 扫描) │          │  XHJOB_MAX_PENDING) │   │
│   └──────────────┘          └─────────┬──────────┘   │
│          ▲                            │ spawn         │
│          │ 周期 tick                   ▼              │
│          │                   ┌────────────────────┐   │
│          │                   │  Pool              │   │
│          │                   │  - async（tokio    │   │
│          │                   │    M:N，IO 密集）   │   │
│          │                   │  - thread（1:1，    │   │
│          │                   │    CPU 密集）       │   │
│          │                   └─────────┬──────────┘   │
│          │                             │ run          │
│          │                             ▼              │
│          │                   ┌────────────────────┐   │
│          │                   │  Executor          │   │
│          │                   │  - shell（sh -c）   │   │
│          │                   │  - http（hyper）    │   │
│          │                   └─────────┬──────────┘   │
│          │                             │ 写状态/结果    │
│          │                             ▼              │
│          │                   ┌────────────────────┐   │
│          └───────────────────│  Store             │   │
│                              │  - SQLite（WAL，    │   │
│                              │    persist 模式）   │   │
│                              │  - InMemory（默认） │   │
│                              └────────────────────┘   │
└──────────────────────────────────────────────────────┘
         ▲
         │ 轮询 xhjob_state / xhjob_result
┌────────┴────────────────────┐
│  PHP 进程读取任务状态/结果     │
└─────────────────────────────┘
```

关键点：

- **PHP 进程不执行任务**，只负责派发与查询，因此 Web 请求不会被任务阻塞。
- **daemon 是常驻进程**，承载 scheduler / pool / executor / store 全部运行时。
- **IPC 帧格式**：`[4 字节大端长度][JSON payload]`，单帧上限 8 MB。
- **IPC 路径**：Unix 为 `<dir>/xhjob.{name}.sock`，Windows 为 `\\.\pipe\xhjob-{name}`。

## 请求流程详解

以 FPM 请求派发一个 shell 任务并随后轮询为例，完整流程如下：

```
FPM dispatch()  ──►  扩展函数 xhjob_dispatch()
                          │
                          ▼  构造 IPC Request（op + payload + trace_id）
                    IPC connect（Unix socket）+ write_frame
                          │
                          ▼
                  Daemon IPC listener accept
                          │
                          ▼  handle：解析 op，校验任务
                    Scheduler 入队（TaskQueue.enqueue）
                          │  超过 XHJOB_MAX_PENDING 拒绝
                          ▼
                    Pool 取出任务 spawn（async task / OS thread）
                          │
                          ▼
                    Executor 执行（shell: /bin/sh -c；http: hyper 请求）
                          │  受 timeout / soft_timeout / retry 约束
                          ▼
                    Store 更新状态（running→success/failed）与结果
                          │
                          ▼  返回 IPC Response（dispatch 直接返回 task id）

FPM 轮询  ──►  xhjob_state($id)  ──►  IPC ──► Store 查询状态
FPM 取结果 ──►  xhjob_result($id) ──►  IPC ──► Store 查询结果
```

阶段说明：

| 阶段 | 组件 | 说明 |
|------|------|------|
| 派发 | `xhjob_dispatch` | 把 TaskBuilder JSON 通过 IPC 帧发给 daemon，立即返回 task id，不等待执行 |
| 入队 | Scheduler / TaskQueue | 按 priority 入队；超过 `XHJOB_MAX_PENDING`（默认 10000）拒绝，防 cron 风暴打爆内存 |
| 调度 | Scheduler | cron / interval / runAt 触发器周期扫描，到点把 pending 任务推给 pool |
| 执行 | Pool + Executor | async 模式 tokio M:N 调度（默认，IO 密集）；thread 模式 1:1 OS 线程（CPU 密集）。Executor 调 shell 或 http |
| 持久化 | Store | persist 模式写 SQLite（WAL）；否则用 InMemory。状态/结果/执行次数均落库 |
| 查询 | `xhjob_state` / `xhjob_result` | FPM 短连接读 daemon，单次 IPC 往返返回当前状态/结果 |

> **IPC 超时保护**：每个 FPM 可达的调用点都套了 `XHJOB_IPC_TIMEOUT_SECS`（默认 5 秒）超时。原因是 `max_execution_time` **无法中断 C 级阻塞调用**——若 daemon 死锁/被 SIGSTOP，没有这个超时会让 FPM worker 永久阻塞，逐个耗尽 worker 池直至 502/504 且无法自愈。

## daemon 生命周期

daemon 的启停通过 PHP 端的两组 API 暴露，底层都走 `src/daemon/mod.rs`。

### 全局函数

| 函数 | 作用 |
|------|------|
| `xhjob_start($name, $dataDir)` | 启动 daemon（幂等）。返回 `true` 前等待 **PID 文件写入 AND socket 可连接**双条件 |
| `xhjob_stop($name, $dataDir)` | 停止 daemon：SIGTERM → 轮询退出 → 必要时 SIGKILL（仅当 PID 文件含 starttime 时） |
| `xhjob_restart($name, $dataDir)` | 先 stop 再 start |
| `xhjob_status($name, $dataDir)` | 查询运行状态与 PID |

### XhjobService 封装类

`Xhjob\XhjobService` 在上述函数之上提供更易用的便捷方法：

| 方法 | 作用 |
|------|------|
| `start()` | 调 `xhjob_start` 后 `wait(10, true)` 等待就绪，返回 daemon PID |
| `stop()` | 调 `xhjob_stop` |
| `restart()` | 调 `xhjob_restart` 后等待就绪，返回新 PID |
| `status()` | 归一化 `xhjob_status` 返回值为 `['running' => bool, 'pid' => int|null]` |
| `healthCheck()` | running 且 pid>0 视为健康，返回 `['healthy', 'pid', 'stats']` |
| `wait($timeoutSec, $expectRunning)` | 轮询 status 直到进入期望状态或超时 |
| `ensureRunning()` | 未运行则自动 start |
| `ensureStopped()` | 运行中则 stop 并等待退出 |

### 启动等待的双条件

`xhjob_start` 返回 `true` 之前会循环（最多 100 次 × 100ms）检查两个条件**同时满足**：

1. PID 文件存在，且其中记录的 PID 仍存活、starttime 匹配（防 PID 复用）；
2. IPC socket 可以成功 `connect`。

仅检查 PID 文件有竞态：daemon 写 PID 在 bind socket 之前，若只等 PID，紧接的 dispatch 会因 socket 未就绪而 `Connection refused`。

### PID 复用防护

daemon 写 PID 文件时同时记录自己的 **starttime**（Linux `/proc/<pid>/stat` 第 22 字段）。后续读取方用 `is_process_alive_with_starttime(pid, Some(starttime))` 校验：PID 存活但 starttime 不匹配，说明该 PID 已被无关进程复用，视为「daemon 已死」。停止时也只在 starttime 已记录的情况下才允许升级到 SIGKILL，避免误杀复用 PID 的无辜进程（fail-safe）。

## 与 Celery / APScheduler 概念映射

如果你熟悉 Python 生态，下表帮你快速建立对应关系：

| Celery / APScheduler 概念 | xhjob 对应 | 说明 |
|--------------------------|-----------|------|
| worker（Celery） | daemon | 常驻进程，承载调度与执行 |
| task（Celery） | Task | 一个可调度单元，由 TaskBuilder 构建 |
| queue（Celery） | TaskQueue | 优先级队列，受 `XHJOB_MAX_PENDING` 限流 |
| beat（Celery） / Scheduler（APScheduler） | scheduler（cron/interval 扫描） | 周期扫描触发器，到点入队 |
| result backend（Celery） | store（SQLite / InMemory） | 存储任务状态与结果 |
| `acks_late`（Celery） | `acksLate` | 任务执行成功后才 ack，崩溃可重投 |
| `soft_time_limit`（Celery） | `soft_timeout` | 软超时：先发 SIGTERM，给任务清理机会 |
| `time_limit`（Celery） | `timeout`（hard） | 硬超时：超时后 SIGKILL |
| `rate_limit`（Celery） | `rate_limit_count` + `rate_limit_window` | 滑动窗口限流 |
| `retry_backoff`（Celery） | `retry_backoff` | 重试指数退避 |
| CronTrigger（APScheduler） | `cron()` | 5/6 字段 cron 表达式 |
| IntervalTrigger（APScheduler） | `every()` | 固定间隔触发 |
| DateTrigger（APScheduler） | `runAt()` | 一次性绝对时间触发 |
| `misfire_grace_time`（APScheduler） | `misfire_grace_time` | 错过触发的宽限时间 |
| `coalesce`（APScheduler） | `coalesce` | 多次积压触发合并为一次 |
| `max_instances`（APScheduler） | `max_instances` | 同任务最大并发实例数 |

## 下一步

- 完整环境变量与路径配置：[安装与配置](install-config/)
- 上手跑通第一个任务：[快速开始](quickstart/)
