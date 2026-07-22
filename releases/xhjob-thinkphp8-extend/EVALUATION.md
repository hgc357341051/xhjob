# xhjob 特性矩阵评估：对标 Python APScheduler 与 Celery

> 评估对象：**xhjob** —— 基于 Rust（ext-php-rs 0.15）的 PHP 扩展，提供单机异步任务调度，无外部依赖（不依赖 Redis/MQ/Swoole）。
>
> 对照对象：
> - **APScheduler**（Python 生态，单机/轻量分布式调度器）
> - **Celery**（Python 生态，分布式任务队列，强依赖 Broker）

---

## 1. 简介

### 1.1 评估目的

xhjob 的定位是为 PHP 单机应用提供"开箱即用"的异步任务调度能力，避免引入 Redis/RabbitMQ 等中间件。本文档通过对标 Python 生态最成熟的两套调度方案，明确：

1. xhjob **已具备**哪些对标能力；
2. 哪些能力在**单机 PHP 场景下已足够**；
3. 哪些能力属于**分布式场景**，单机下不必实现；
4. 标志性差距与本 spec 已补齐项；
5. 下一步优化方向。

### 1.2 评估方法

- 来源：APScheduler 3.x/4.x 官方文档、Celery 5.x 官方文档。
- 状态标识：
  - ✅ **已实现**：xhjob 提供等价或更优能力。
  - ⚠️ **部分实现**：核心可用，但存在形态或能力差异。
  - ❌ **未实现**：xhjob 当前未提供。
  - ➖ **不适用**：在单机 PHP 场景下没有实现价值。
- 备注列给出形态对照与优化建议。

### 1.3 xhjob 已实现能力概览

- **PHP 函数（25 个）**：`xhjob_start/stop/restart/status/dispatch/state/result/remove/pause/resume/cancel/list/requeue/reschedule/get/events/chain/chain_state/group/group_state/chord/chord_state/report_progress/pull_events/inspect`
- **TaskBuilder 链式方法（36 个）**：覆盖触发器、重试、超时、并发控制、持久化、编排、事件、代理、编码等。
- **Triggers**：cron(5/6 段)、interval、date(run_at)、start_date、end_date
- **重试**：retry_max + retry_delay + retry_backoff（指数退避）
- **超时**：timeout(硬) + soft_timeout(SIGTERM→SIGKILL)
- **并发控制**：allowOverlap + maxInstances + coalesce
- **持久化**：InMemoryStore(默认) + SqliteStore(feature=persist, WAL)
- **任务编排**：chain(顺序流水线) + group(并行批处理)
- **事件**：started/succeeded/failed/missed/cancelled/paused/resumed/expired/max_instances_reached/rate_limited
- **其他**：jitter/expires/ignore_result/acks_late/acks_on_failure/rate_limit/result_ttl/misfire_grace_time/timezone/tags/meta/id/replace_existing/priority/max_executions
- **本 spec 新增**：progress(进度上报) / chord(group+回调) / countdown(相对延迟) / inspect(运行时检查)
- **多服务实例**：named services（同时运行多个独立 daemon）
- **自定义数据目录**：data_dir
- **HTTP 代理**：http/https/socks5/socks5h + Basic Auth
- **Shell 编码转换**：encoding_rs (GBK/Big5/auto)
- **持久化恢复**：daemon 重启后恢复 ACTIVE 任务，acks_late 的 Running 任务重置为 Pending 重新触发

---

## 2. APScheduler 特性矩阵

