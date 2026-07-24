# 架构概览

## 概述

xhjob 采用 **PHP 进程 ↔ 独立 daemon** 的双进程架构。PHP 扩展只做 IPC 客户端，所有调度、执行、持久化都在 Rust daemon 内完成。这使得 FPM 请求可以非阻塞地投递任务后立即返回，长任务在 daemon 侧异步执行。

核心组件：PHP 进程（CLI 或 FPM）通过 Unix socket 与独立的 daemon 进程 IPC；daemon 内部含 IPC server、scheduler（cron/interval/date trigger + max_pending 队列）、pool（async 或 thread）、executor（shell/http）、store（InMemory 或 SQLite WAL）、watchdog。

## 函数签名 / 方法签名

daemon 生命周期相关 API：

```php
xhjob_start(?string $name = null, ?string $data_dir = null): bool
xhjob_stop(?string $name = null, ?string $data_dir = null): bool
xhjob_restart(?string $name = null, ?string $data_dir = null): bool
xhjob_status(?string $name = null, ?string $data_dir = null): array
xhjob_run_daemon(?string $service_name = null, ?string $data_dir = null): bool  // 隐藏入口
```

复合编排 API：

```php
xhjob_chain(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
xhjob_chain_state(string $chain_id, ?string $name = null, ?string $data_dir = null): ?string
xhjob_group(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
xhjob_group_state(string $group_id, ?string $name = null, ?string $data_dir = null): ?string
xhjob_chord(string $header_json, string $callback_json, ?string $name = null, ?string $data_dir = null): string
xhjob_chord_state(string $chord_id, ?string $name = null, ?string $data_dir = null): ?string
```

运行期观测 API：

```php
xhjob_events(int $since_ts, ?string $task_id = null, ?string $name = null, ?string $data_dir = null): string
xhjob_pull_events(int $since_ts, ?string $event_type = null, ?string $name = null, ?string $data_dir = null): string
xhjob_inspect(string $mode, ?string $name = null, ?string $data_dir = null): string
```

## 参数说明

| API | 参数 | 说明 |
|-----|------|------|
| `xhjob_start` | `$name` | 服务名（命名空间化 sock/pid/log/db） |
| `xhjob_run_daemon` | `$service_name` | 隐藏入口；daemon 通过 re-exec 调用它进入主循环，业务代码不应直接调用 |
| `xhjob_chain` | `$tasks_json` | 顺序任务数组 JSON |
| `xhjob_chord` | `$header_json` | header 并行任务数组 JSON |
| `xhjob_chord` | `$callback_json` | 汇总回调任务 JSON |
| `*_state` | `$chain_id` / `$group_id` / `$chord_id` | 编排 ID |
| `xhjob_events` | `$since_ts` | 起始 Unix 时间戳 |
| `xhjob_events` | `$task_id` | 可选，按任务过滤 |
| `xhjob_pull_events` | `$event_type` | 可选，按事件类型过滤 |
| `xhjob_inspect` | `$mode` | 检视模式 |

## 返回值

- `xhjob_start` / `xhjob_stop` / `xhjob_restart`：`bool`。
- `xhjob_status`：键值对数组，含 `running`（bool）、可选 `pid`、非法时含 `error`。
- `xhjob_run_daemon`：`bool`（正常进入循环不返回）。
- `xhjob_chain` / `xhjob_group` / `xhjob_chord`：`string`，编排 ID 或 `error: ...`。
- `xhjob_chain_state` / `xhjob_group_state` / `xhjob_chord_state`：`?string`，状态 JSON 或 `null`。
- `xhjob_events` / `xhjob_pull_events` / `xhjob_inspect`：`string`（JSON 字符串或 `error:` 前缀）。

## 注意事项

- **PHP 进程是瘦客户端**：不持有任务队列，重启 PHP-FPM 不影响在跑任务。
- **daemon 单实例 per 服务名**：同一服务名重复 `xhjob_start()` 会复用已有实例。
- `xhjob_run_daemon` 是内部入口，业务层不要调用。
- store 选 InMemory 还是 SQLite WAL 取决于是否开启 persist（`XHJOB_PERSIST` 或 builder 的 `persist(true)`）。

## 组件图

```
┌──────────────┐   Unix socket    ┌──────────────────────────────────────┐
│  PHP 进程     │ ═══════════════► │            daemon (Rust)             │
│ (CLI / FPM)  │ ◄═══════════════  │                                      │
│  IPC client  │   IPC reply       │  ┌─────────┐    ┌─────────────────┐  │
└──────────────┘                   │  │ IPC srv │───►│   scheduler     │  │
                                   │  └─────────┘    │ cron/interval/  │  │
                                   │                 │ date trigger    │  │
                                   │                 │ + max_pending   │  │
                                   │                 │   队列          │  │
                                   │                 └────────┬────────┘  │
                                   │                          ▼           │
                                   │                 ┌─────────────────┐  │
                                   │                 │      pool       │  │
                                   │                 │ async / thread /│  │
                                   │                 │ coroutine       │  │
                                   │                 └────────┬────────┘  │
                                   │                          ▼           │
                                   │                 ┌─────────────────┐  │
                                   │                 │    executor     │  │
                                   │                 │  shell / http   │  │
                                   │                 └────────┬────────┘  │
                                   │                          ▼           │
                                   │                 ┌─────────────────┐  │
                                   │                 │      store      │  │
                                   │                 │ InMemory /      │  │
                                   │                 │ SQLite (WAL)    │  │
                                   │                 └─────────────────┘  │
                                   │           watchdog 监控全局          │
                                   └──────────────────────────────────────┘
```