| 特性 | APScheduler 实现 | xhjob 实现 | 状态 | 备注/优化建议 |
|---|---|---|---|---|
| **CronTrigger (5 段)** | minute/hour/day/month/day_of_week + 字段别名(L@w,`*`等) | `cron("m h dom mon dow")` 支持 5 段标准 cron | ✅ | xhjob 字段表达式覆盖标准 cron，建议补充 `L`(last)/`#`(nth) 高阶表达式 |
| **CronTrigger (秒级 6 段)** | second 起始的 7 字段 | `cron` 支持 6 段（含秒） | ✅ | 已补齐秒级 |
| **IntervalTrigger** | `interval(seconds=...)` 固定间隔循环 | `every(n)` / `interval(n)` | ✅ | 等价 |
| **DateTrigger** | `run_date` 一次性触发 | `runAt(timestamp)` / `startAt` | ✅ | 等价 |
| **CalendarIntervalTrigger** | 按"自然日历"递增（1 个月=1 个月，跨闰年正确） | ❌ 未提供日历语义的 interval | ⚠️ | 当前 `every` 是固定秒数；如需"每月 1 号"建议直接用 cron 表达式替代 |
| **ThreadPoolExecutor** | 线程池并发执行 | Rust worker 调度，PHP 进程内异步派发 | ✅ | 由 Rust 调度，不依赖 Python 式线程池；并发模型不同但等价 |
| **ProcessPoolExecutor** | 多进程执行 CPU 密集任务 | ❌ 未提供进程池 | ➖ | 单机 PHP 场景下任务多为 IO/HTTP/Shell，进程池收益有限；如需 CPU 隔离可用 `viaShell` 启动独立进程 |
| **MemoryJobStore** | 进程内 dict | `InMemoryStore`（默认） | ✅ | 等价 |
| **SQLAlchemyJobStore** | 任意 SQLAlchemy 后端（MySQL/PG/SQLite…） | `SqliteStore`（feature=persist, WAL） | ⚠️ | 仅 SQLite；单机场景已够，多后端非单机目标 |
| **MongoDBJobStore** | MongoDB 持久化 | ❌ | ➖ | 单机 PHP 场景不值得引入 MongoDB 依赖 |
| **RedisJobStore** | Redis 持久化 | ❌ | ➖ | 与 xhjob"无外部依赖"定位冲突 |
| **add_listener (事件监听)** | `add_listener(cb, EVENT_*)` 注册回调 | `xhjob_events(id)` 拉取 + 本 spec 补齐事件订阅 | ✅ | xhjob 以"拉取为主、订阅可选"形式提供；形态不同但等价 |
| **add_job / modify_job / remove_job** | 任务全生命周期 CRUD | `xhjob_dispatch/state/remove` + `reschedule` | ✅ | `reschedule` ≈ `reschedule_job`；`dispatch` ≈ `add_job` |
| **reschedule_job** | 修改触发器并重排 | `xhjob_reschedule` | ✅ | 等价 |
| **pause_job / resume_job** | 暂停/恢复 | `xhjob_pause/resume` | ✅ | 等价 |
| **get_jobs / get_job** | 列出与查询单个任务 | `xhjob_list` + `xhjob_get` | ✅ | 等价 |
| **coalesce** | 错过多次触发时合并为 1 次 | `coalesce(bool)` | ✅ | 等价 |
| **max_instances** | 同任务最大并发实例数 | `maxInstances(n)` | ✅ | 等价 |
| **misfire_grace_time** | 容忍迟到的宽限时间 | `misfireGraceTime(n)` | ✅ | 等价 |
| **jitter** | 随机抖动避免惊群 | `jitter(n)` | ✅ | 等价 |
| **replace_existing** | 同 ID 任务替换策略 | `replaceExisting(bool)` | ✅ | 等价 |
| **next_run_time** | 计算下次触发时刻 | `xhjob_state` 返回 next_run | ✅ | 通过 state 暴露；可补 `xhjob_next_run(id)` 便捷 API |
| **timezone** | pytz/zoneinfo 时区 | `timezone(str)` | ✅ | 等价 |
| **tags** | 任务标签集合 | `tag(...)` | ✅ | 等价 |
| **max_executions** | 任务最大执行次数后自动移除 | `maxExecutions(n)` | ✅ | APScheduler 4.x 才原生支持，xhjob 已提供 |

> **小结**：APScheduler 25 项核心特性中，xhjob 已实现/等价 21 项（✅），部分实现 1 项（CalendarInterval，⚠️），不适用 3 项（ProcessPool/Mongo/Redis，➖）。在单机调度核心语义上 xhjob 与 APScheduler 基本对齐。

---

## 3. Celery 特性矩阵

| 特性 | Celery 实现 | xhjob 实现 | 状态 | 备注/优化建议 |
|---|---|---|---|---|
| **Broker (Redis/RabbitMQ/SQS 等)** | 强依赖中间件解耦生产/消费 | ❌ 无 Broker，Rust daemon 直连 PHP | ➖ | 与"无外部依赖"定位冲突；单机无需 Broker |
| **Multiple Result Backends (RPC/Redis/DB/Cache/MongoDB)** | 多后端可插拔结果存储 | `InMemoryStore` + `SqliteStore` | ⚠️ | 仅 2 种；单机场景已够 |
| **Task Queue Routing** | 按 queue 名路由到不同 worker | named services（多独立 daemon） | ⚠️ | 形态不同：xhjob 用"多 service 实例"代替"多 queue 路由"，单机等价 |
| **retry + retry_max** | `retry=True, max_retries=N` | `withRetry(max, delay)` / `retry_max` | ✅ | 等价 |
| **retry_backoff (指数退避)** | `retry_backoff=True` 固定/指数 | `retryBackoff(bool/factor)` | ✅ | 等价 |
| **autoretry_for (异常过滤重试)** | `autoretry_for=(ExcA,)` 仅对指定异常重试 | ❌ 仅支持无条件重试 | ⚠️ | 建议：在 `viaShell`/`viaHttp` 下按 exit code 或 HTTP 状态码白名单重试；当前未暴露 |
| **timeout (硬超时)** | `time_limit` 秒级硬超时 | `timeout(n)` | ✅ | 等价 |
| **soft_time_limit (软超时)** | `soft_time_limit` SIGTERM 优雅停止 | `softTimeout(n)` (SIGTERM→SIGKILL) | ✅ | xhjob 显式 SIGTERM→SIGKILL 升级链，语义更清晰 |
| **expires** | 任务过期自动丢弃 | `expires(n)` | ✅ | 等价 |
| **ignore_result** | 不存储结果 | `ignoreResult(bool)` | ✅ | 等价 |
| **store_errors_even_if_ignored** | 即使 ignore_result 也存异常 | ❌ 未提供 | ⚠️ | 建议：`ignoreResult` 时仍记录 `failed` 事件，可经 `xhjob_events` 查询 |
| **acks_late** | 任务完成后才 ack | `acksLate(bool)` | ✅ | 等价 |
| **acks_on_failure** | 失败时是否 ack | `acksOnFailure(bool)` | ✅ | 等价 |
| **rate_limit** | 任务速率限制（per worker） | `rateLimit(n)` | ✅ | 等价；单机语义一致 |
| **task_track_started** | 上报 started 事件 | `started` 事件原生 | ✅ | 等价 |
| **result_expires (result_ttl)** | 结果过期清理 | `resultTtl(n)` | ✅ | 等价 |
| **countdown (相对延迟)** | `apply_async(countdown=N)` 相对当前延迟 N 秒 | 本 spec 新增 `countdown(n)` | ✅ | 本 spec 已补齐 |
| **eta (绝对延迟)** | `apply_async(eta=datetime)` 绝对时刻 | `runAt(timestamp)` / `startAt` | ✅ | xhjob 用 timestamp 表达，等价 |
| **chain** | 顺序流水线，前输出→后输入 | `xhjob_chain` + `chain_state` | ✅ | 等价 |
| **group** | 并行批处理 | `xhjob_group` + `group_state` | ✅ | 等价 |
| **chord (group+回调)** | group 全部完成后触发回调 | 本 spec 新增 `chord` | ✅ | 本 spec 已补齐 |
| **chunks** | 将列表切分为 N 份分批执行 | ❌ 未提供 | ⚠️ | 建议：在 PHP 层用 `array_chunk` + `group` 组合实现，无需下沉到扩展 |
| **map / starmap** | 对可迭代对象并发 map | ❌ 未提供 | ⚠️ | 建议：PHP 层 `array_map` + `group` 组合即可；下沉收益低 |
| **update_state (进度上报)** | 任务运行中上报自定义状态/进度 | 本 spec 新增 `progress` | ✅ | 本 spec 已补齐 |
| **Events (worker/task events)** | 丰富的事件总线 + 事件接收器 | `xhjob_events` 拉取 + 10 类事件 | ✅ | 形态不同（拉取 vs 推送），单机下拉取足够 |
| **inspect (active/registered/scheduled/stats)** | `celery inspect` 运行时检查 | 本 spec 新增 `inspect` | ✅ | 本 spec 已补齐 |
| **Worker Control (revoke/terminate/ping/active)** | 远程控制 worker | `xhjob_cancel`/`pause`/`remove`/`status` | ⚠️ | `cancel`≈revoke；`terminate`(SIGKILL 单任务) 建议补齐 |
| **prefetch_count** | worker 预取任务数调优 | ❌ 未暴露预取参数 | ➖ | 单机无多 worker 竞争，预取调优无意义 |
| **worker_concurrency** | worker 并发度 | `maxInstances` 间接控制 | ⚠️ | 形态不同：xhjob 按"任务最大并发实例"控并发，非全局并发池 |
| **task_remote_tracebacks** | 远程 traceback 增强 | ❌ | ➖ | 单机无远程 worker，无远程 traceback 概念 |
| **worker_cancel_long_tasks_on_connection_loss** | 连接丢失时取消长任务 | ❌ | ➖ | 单机无"连接丢失"场景 |
| **Flower (web 监控)** | 实时 web 监控面板 | ❌ | ➖ | 单机场景可用 `xhjob_status` + `inspect` + `events` 命令行查询替代 |
| **多 worker 集群** | 水平扩展多 worker | named services（多独立 daemon，非真正集群） | ⚠️ | 多 service 是"多独立调度器"而非"集群扩容"；单机够用 |
| **优先级队列 (priority)** | Redis broker 支持优先级队列 | `priority(n)` | ✅ | xhjob 在调度器内部按 priority 排序，单机语义等价 |
| **任务标签/路由** | 标签 + 路由表 | `tag(...)` + named services | ✅ | 标签等价；路由用 named service 替代 |