组件职责：

| 组件 | 职责 |
|------|------|
| PHP 扩展 | 27 个顶层函数 + `Xhjob` builder，IPC 客户端 |
| IPC server | 监听 Unix socket，鉴权（SO_PEERCRED / API token） |
| scheduler | cron / interval(`every`) / date(`runAt`) 三种 trigger；受 `max_pending` 队列约束 |
| pool | `async`(默认) / `thread` / `coroutine` 三种执行池（`XHJOB_POOL_MODE`） |
| executor | shell（`viaShell`）/ http（`viaHttp`）两种执行器 |
| store | InMemory 或 SQLite WAL（persist 开启时） |
| watchdog | 周期巡检（`XHJOB_WATCHDOG_INTERVAL` / `XHJOB_WATCHDOG_FACTOR`） |

## 请求流程

以 FPM 投递一个 shell 任务为例：

1. **FPM dispatch（非阻塞）**：PHP 进程调用 `Xhjob::task()->viaShell(...)->dispatch()`，扩展经 Unix socket 把任务 JSON 发给 daemon，**不等执行完成**，立即拿到 `task_id` 返回给 HTTP 客户端。
2. **daemon 入队**：IPC server 收到任务，scheduler 根据 trigger 决定立即入队或按计划排队（受 `XHJOB_MAX_PENDING` 上限约束）。
3. **pool 取出执行**：pool worker 从队列取任务，交给 executor（shell 派生子进程 / http 发请求），受 `timeout` / `softTimeout` 约束。
4. **写 store**：执行结果与状态写入 store（InMemory 或 SQLite WAL），供后续 `xhjob_result` / `xhjob_get` / `xhjob_state` 查询。
5. **PHP 取结果**：CLI 或另一个 FPM 请求用 `task_id` 轮询/拉取结果。

```
FPM 请求 ──dispatch(非阻塞)──► daemon 队列 ──► executor ──► store
                                                              ▲
CLI / FPM ──result/get/state──────────────────────────────────┘
```

## daemon 生命周期

| 操作 | API | 行为 |
|------|-----|------|
| 启动 | `xhjob_start()` | 拉起 Rust daemon，写 PID 文件，进入主循环 |
| 停止 | `xhjob_stop()` | 优雅停止，`XHJOB_SHUTDOWN_DRAIN_SECS` 内排空在跑任务 |
| 重启 | `xhjob_restart()` | stop + start，持久化任务调度不丢 |
| 状态 | `xhjob_status()` | 返回 `running` / `pid` / `error` |
| 内部循环 | `xhjob_run_daemon()` | 隐藏入口，re-exec 后进入 daemon 循环，业务勿调 |

PID 文件采用双行格式（`pid\nstarttime`，starttime 取自 `/proc/{pid}/stat` 字段 22），用于防止 PID 复用误判（详见 [安装与配置](install-config.md)）。

## 与 Celery / APScheduler 概念映射

| 外部概念 | xhjob 对应 | 说明 |
|----------|-----------|------|
| Celery worker | daemon + pool + executor | 独立进程承担执行 |
| Celery task | `Xhjob` builder / `TaskBuilder` | 链式构建后 `dispatch()` |
| Celery `apply_async` | `dispatch()` | 投递 |
| Celery `result.get` | `xhjob_result` / `xhjob_get` | 取结果 |
| Celery chain | `xhjob_chain` | 顺序依赖 |
| Celery group | `xhjob_group` | 并行批次 |
| Celery chord | `xhjob_chord` | header 并行 + callback 汇总 |
| Celery retry | `withRetry(max, delay)` / `retryBackoff` | 重试与退避 |
| Celery rate_limit | `rateLimit(count, window)` | 限流 |
| APScheduler CronTrigger | `cron($expr)` / `orCron($exprs)` | cron 触发 |
| APScheduler IntervalTrigger | `every($secs)` | 间隔触发 |
| APScheduler DateTrigger | `runAt($ts)` | 一次性定时 |
| APScheduler jobstore | store (InMemory / SQLite WAL) | 持久化 |
| APScheduler misfire_grace | `misfireGraceTime($secs)` | 错过补偿窗口 |
| APScheduler coalesce | `coalesce(bool)` | 合并积压 |

## 生产建议

- **daemon 与 FPM 分离部署**：daemon 用 systemd 常驻，FPM 只做 IPC 客户端，互不拖累。
- **pool 模式按负载选**：IO 密集用默认 `async`（`XHJOB_ASYNC_POOL_SIZE=1024`）；CPU 密集用 `thread`（`XHJOB_THREAD_POOL_SIZE=num_cpus`）。
- **持久化生产必开**：`XHJOB_PERSIST=true` + SQLite WAL，重启不丢调度。
- **编排优先用原生 API**：chain/group/chord 在 daemon 侧原子编排，比 PHP 端自己轮询拼装更可靠。
- **监控用 inspect/events**：`xhjob_inspect($mode)` 看运行时统计，`xhjob_events` / `xhjob_pull_events` 订阅事件流做告警。