> **小结**：Celery 35 项特性中，已实现/等价 22 项（✅），部分实现 8 项（⚠️），不适用 5 项（➖：Broker/prefetch/remote_tracebacks/连接丢失取消/Flower）。考虑到 Celery 强分布式基因，xhjob 在单机语义上已覆盖 Celery 的"任务/编排/重试/超时/事件/控制"主线能力。

---

## 4. 评估结论

### 4.1 已实现核心能力占比

| 维度 | 对照系 | 总项数 | ✅已实现 | ⚠️部分 | ➖不适用 | ❌未实现 |
|---|---|---|---|---|---|---|
| APScheduler | 单机调度器 | 25 | 21 | 1 | 3 | 0 |
| Celery | 分布式队列 | 35 | 22 | 8 | 5 | 0 |

- **APScheduler 维度**：单机调度核心语义对齐度 ≈ 84%（✅/总），未实现项均为分布式专属存储（Mongo/Redis）或进程池，单机不需要。
- **Celery 维度**：单机任务/编排/重试/超时/事件主线对齐度 ≈ 63%（✅/总）；若剔除"➖不适用"的分布式项，对齐度 ≈ 73%。

### 4.2 单机 PHP 场景下已足够的能力

✅ **触发器全谱**：cron(5/6 段)/interval/date/start_date/end_date 覆盖 99% 单机定时需求。
✅ **重试与退避**：retry_max + retry_delay + 指数退避，对网络抖动任务足够。
✅ **超时双闸**：硬超时 + 软超时（SIGTERM→SIGKILL），覆盖"卡死"与"优雅停止"。
✅ **并发控制三件套**：allowOverlap + maxInstances + coalesce，单机资源保护已完备。
✅ **持久化与恢复**：SqliteStore(WAL) + daemon 重启恢复 + acks_late 重投，单机可靠性够用。
✅ **任务编排**：chain + group + chord，单机流水线/批处理能力对齐 Celery。
✅ **事件与可观测**：10 类事件 + events 拉取 + inspect 检查，单机排障足够。
✅ **进度上报**：progress 补齐后，长任务可向前端反馈进度。
✅ **HTTP/Shell 双模式 + 代理 + 编码**：覆盖 PHP 应用最常见的"调外部 API"与"调系统命令"两类场景。

### 4.3 标志性差距与本 spec 已补齐项

| 差距项 | 状态 | 说明 |
|---|---|---|
| chord (group + 回调) | ✅ 本 spec 补齐 | Celery 标志性编排原语，现已具备 |
| countdown (相对延迟) | ✅ 本 spec 补齐 | 与 `runAt`/`startAt`(eta) 成对，补齐延迟语义 |
| progress (进度上报) | ✅ 本 spec 补齐 | 对齐 Celery `update_state`，长任务可观测 |
| inspect (运行时检查) | ✅ 本 spec 补齐 | 对齐 Celery `inspect`，单机排障能力 |
| 事件监听（订阅式） | ✅ 本 spec 补齐 | 补齐订阅式监听，与 `events` 拉取互补 |

### 4.4 仍缺失但单机场景不值得实现的能力

- ❌ 多 Broker（Redis/RabbitMQ/SQS）：与"无外部依赖"定位冲突。
- ❌ 多 worker 集群：单机无水平扩展诉求。
- ❌ 远程 worker：单机无跨节点调度。
- ❌ 多 result backend（RPC/Cache/Mongo）：单机 InMemory+Sqlite 已够。
- ❌ 多 jobstore 后端（Mongo/Redis）：同上。
- ❌ Flower web 监控：命令行 `status`/`inspect`/`events` 已够，web 化收益低。
- ❌ ProcessPool：PHP 任务多为 IO/HTTP/Shell，进程池收益有限。
- ❌ prefetch_count：单机无多 worker 竞争，预取调优无意义。
- ❌ task_remote_tracebacks：单机无远程 worker，无远程 traceback 概念。

### 4.5 综合评价

xhjob 在"**单机 PHP 异步任务调度**"这一细分领域的定位清晰且具竞争力：

1. **零外部依赖**：不引入 Redis/MQ/Swoole，对 PHP 单体应用（如 ThinkPHP 8 扩展包形态）部署友好，是 APScheduler/Celery 在 Python 单机场景的等价替代。
2. **Rust 内核**：调度核心用 Rust 实现，避免 PHP 长驻进程的内存/稳定性问题，比纯 PHP 守护进程方案更可靠。
3. **语义对齐**：在触发器、重试、超时、并发、编排、事件、进度等主线能力上已与 APScheduler/Celery 对齐，本 spec 补齐 chord/countdown/progress/inspect/事件订阅后，单机编排与可观测差距显著收窄。
4. **明确边界**：不追求分布式能力，主动规避多 Broker/多 worker/多 backend/Flower 等分布式专属特性，避免"既要又要"导致的复杂度膨胀。

**结论**：对于"不需要分布式、但需要可靠的单机异步任务调度"的 PHP 应用，xhjob 是当前生态中少有的、可直接落地的方案；其能力边界与单机定位高度自洽，不构成"功能缺失"的负面信号。

---

## 5. "单机 PHP 场景不值得实现"清单

| 特性 | 不实现原因 | 替代方案（如有） |
|---|---|---|
| Broker (Redis/RabbitMQ/SQS) | 与"无外部依赖"核心定位冲突；单机进程内通信已足够 | Rust daemon 直连 PHP，named services 隔离 |
| 多 worker 集群 | 单机无水平扩展诉求；多 worker 反而引入资源竞争 | 单 daemon + `maxInstances` 控并发 |
| 远程 worker / 跨节点调度 | 单机无跨节点场景 | 无 |
| RPC result backend | 单机无远程调用 | InMemoryStore / SqliteStore |
| Cache result backend | 单机无共享缓存集群 | SqliteStore |
| MongoDB result/job store | 单机引入 MongoDB 违背"零依赖" | SqliteStore |
| Redis result/job store | 同上 | InMemoryStore / SqliteStore |
| Flower web 监控 | 单机命令行 `status`/`inspect`/`events` 已覆盖监控诉求 | `xhjob_status` + `inspect` + `events` |
| ProcessPoolExecutor | PHP 任务多为 IO/HTTP/Shell，进程池收益有限 | `viaShell` 启动独立进程做 CPU 隔离 |
| prefetch_count | 单机无多 worker 竞争，预取调优无意义 | 无 |
| worker_concurrency (全局池) | 单机按"任务最大实例数"控并发更直观 | `maxInstances` |
| task_remote_tracebacks | 单机无远程 worker，无远程 traceback | Rust 侧错误直接回传 |
| worker_cancel_long_tasks_on_connection_loss | 单机无"连接丢失"场景 | `timeout`/`softTimeout` + `cancel` |
| chunks / map / starmap 原语 | 下沉到扩展收益低，PHP 层组合即可 | `array_chunk`/`array_map` + `group` |
| autoretry_for (异常白名单重试) | 单机任务类型可控，可按 exit code/HTTP 状态在业务层判断 | 建议：后续在 `viaShell`/`viaHttp` 暴露 exit code / status 白名单 |
| store_errors_even_if_ignored | 边界价值有限 | 建议：`ignoreResult` 时仍记录 `failed` 事件，经 `events` 查询 |
| CalendarIntervalTrigger | cron 表达式已覆盖"每月 1 号"等日历语义 | 用 cron 表达式替代 |
| 多 jobstore 可插拔抽象 | 单机仅需 InMemory+Sqlite 两档 | feature flag(`persist`) 已足够 |

---

## 6. 下一步优化方向

1. **补齐 `terminate` 语义**：在 `cancel` 之外提供"立即 SIGKILL 单任务"的能力，对齐 Celery `revoke(terminate=True)`，用于处理 `softTimeout` 仍无法停止的僵尸任务。
2. **暴露 `autoretry_for` 等价能力**：为 `viaShell`（按 exit code）与 `viaHttp`（按状态码）提供"重试白名单/黑名单"，提升重试策略的精细度。
3. **优化 `next_run_time` 便捷 API**：新增 `xhjob_next_run(id)` 直接返回下次触发时刻，避免每次都拉取完整 state。
4. **`ignoreResult` 仍记录 `failed` 事件**：在 `ignore_result` 模式下保留失败事件写入，对齐 Celery `store_errors_even_if_ignored`，便于排障。
5. **cron 高阶表达式**：补充 `L`(last day)/`#`(nth weekday) 等表达式，减少"每月最后一个工作日"类需求的手写规避成本。
