# 第二轮深度审查与功能完善 Spec

## Why

上一轮 `audit-and-polish-from-user-perspective` 修复了 9 个使用者视角的真实问题（含 1 个 fatal error），并以 43/43 单测 + 7/7 .phpt + 全部 examples 通过收尾。但深入审查代码细节后发现，仓库中仍存在一批上一轮未触及的真实问题，且使用者视角还有可显著提升的功能空白：

1. **Dead code 的"潜在价值"未被启用**：`is_retryable_http_status` / `is_retryable_shell_exit` 已实现但 `should_retry` 中 `let _ = error;` 直接返回 true，导致 HTTP 404 等不可重试错误也会被重试 N 次——这是使用者会踩的真实坑。`Event` / `should_fire_missed` / `thread_pool` / `make_store` 等是未来扩展接口，应加注释保留而非删除。
2. **误导性 API 参数**：`scheduler::cron::next_fire` 接受 `seconds: bool` 但函数体直接 `let _ = seconds;` 忽略它；调用方都传 `false`，给后续维护者造成"是否支持秒级 cron"的疑惑。
3. **cron 表达式非法时静默 warn**：`TaskBuilder::build` 中 cron 解析失败只 `tracing::warn!`，task 仍入队但 `next_fire=None`，永远不被触发。用户得到 task_id 却等不到执行，无法排查。
4. **缺少 cron 执行次数限制**：用户明确希望"定时任务可以指定执行次数，次数执行完毕就停止结束任务"——这是 APScheduler `max_executions` 概念，当前缺失。
5. **文档关键说明缺漏**：`XHJOB_PERSIST` 只在 daemon 启动时读取一次、5/6 段 cron 表达式格式（6 段含秒）未在 README 明确、`examples/cron_http.php` dispatch 后 daemon 仍持续触发 cron 未提示。
6. **与 Python 成熟框架的差距未对齐**：原始 spec（`implement-async-task-scheduler`）已明确"参考 APScheduler CronTrigger / max_instances / coalesce"与"参考 Celery retry / AsyncResult"。对照成熟框架审视当前实现，发现多个 APScheduler/Celery 既有但当前缺失的、在单机版合理可用的能力（详见下文"对照 Python 成熟框架对齐"章节）。
7. **maxExecutions 终态判定时机 bug**：`src/scheduler/queue.rs::process_one` 中 success 路径先调用 `update_state(Success)` 再调用 `increment_execution_count`，导致 cron 任务第一次执行后立即变为 Success 终态。`scan_once` 通过 `load_active_tasks` 过滤终态任务，因此不再触发该任务，`execution_count` 永远停在 1。`tests/max_executions.php` 测试已复现此 bug（4 PASS / 2 FAIL）。**修复**：cron 任务（`task.cron.is_some()`）success 后不应立即标记 Success，应保持可触发状态让 scan_once 继续；仅当 `execution_count >= max_executions`（且 max_executions > 0）时才标记 Success。非 cron 任务保持原行为（success 后立即终态）。
8. **更多 APScheduler/Celery 既有能力可补充**：除已对齐的 A1-A6 + C1-C5 外，对照 APScheduler `IntervalTrigger` / `DateTrigger` / `jitter`、Celery `expires` / `requeue` / `retry_backoff` 等，仍有多项单机版合理可用的能力尚未实现（详见下文 A7-A9 + C6-C8 新增对齐项）。
9. **第三轮深度对比仍可补齐 6 项**：进一步对照 APScheduler `max_instances`（N>1 真实并发实例）/ `reschedule_job`（在线修改 cron）/ `get_job`（单任务详情查询），与 Celery `task_ignore_result`（fire-and-forget 不存结果）/ `task_acks_late`（崩溃恢复重排）/ `task_soft_time_limit`（软超时优雅退出），发现当前实现仍有 6 项单机版合理可用的能力空白（详见下文 A10-A12 + C9-C11 第三轮对齐项）。当前 `max_instances` 字段虽已存在但被 `allow_overlap` 限制为二元开关，未能真正支持"N>1 并发实例"场景；`reschedule_job` 完全缺失，运维改 cron 必须删旧 dispatch 新；`get_job` 完全缺失，单任务详情查询只能用 `xhjob_state` 拿到有限 StateInfo 字段；`ignore_result` 完全缺失，fire-and-forget 任务仍占满 results 表；`acks_late` 完全缺失，daemon 崩溃后 Running 任务永久卡死；`soft_timeout` 完全缺失，shell 任务超时只能 SIGKILL，无优雅退出窗口。
10. **第四轮深度对比仍可补齐 6 项**：再深入对照 APScheduler `misfire_grace_time`（每作业级覆盖，当前仅全局默认 60s）/ `replace_existing`（同 id 幂等替换，当前 dispatch 同 id 会冲突）/ tags（作业分组与按 tag 过滤 list），与 Celery `rate_limit`（每任务限流，外部 API 调用场景必备）/ `task_acks_on_failure_or_timeout`（失败时是否确认，与 acksLate 互补，关键任务永不放弃）/ `worker_max_tasks_per_child`（daemon 进程自我回收防内存泄漏），发现当前实现仍有 6 项单机版合理可用的能力空白（详见下文 A13-A15 + C12-C14 第四轮对齐项）。当前 `misfire_grace_time` 完全缺失 per-job 字段，用户无法为不同任务设不同容错窗口；`replace_existing` 完全缺失，部署脚本重跑必须先手动 remove 旧 task；`tags` 完全缺失，运维无法按业务标签批量过滤（如只查 'reports' 类任务）；`rate_limit` 完全缺失，cron 调用外部 API 无法限速（容易触发 429 限流）；`acks_on_failure` 完全缺失，失败任务严格按 retry_max 终态，关键任务无法"永不放弃直到成功"；`worker_max_tasks_per_child` 完全缺失，daemon 长跑后内存泄漏无法自愈。
11. **第五轮深度对比仍可补齐 6 项**：再深入对照 APScheduler `timezone`（per-job 时区，参考 `CronTrigger(timezone=...)`）/ `add_listener` + `EVENT_JOB_*` 事件流（任务执行事件订阅，当前实现仅有日志）/ `coalesce` 显式 per-job 行为（当前 `coalesce` 字段虽存在但仅作为默认值，未真正控制 misfire 合并/丢弃行为），与 Celery `chain`（顺序流水线，前任务输出作为后任务输入，ETL 场景必备）/ `group`（并行批处理，等待一组任务全部完成，批量场景必备）/ `worker_max_memory_per_child`（基于内存使用 daemon 自我回收，与 C14 互补，针对内存泄漏而非任务数），发现当前实现仍有 6 项单机版合理可用的能力空白（详见下文 A16-A18 + C15-C17 第五轮对齐项）。当前 `timezone` 完全缺失 per-job 字段，所有 cron 任务用同一全局时区评估，跨国应用（如"纽约时间 9 点日报" vs "北京时间 9 点日报"）必须为每个任务写不同 cron 表达式或手工换算；事件流完全缺失，运维只能 grep 日志，无法用统一 API 查询任务执行事件流（如"过去 1 小时哪些任务失败"）；`coalesce` 字段虽存在但实际行为未真正按字段值控制 misfire 合并/丢弃，仅在 README 文档说明；任务链完全缺失，ETL 管道必须手工 dispatch + 轮询 + dispatch；任务组完全缺失，批量处理必须循环 dispatch 多个任务无法整体等待；`worker_max_memory_per_child` 完全缺失，daemon 内存泄漏只能靠"任务数"回收（C14），无法直接按"内存用量"回收。
12. **第六轮深度对比仍可补齐 6 项**：再深入对照 APScheduler `or_` / `and_` 复合触发器（多个 cron 表达式 OR 合并触发，当前必须 dispatch 多个任务实现"9 点或 18 点各跑一次"）/ calendar 触发器 + holiday skipping（按日期跳过触发，如工作日报告跳过周末与节假日，当前必须为每个节假日写 cron 排除规则或 cron 表达式 hack）/ `modify_job`（任意字段在线修改，当前只有 `xhjob_reschedule` 仅改 cron 字段，改 priority / retry_max / timeout 必须删旧 dispatch 新丢失 state / attempts / execution_count），与 Celery `chord`（header group 完成后触发 callback 接收所有 results，Map-Reduce 模式必备，当前 C16 group 只能并行执行无 callback 聚合）/ `chunks`（将大列表切分为 N 个 chunk 并行 dispatch，批量处理 10000 条记录场景必备，当前必须循环 dispatch 无法整体管理）/ `worker_concurrency`（daemon 全局并发上限，区别于 per-task `maxInstances`，当前 daemon 无全局并发限制，N 个不同任务可同时 Running 撑爆系统资源），发现当前实现仍有 6 项单机版合理可用的能力空白（详见下文 A19-A21 + C18-C20 第六轮对齐项）。当前复合触发器完全缺失，"9 点 + 18 点日报"必须 dispatch 2 个 task 浪费存储；节假日跳过完全缺失，工作日报告周末照常触发产生无效输出；`modify_job` 完全缺失，调整任务参数必须重新 dispatch 丢失历史；chord 完全缺失，Map-Reduce 必须手工 group + 轮询 + dispatch callback；chunks 完全缺失，大列表批处理必须手工切分循环 dispatch；`worker_concurrency` 完全缺失，daemon 无全局并发限制易被 N 个并发任务撑爆 CPU / 内存。

本轮聚焦：修复 maxExecutions bug、启用真实有价值的功能、引入 cron 执行次数限制、对齐 Python 成熟框架补齐功能（含 A7-A9 + C6-C8 六项 + A10-A12 + C9-C11 六项第三轮深度对齐 + A13-A15 + C12-C14 六项第四轮深度对齐 + A16-A18 + C15-C17 六项第五轮深度对齐 + **A19-A21 + C18-C20 六项第六轮深度对齐**）、补齐文档与边界测试。允许引入新功能特性。

## What Changes

### 新功能

- **新增 cron 任务执行次数限制**：`TaskBuilder::max_executions(n)` / PHP `maxExecutions(int $n): $this`
  - `Task` 新增 `max_executions: u32`（0 = 无限，默认）与 `execution_count: u32`（已执行次数，默认 0）
  - cron 触发并执行后 `execution_count += 1`
  - 当 `max_executions > 0 && execution_count >= max_executions` 时，state 置为 `Success` 并停止后续触发
  - SQLite schema 新增两列；旧库自动 `ALTER TABLE` 兼容
  - 顶层函数 `xhjob_dispatch` 透传新字段
- **`should_retry` 按错误类型真实判断**：启用 `is_retryable_http_status` / `is_retryable_shell_exit`
  - HTTP 5xx + 网络错误才重试；4xx 不重试直接 FAILED
  - shell 任何非零 exit 仍重试（保持当前行为）
  - `RetryPolicy::should_retry` 签名调整为接受 `&TaskResult` 而非 `&str`
- **A7. IntervalTrigger `every(N)`**：`TaskBuilder::every(secs: u64)` / PHP `every(int $secs): $this`，`Task` 新增 `interval: Option<u64>`，scan_once 计算 `next_fire = now + interval`
- **A8. DateTrigger `runAt(ts)`**：`TaskBuilder::run_at(ts: i64)` / PHP `runAt(int $ts): $this`，`Task` 新增 `run_at: Option<i64>`，触发后立即终态（一次性）
- **A9. Jitter `jitter(secs)`**：`TaskBuilder::jitter(secs: u64)` / PHP `jitter(int $secs): $this`，`Task` 新增 `jitter: u64`，scan_once 在 next_fire 后追加 `0..jitter` 随机偏移
- **C6. Task expires**：`TaskBuilder::expires(secs: u64)` / PHP `expires(int $secs): $this`，`Task` 新增 `expires: u64`，`TaskState` 新增 `Expired` 终态，scan_once 检测入队后未执行超时则置 Expired
- **C7. Task requeue**：`xhjob_requeue($id)` 重置 Cancelled/Failed/Expired 任务为 Pending + 清零 attempts + next_fire=now + 保留 task 定义
- **C8. Retry exponential backoff**：`TaskBuilder::retry_backoff(bool)` / PHP `retryBackoff(bool $on): $this`，`Task` 新增 `retry_backoff: bool`，schedule_retry 计算 `delay * 2^(attempts-1)` 上限 `delay * 60`
- **A10. max_instances(N) 真实生效（升级版）**：解耦 `max_instances` 与 `allow_overlap`，让 `max_instances: u32` 真正作为"并发实例上限"使用：N=1（默认）= 不允许重叠；N>1 = 允许至多 N 个 Running 实例并发；与 `allow_overlap=true` 配合时取较宽松者（向后兼容）。`OverlapController::should_fire` 改为检查 `count_running_instances(id) < max_instances`（max_instances>0 时）
- **A11. reschedule_job（在线修改 cron）**：`xhjob_reschedule($id, $cron)` / `Xhjob::reschedule($id, $cron)`，更新 `task.cron` + 重新计算 `next_fire = cron_next(now)`，保留 state / attempts / execution_count / meta 等所有现有字段；运维改 cron 频率无需删旧 dispatch 新
- **A12. get_job（单任务详情查询）**：`xhjob_get($id)` 返回完整 Task JSON（含所有配置字段：cron / interval / run_at / retry / timeout / priority / max_instances / max_executions / start_date / end_date / result_ttl / meta / state / attempts / next_fire 等）；区别于 `xhjob_state`（仅返回 StateInfo 摘要）与 `xhjob_list`（返回 TaskSummary 列表）
- **C9. ignoreResult（fire-and-forget 不存结果）**：`TaskBuilder::ignore_result(bool)` / PHP `ignoreResult(bool $on): $this`，`Task` 新增 `ignore_result: bool`（默认 false），`process_one` 中跳过 `save_result` 调用；`xhjob_result($id)` 返回 null；state 转换仍正常
- **C10. acksLate（延迟确认 / 崩溃恢复）**：`TaskBuilder::acks_late(bool)` / PHP `acksLate(bool $on): $this`，`Task` 新增 `acks_late: bool`（默认 false），daemon 启动时扫描所有 Running 任务，对 `acks_late=true` 的任务重置为 Pending + next_fire=now（重新入队）；适用于"daemon 崩溃后 Running 任务不应卡死"场景
- **C11. softTimeout（软超时优雅退出）**：`TaskBuilder::soft_timeout(secs: u64)` / PHP `softTimeout(int $secs): $this`，`Task` 新增 `soft_timeout: Option<u64>`（默认 None），shell 任务超时先发 SIGTERM 等待 `(timeout - soft_timeout)` 秒后 SIGKILL；HTTP 任务设置时 warn 忽略（HTTP 客户端不支持优雅中断）；`soft_timeout >= timeout` 时 warn 并忽略 soft_timeout
- **A13. misfire_grace_time（每作业级容错窗口）**：`TaskBuilder::misfire_grace_time(secs: u64)` / PHP `misfireGraceTime(int $secs): $this`，`Task` 新增 `misfire_grace_time: u64`（默认 0 = 使用全局默认 60s）；`scan_once` 检测到 `now > next_fire` 时，若 `now - next_fire > grace_time` 则视为 misfire（按 coalesce 规则合并/丢弃并推进 next_fire），否则仍触发；区别于全局默认，per-job 字段允许不同任务用不同容错窗口（如关键任务 300s / 普通任务 30s）
- **A14. replace_existing（幂等 dispatch）**：`TaskBuilder::id(s: impl Into<String>)` / PHP `withId(string $id): $this` + `TaskBuilder::replace_existing(bool)` / PHP `replaceExisting(bool $on): $this`；dispatch 时若 `replace_existing=true` 且 store 中已存在同 id 任务，先 remove 旧 task 再 insert 新 task（完全替换，state/attempts/execution_count 不保留，因为是新定义）；若 `replace_existing=false`（默认）且 id 冲突返回 `error: task id already exists`；不设置 id 时系统仍生成 UUID（replace_existing 无效果）。适用场景：部署脚本重跑幂等、cron 注册同 id 自动覆盖
- **A15. tags（作业分组与按 tag 过滤）**：`TaskBuilder::tags(&[&str])` / PHP `tags(array $tags): $this`，`Task` 新增 `tags: Vec<String>`（默认空，serde 序列化为 JSON 数组）；`xhjob_list($name, $state_filter=null, $tag=null)` 新增可选 `$tag` 参数过滤包含该 tag 的任务；SQLite schema 新增 `tags TEXT` 列存 JSON。适用场景：运维按业务标签批量查询（如只看 'reports' 类任务、只看 'critical' 类任务）
- **C12. rate_limit（每任务限流）**：`TaskBuilder::rate_limit(max_count: u32, window_secs: u64)` / PHP `rateLimit(int $maxCount, int $windowSecs): $this`，`Task` 新增 `rate_limit_count: u32`（默认 0=不限流）+ `rate_limit_window: u64`（默认 0）；`OverlapController::should_fire` 或 scan_once 触发前查询：在 `window_secs` 滑动窗口内已开始执行的实例数 >= `max_count` 时跳过本次触发并推进 next_fire；用 in-memory 滑动窗口计数器（daemon 重启后从 `started_at` 重建）。适用场景：cron 调用外部 API 限速（如每分钟最多 10 次避免 429）
- **C13. acks_on_failure（失败时是否确认，与 acksLate 互补）**：`TaskBuilder::acks_on_failure(bool)` / PHP `acksOnFailure(bool $on): $this`，`Task` 新增 `acks_on_failure: bool`（默认 true）；`process_one` 中任务执行失败时，若 `acks_on_failure=true`（默认）保持当前行为（按 retry_max 重试或终态 Failed，向后兼容）；若 `acks_on_failure=false` 则失败时不进入终态，重置为 Pending + next_fire=now+retry_delay（指数退避若启用），**忽略 retry_max 上限**，任务"永不放弃直到成功或被 cancel/remove"。适用场景：关键通知任务、订单状态同步任务，必须最终成功
- **C14. worker_max_tasks_per_child（daemon 自我回收）**：环境变量 `XHJOB_MAX_TASKS_PER_CHILD=N`（默认 0=不回收），daemon 启动时读取一次；运行时维护原子计数器 `tasks_executed_since_start`，每次 `process_one` 完成后递增；当 `tasks_executed_since_start >= N` 时 daemon 优雅退出（关闭 scheduler + queue + 等待 in-flight 任务完成 + flush store + exit 0）；由外部进程管理器（systemd / supervisor / docker restart=always）自动重启。适用场景：daemon 长跑后内存泄漏自愈、定时重启释放资源
- **A16. timezone per-job（每作业独立时区）**：`TaskBuilder::timezone(tz: impl Into<String>)` / PHP `timezone(string $tz): $this`，`Task` 新增 `timezone: Option<String>` 字段（默认 None = 使用 daemon 全局时区，向后兼容）；`scan_once` 中 cron 表达式评估时使用 `task.timezone` 解析（IANA 时区标识，如 `"America/New_York"` / `"Asia/Shanghai"`），解析失败时 warn 并回退到全局时区；非 cron 任务（interval / runAt）设置 `timezone` 时 warn 并忽略。适用场景：跨国应用，每个 cron 任务按业务所在时区评估"9 点日报"而非按 daemon 全局时区
- **A17. Event listener API（任务执行事件流查询）**：新模块 `src/scheduler/events.rs` + `TaskStore` 新增 `record_event(task_id, event_type, payload, ts)` / `list_events(since_ts, task_id_filter)` trait 方法；`process_one` / `scan_once` / `cancel_task` / `pause` / `resume` 等关键路径写入事件（类型：`started` / `succeeded` / `failed` / `missed` / `cancelled` / `paused` / `resumed` / `expired`）；SQLite schema 新增 `events` 表（`id INTEGER PK` / `task_id TEXT` / `event_type TEXT` / `payload TEXT` / `ts INTEGER`）；新增 PHP 顶层函数 `xhjob_events($since_ts, $name='default', $task_id=null, $data_dir=null): string` 返回 JSON 数组；事件 TTL 自动清理（默认 24h，可通过 `XHJOB_EVENTS_TTL_SECS` 环境变量配置）。适用场景：监控集成（如"过去 1 小时哪些任务失败"），运维告警，业务侧轮询任务执行状态变化
- **A18. coalesce 显式 per-job 行为（misfire 合并/丢弃规则）**：`TaskBuilder::coalesce(on: bool)` / PHP `coalesce(bool $on): $this`，`Task` 已存在 `coalesce: bool` 字段（默认 true 保持当前行为）；`scan_once` 中 misfire 检测时（配合 A13 `misfire_grace_time`）真正按 `task.coalesce` 字段控制：`coalesce=true`（默认）→ 合并多次 missed 触发为最后一次执行 + 推进 next_fire；`coalesce=false` → 丢弃所有 missed 触发 + 推进 next_fire 至下一轮 cron 时刻（不执行任何补偿）。区别于 A4（仅 README 说明），A18 让字段真正生效。适用场景：批量任务用 `coalesce=true` 避免一次性补跑多次；幂等性任务用 `coalesce=false` 避免重复执行
- **C15. Task chain（顺序流水线）**：新增 PHP 顶层函数 `xhjob_chain(array $task_configs, $name='default', $data_dir=null): string` 返回 `chain_id`；daemon 中按顺序执行每个任务，前一个任务的 stdout 作为后一个任务的 input payload（前任务成功后才触发后任务，前任务失败则整个 chain 终态 Failed）；新增 PHP 顶层函数 `xhjob_chain_state($chain_id, $name='default', $data_dir=null): string` 返回 JSON（含 chain_id / tasks 数组 / current_step / state：`pending` / `running` / `succeeded` / `failed`）；SQLite schema 新增 `chains` 表（`chain_id TEXT PK` / `tasks TEXT` JSON 数组 / `current_step INTEGER` / `state TEXT` / `created_at INTEGER` / `updated_at INTEGER`）。适用场景：ETL 管道（extract → transform → load）、多步骤业务流程（订单创建 → 库存扣减 → 通知发送）
- **C16. Task group（并行批处理）**：新增 PHP 顶层函数 `xhjob_group(array $task_configs, $name='default', $data_dir=null): string` 返回 `group_id`；daemon 中并行 dispatch 所有任务（同时入队），等待所有任务终态后 group 整体完成；新增 PHP 顶层函数 `xhjob_group_state($group_id, $name='default', $data_dir=null): string` 返回 JSON（含 group_id / tasks 数组 / completed_count / total_count / state：`pending` / `running` / `succeeded` / `partial_failed` / `failed`）；SQLite schema 新增 `groups` 表（`group_id TEXT PK` / `tasks TEXT` JSON 数组 / `state TEXT` / `created_at INTEGER` / `updated_at INTEGER`）。适用场景：批量处理（如同时发送 1000 封邮件）、Map-Reduce 模式（map 阶段并行执行多个独立子任务）
- **C17. worker_max_memory_per_child（基于内存的 daemon 自我回收）**：环境变量 `XHJOB_MAX_MEMORY_PER_CHILD=MB`（默认 0=不回收），daemon 启动时读取一次；运行时每次 `process_one` 完成后读取 `/proc/self/status` 中 `VmRSS` 字段（KB 单位）转换为 MB；当 `vm_rss_mb >= XHJOB_MAX_MEMORY_PER_CHILD` 时 daemon 优雅退出（同 C14 流程：关闭 scheduler + queue + 等待 in-flight 任务完成 + flush store + exit 0）；与 C14 `max_tasks_per_child` 互补：C14 按"任务数"回收防累积，C17 按"内存用量"回收防内存泄漏，两者独立生效，先到达阈值者先触发退出。适用场景：daemon 处理大 payload 任务导致内存增长（如处理大 JSON / 大文件），无法靠"任务数"回收时按"内存"回收
- **A19. or_ 复合触发器（多个 cron 表达式 OR 合并）**：`TaskBuilder::or_cron(exprs: &[&str])` / PHP `orCron(array $exprs): $this`，`Task` 新增 `cron_expressions: Vec<String>` 字段（默认空 Vec，与 `cron` 字段互斥）；`scan_once` 中：若 `cron_expressions` 非空，对每个表达式独立计算 next_fire，取最小值作为本任务 next_fire；触发后推进所有匹配的表达式（其他未匹配的表达式保持原 next_fire）；与 `cron` 字段同时设置时 `cron` 优先（向后兼容），`or_cron` 被忽略并 warn；与 `interval` / `runAt` 互斥。SQLite schema 新增 `cron_expressions TEXT` 列存 JSON 数组。适用场景：日报任务"9 点 + 18 点各跑一次"、运维任务"工作日 9 点 + 周末 10 点"
- **A20. calendar 触发器 + holiday skipping（按日期跳过触发）**：`TaskBuilder::skip_dates(dates: &[&str])` / PHP `skipDates(array $dates): $this` + `TaskBuilder::workdays_only(on: bool)` / PHP `workdaysOnly(bool $on): $this`，`Task` 新增 `skip_dates: Vec<String>`（YYYY-MM-DD 格式日期列表，默认空）+ `workdays_only: bool`（默认 false）字段；`scan_once` 中 cron/interval 触发前检查 `now` 的本地日期：若日期在 `skip_dates` 列表中 或 `workdays_only=true` 且是周末（周六/周日），则跳过本次触发并推进 next_fire 至下一轮 cron 时刻；不引入第三方日历库（避免依赖），仅支持手动指定日期列表 + 周末判定（用户可定期更新 skip_dates 列表实现节假日跳过）；与 cron / interval 配合生效，与 runAt 一次性任务互斥（runAt 是绝对时刻不跳过）。SQLite schema 新增 `skip_dates TEXT` 列存 JSON 数组 + `workdays_only INTEGER NOT NULL DEFAULT 0` 列。适用场景：工作日报告跳过周末、节假日跳过（如春节/国庆手动配置 skip_dates）、特殊维护日跳过
- **A21. modify_job（任意字段在线修改）**：`xhjob_modify($id, array $changes)` / `Xhjob::modify($id, array $changes)`，`TaskStore` trait 新增 `modify_task(id, changes: &serde_json::Value) -> Result<bool>` 方法；允许在线更新任意 task 配置字段（priority / retry_max / retry_delay / timeout / soft_timeout / max_instances / coalesce / max_executions / start_date / end_date / result_ttl / meta / ignore_result / acks_late / tags / rate_limit_count / rate_limit_window / acks_on_failure / timezone / misfire_grace_time / expires / skip_dates / workdays_only / cron_expressions 等），但**不可更新** state / attempts / execution_count / next_fire / id / created_at / started_at / finished_at（这些是运行时状态字段，应由系统管理）/ `cron`（用 `xhjob_reschedule` 专门处理）；更新成功后若涉及调度字段（cron / interval / run_at / start_date / end_date / max_executions / tags / timezone / skip_dates / workdays_only / cron_expressions / misfire_grace_time / coalesce）则重新计算 next_fire；区别于 `xhjob_reschedule`（仅改 cron 字段）与 `xhjob_requeue`（重置终态任务），`xhjob_modify` 是通用字段更新；失败原因：id 不存在 / 字段非法 / 值类型不匹配 / 字段不允许更新（如 state）。适用场景：运维发现任务 timeout 过短在线调整、调整 priority 让重要任务优先、更新 tags 重新分组
- **C18. chord（header group + callback）**：新增 PHP 顶层函数 `xhjob_chord(array $header_tasks, $callback_config, $name='default', $data_dir=null): string` 返回 `chord_id`；daemon 收到 chord dispatch 后并行 dispatch 所有 header tasks（同时入队，按 priority 排队执行）；header task 完成后：completed_count += 1，记录每个 task 的 result 到 chords 表；全部 header tasks 完成后：将所有 results 作为 callback 的 input payload（JSON 数组，通过 `XHJOB_CHORD_INPUT` 环境变量传入）→ dispatch callback → update_chord_state("running")；callback 完成后：update_chord_state("succeeded") 或 "failed"；新增 `xhjob_chord_state($chord_id, $name='default', $data_dir=null): string` 返回 JSON（含 `chord_id` / `header_tasks` 数组（每个元素含 task_id + state + result）/ `callback` 对象（含 task_id + state）/ `state`：`pending` / `running` / `succeeded` / `failed` / `partial_failed`）；SQLite schema 新增 `chords` 表（`chord_id TEXT PRIMARY KEY` / `header_tasks TEXT NOT NULL` JSON 数组 / `callback_config TEXT NOT NULL` JSON / `state TEXT NOT NULL DEFAULT 'pending'` / `results TEXT` JSON 数组（按完成顺序存 header results）/ `completed_count INTEGER NOT NULL DEFAULT 0` / `total_count INTEGER NOT NULL` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）。区别于 C15 chain（顺序执行）与 C16 group（仅并行无 callback），chord 是"并行 header + 顺序 callback"模式，Map-Reduce 必备：map = header group 并行处理分片，reduce = callback 聚合所有 results。适用场景：Map-Reduce 聚合（map 100 个分片计算 → reduce 求和）、批量抓取 + 汇总（抓 100 个 URL → 汇总去重）、分布式计算（并行 N 个独立计算 → 合并结果）
- **C19. chunks（大列表切分并行批处理）**：新增 PHP 顶层函数 `xhjob_chunks(array $task_template, array $items, int $chunk_size, $name='default', $data_dir=null): string` 返回 `chunks_id`；daemon 收到 chunks dispatch 后按 `chunk_size` 将 `items` 切分为 N 块，每块作为一个独立 task dispatch（task payload 为 JSON `{chunk_index: N, items: [...], total_chunks: M, chunks_id: "..."}`，通过 `XHJOB_CHUNK_INPUT` 环境变量传入）；并行 dispatch 所有 chunk tasks（同时入队，按 priority 排队执行）；每个 chunk task 完成后：completed_count += 1，记录每个 chunk 的 result 到 chunks 表；全部 chunk tasks 完成后：update_chunks_state("succeeded")；新增 `xhjob_chunks_state($chunks_id, $name='default', $data_dir=null): string` 返回 JSON（含 `chunks_id` / `total_chunks` / `completed_count` / `state`：`pending` / `running` / `succeeded` / `partial_failed` / `failed` / `items_total`）；SQLite schema 新增 `chunks` 表（`chunks_id TEXT PRIMARY KEY` / `task_template TEXT NOT NULL` JSON / `items TEXT NOT NULL` JSON 数组 / `chunk_size INTEGER NOT NULL` / `total_chunks INTEGER NOT NULL` / `state TEXT NOT NULL DEFAULT 'pending'` / `completed_count INTEGER NOT NULL DEFAULT 0` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）。区别于 C16 group（用户自己组织 task_configs 数组），chunks 自动按 chunk_size 切分大列表为 N 个等大 task，简化批量处理代码。适用场景：批量发送 10000 封邮件（chunk_size=100 → 100 个 task 各发 100 封）、批量抓取 1000 个 URL（chunk_size=50 → 20 个 task 各抓 50 个）、批量更新 10000 条数据库记录（chunk_size=500 → 20 个 task 各更新 500 条）
- **C20. worker_concurrency（daemon 全局并发上限）**：环境变量 `XHJOB_WORKER_CONCURRENCY=N`（默认 0=不限制），daemon 启动时读取一次；daemon 维护全局原子计数器 `current_running_count: AtomicU32`；`process_one` 取任务前检查 `if worker_concurrency > 0 && current_running_count >= worker_concurrency`：等待（短时间 sleep 或 skip 本次取任务循环）；任务开始时 `current_running_count.fetch_add(1, Ordering::Relaxed)`，任务完成时 `fetch_sub(1, Ordering::Relaxed)`；区别于 per-task `maxInstances`（限制同一 task_id 的并发实例数），`worker_concurrency` 是 daemon 全局限制（所有任务合计至多 N 个并发实例），两者独立生效；与 `maxInstances` 配合：如 `XHJOB_WORKER_CONCURRENCY=10` + 任务 A `maxInstances(3)` + 任务 B `maxInstances(5)` → 任务 A 至多 3 个并发、任务 B 至多 5 个并发、A+B 合计至多 10 个并发。适用场景：daemon 资源有限（CPU 4 核 / 内存 4GB），限制总并发避免 OOM、配合 systemd CPUQuota 限制、防止突发流量撑爆数据库连接池

### 完善

- **修复 maxExecutions 终态判定时机 bug**：cron 任务 success 后不立即标记 Success 终态，仅当 `execution_count >= max_executions` 才 Success（详见 ADDED Requirements 中"修复 maxExecutions 终态判定时机 bug"）
- **cron 表达式非法时 dispatch 立即失败**：`TaskBuilder::build` 中 cron 解析失败返回 `Err`，让 `dispatch()` 立即返回 `error: invalid cron: ...`
- **简化 `next_fire` 签名**：删除被忽略的 `seconds: bool` 参数，调用方与单元测试同步调整
- **保留未来扩展接口并加注释**：`Event` / `should_fire_missed` / `make_store` / `default_db_path` / `thread_pool` 模块均加 `#[allow(dead_code)]` + 文档注释说明保留原因与未来启用路径（如 `Event` 用于未来 IPC 事件流、`thread_pool` 用于未来 CPU 密集任务、`make_store` 用于 store 工厂）
- **删除真正无用的 `sleep_for_retry`**：retry 已通过 `next_fire` 机制实现延迟，此函数无启用路径

### 对照 Python 成熟框架对齐（APScheduler + Celery）

参考 [APScheduler](https://apscheduler.readthedocs.io/)（成熟定时任务模块）与 [Celery](https://docs.celeryq.dev/)（成熟后台任务队列模块），对当前实现做能力对比与补齐。**仅引入单机版合理可用、不引入外部依赖（不依赖 Redis / RabbitMQ / 进程池管理器）的特性**：

#### 对齐 APScheduler

- **A1. 作业删除 API（参考 `remove_job`）**：APScheduler 提供 `remove_job(job_id)` 显式删除 cron 作业，当前 xhjob 只能靠 `xhjob_stop()` 全停。新增 `xhjob_remove($id)` 与 `Xhjob::remove($id)`（仅删 cron 作业定义，不影响正在执行的实例）。
- **A2. 作业暂停/恢复（参考 `pause_job` / `resume_job`）**：APScheduler 提供 `pause_job` / `resume_job` 让 cron 作业临时停止触发但保留定义。新增 `Task` 字段 `paused: bool`（持久化字段），`xhjob_pause($id)` / `xhjob_resume($id)`，cron `scan_once` 跳过 `paused=true` 任务。
- **A3. 起始/结束时间（参考 `next_run_time` 起止范围 `start_date` / `end_date`）**：APScheduler 支持作业仅在 `[start_date, end_date]` 区间内触发。新增 `Task` 字段 `start_date: Option<i64>` 与 `end_date: Option<i64>`（Unix ts）；`TaskBuilder::startAt($ts)` / `endAt($ts)` / PHP `startAt(int $ts)` / `endAt(int $ts)`；`scan_once` 在 `now < start_date` 跳过触发（仍按 cron 推进 next_fire），在 `now > end_date` 后将 state 置为 `Success` 并停止后续触发（与 `max_executions` 终止逻辑复用）。
- **A4. misfire 策略显式化（参考 `misfire_grace_time` + `coalesce` 已有，补 README 说明）**：当前 `coalesce` 字段已存在但文档未说明 misfire 行为。README 新增"misfire 处理"小节明确：错过触发在 grace_time（默认 60s）内的合并/丢弃规则。
- **A5. 任务列举 API（参考 `get_jobs`）**：APScheduler 提供 `get_jobs()` 列出所有已注册作业。新增 `xhjob_list($name='default', $state_filter=null): array` 返回当前 service 的 task 摘要列表（id/type/state/cron/attempts/next_fire/paused），便于运维查询与监控。
- **A6. 单次触发 `next_run_time` 暴露**：APcheduler 每个 job 都有 `next_run_time`。当前 `Task.next_fire` 已存在但 `xhjob_state` 未返回。`xhjob_state` 返回新增 `next_fire` 字段（向后兼容追加）。
- **A7. IntervalTrigger（`every(N)` 秒级周期）**：参考 APScheduler `IntervalTrigger`。当前用户必须写 6 段 cron `*/30 * * * * *` 才能实现"每 30 秒一次"。新增 `TaskBuilder::every(secs: u64)` / PHP `every(int $secs): $this`：`Task` 新增 `interval: Option<u64>` 字段；`scan_once` 检测到 `interval.is_some()` 时直接计算 `next_fire = now + interval` 而非解析 cron。配置直观，且避免 cron 5/6 段歧义。与 cron 互斥（同时设置时 cron 优先，every 被忽略并 warn）。
- **A8. DateTrigger（`runAt(ts)` 一次性绝对时刻触发）**：参考 APScheduler `DateTrigger`。当前 `delay(seconds)` 是相对延迟，用户经常需要"在某绝对时刻执行一次"。新增 `TaskBuilder::run_at(ts: i64)` / PHP `runAt(int $ts): $this`：`Task` 新增 `run_at: Option<i64>` 字段；`next_fire = ts`，触发后立即置 `Success` 终态（一次性任务，不重新调度）。与 `delay(seconds)` 互补：delay 是相对当前时刻，runAt 是绝对时刻。
- **A9. Jitter（随机抖动避免惊群）**：参考 APScheduler `jitter` 参数。当多个 cron 任务在同一时刻触发（如所有每分钟任务都在 `:00` 秒），会造成瞬时压力。新增 `TaskBuilder::jitter(secs: u64)` / PHP `jitter(int $secs): $this`：`Task` 新增 `jitter: u64`（默认 0）；`scan_once` 在计算 `next_fire` 后追加 `0..jitter` 的随机偏移，分散触发时机。仅对 cron / interval 任务生效；runAt 一次性任务不抖动。
- **A10. max_instances(N) 真实生效（升级版，参考 APScheduler `max_instances`）**：APScheduler 中每个 job 都有 `max_instances` 参数（默认 1），允许同一 job 同时至多 N 个实例运行。当前 xhjob 的 `Task.max_instances: u32` 字段已存在（默认 1），但 `OverlapController::should_fire` 仅在 `allow_overlap=true` 时允许并发，且并发数无上限——`max_instances` 实际未生效。**升级方案**：解耦 `max_instances` 与 `allow_overlap`：
  - `max_instances=1`（默认）：不允许并发（当前行为，向后兼容）
  - `max_instances=N`（N>1）：允许至多 N 个 Running 实例并发，`should_fire` 检查 `count_running_instances(id) < N`
  - `allow_overlap=true` 且 `max_instances=1`：当前行为（无限并发，向后兼容）；如同时设置 `max_instances=N` 则取 N（更精确）
  - `allow_overlap=false` 且 `max_instances=N`：以 N 为准（`max_instances` 优先级高于 `allow_overlap`）
  - PHP `maxInstances(int $n): $this` builder 方法已存在，仅升级调度逻辑
  - 适用场景：1 分钟 cron 任务偶尔执行 90 秒，设 `maxInstances(2)` 允许至多 2 个并发实例避免漏触发；但不能无限并发避免堆积
- **A11. reschedule_job（参考 APScheduler `reschedule_job`）**：APScheduler 提供 `reschedule_job(job_id, trigger)` 在线修改 job 的 trigger 而不丢失作业历史。当前 xhjob 改 cron 必须先 `xhjob_remove($id)` 再 `dispatch` 新 task——丢失 state / attempts / execution_count / meta。新增 `xhjob_reschedule($id, $cron)` / `Xhjob::reschedule($id, $cron)`：
  - 更新 `task.cron` 字段
  - 重新计算 `next_fire = cron_next(now, tz)`（用任务原 timezone）
  - 保留所有其他字段：state / attempts / execution_count / meta / max_executions / start_date / end_date / result_ttl 等
  - 对非 cron 任务（interval / runAt / one-shot）调用 reschedule 返回 false 并 warn
  - 对终态任务调用 reschedule 返回 false（需先 requeue）
  - 适用场景：运维发现 cron 频率过高/过低，在线调整不中断业务
- **A12. get_job（参考 APScheduler `get_job`）**：APScheduler 提供 `get_job(job_id)` 返回完整 Job 对象（含 trigger / next_run_time / id / name / args 等所有字段）。当前 xhjob 只有 `xhjob_state`（返回 StateInfo 摘要，仅 state / attempts / next_fire / paused / start_date / end_date / meta 等）和 `xhjob_list`（返回 TaskSummary 列表）。新增 `xhjob_get($id)` 返回完整 Task JSON：
  - 包含所有配置字段：task_type / payload / cron / interval / run_at / retry_max / retry_delay / timeout / soft_timeout / priority / allow_overlap / max_instances / coalesce / max_executions / start_date / end_date / result_ttl / meta / ignore_result / acks_late / jitter / expires / retry_backoff / timezone / encoding / proxy
  - 包含所有状态字段：state / attempts / execution_count / next_fire / created_at / started_at / finished_at / last_error / paused / cancel_requested
  - 适用场景：调试单个任务时一次拿到全部配置 + 状态；区别于 `xhjob_state` 只看运行时状态，`xhjob_get` 含配置

#### 对齐 Celery

- **C1. 任务取消（参考 `revoke`）**：Celery 提供 `revoke(task_id)` 取消未执行的待运行任务。新增 `xhjob_cancel($id)`：将 state=Pending 的任务置为 `Cancelled`（新 state）终态；已 Running 的任务不能强制 kill 子进程（保持当前行为），但停止后续重试与 cron 触发。
- **C2. 重试时间上限（参考 `time_limit`）**：Celery 提供 `time_limit`（任务硬超时）。当前 `Task.timeout` 已覆盖单次执行超时，对齐方向是 README 明确"`timeout` 适用于单次执行而非总重试时长"，不新增字段（避免过度设计）。
- **C3. 任务优先级队列（已有字段，对齐 README 说明）**：`priority` 字段已存在但 scan_once 是否真按优先级排队需验证；如未实现则在队列层加按 priority 倒序的排序。
- **C4. 任务结果过期清理（参考 `result_expires`）**：Celery 默认 1 天后清理 result。新增 `Task` 可选字段 `result_ttl: Option<u64>`（秒，默认 0=永久保留）+ `TaskBuilder::resultTtl($secs)` / PHP `resultTtl(int $secs): $this`；任务终态 SUCCESS/FAILED 后，若 `now - finished_at > result_ttl` 则清理 results 行（保留 task 行）。
- **C5. 任务进度回调字段（参考 `update_state` 元数据）**：Celery 任务可写入自定义 `meta`。新增 `Task` 可选字段 `meta: Option<String>`（任意用户元数据 JSON 字符串）+ `TaskBuilder::withMeta(string $json): $this` / PHP `withMeta(string $json): $this`，`xhjob_state` 返回 `meta` 字段；不引入进度更新 API（避免过度设计，进度由用户在 meta 中自定义）。
- **C6. Task expires（任务级过期，区别于 result_ttl）**：参考 Celery `expires` 参数（task-level）。当前 `result_ttl` 是任务完成后 result 的保留时长；Celery `expires` 是任务**入队后若未开始执行**的过期时长。新增 `TaskBuilder::expires(secs: u64)` / PHP `expires(int $secs): $this`：`Task` 新增 `expires: u64`（默认 0=不过期）；`scan_once` 检测到 `task.expires > 0 && task.created_at + expires < now && task.state == Pending` 时，将 state 置为 `Expired`（新终态）并跳过执行。区别于 result_ttl（执行后清理），expires 是执行前丢弃。
- **C7. Task requeue（重新入队）**：参考 Celery `task.retry()` 与运维场景。当前 Cancelled/Failed/Expired 任务无法重新触发，用户必须重新 dispatch 一个新 task。新增 `xhjob_requeue($id)`：将终态任务（Cancelled/Failed/Expired）重置为 Pending，清零 attempts，重置 next_fire = now（立即入队），保留 task 定义（cron / interval / meta / max_executions 等）。Running/Pending 状态拒绝 requeue（返回 false）。适用于"修复 bug 后重试"或"取消后恢复执行"场景。
- **C8. Retry exponential backoff（指数退避）**：参考 Celery `retry_backoff=True`。当前 `retry_delay` 是固定值，对于网络抖动场景指数退避更友好。新增 `TaskBuilder::retry_backoff(bool)` / PHP `retryBackoff(bool $on): $this`：`Task` 新增 `retry_backoff: bool`（默认 false）；当 `on=true` 时，`schedule_retry` 计算 next_fire 时按指数退避 `delay * 2^(attempts-1)`，上限为 `delay * 60`（避免无限增长）。默认 false（保持固定 delay 行为，向后兼容）。
- **C9. ignoreResult（fire-and-forget 不存结果，参考 Celery `task_ignore_result`）**：Celery 全局配置 `task_ignore_result = True` 让 worker 不存储 result，适用于 fire-and-forget 场景（如发通知、触发钩子）。当前 xhjob 所有任务都会调用 `save_result` 写入 results 表，对 fire-and-forget 任务是浪费。新增 `TaskBuilder::ignore_result(bool)` / PHP `ignoreResult(bool $on): $this`：`Task` 新增 `ignore_result: bool`（默认 false）；`process_one` 中检测 `task.ignore_result=true` 时跳过 `save_result` 调用；`xhjob_result($id)` 返回 null（结果未存储）；state / attempts / execution_count 等状态字段仍正常更新。区别于 `result_ttl`（result_ttl 是存了再清理，ignoreResult 是根本不存）。
- **C10. acksLate（延迟确认 / 崩溃恢复，参考 Celery `task_acks_late`）**：Celery 中 `task_acks_late = True` 让任务在完成后才确认（默认是在 worker 接收时就确认）。如果 worker 在执行中崩溃，未确认的任务会被重新分配给其他 worker。当前 xhjob daemon 崩溃后，所有 Running 任务永久卡在 Running 状态（重启后 scan_once 不会重排它们）。新增 `TaskBuilder::acks_late(bool)` / PHP `acksLate(bool $on): $this`：`Task` 新增 `acks_late: bool`（默认 false）；daemon 启动时（`daemon_main::run` 初始化阶段）扫描所有 Running 任务，对 `acks_late=true` 的任务重置 state=Pending + next_fire=now（重新入队）；`acks_late=false`（默认）保持当前行为（Running 任务卡死）。适用场景：长时任务 + daemon 可能崩溃的场景，避免任务永久卡死。
- **C11. softTimeout（软超时优雅退出，参考 Celery `task_soft_time_limit`）**：Celery 区分 `task_time_limit`（硬超时，SIGKILL）与 `task_soft_time_limit`（软超时，抛 `SoftTimeLimitExceeded` 异常让任务优雅清理）。当前 xhjob 只有 `Task.timeout` 硬超时，shell 任务超时直接 SIGKILL 无优雅退出窗口。新增 `TaskBuilder::soft_timeout(secs: u64)` / PHP `softTimeout(int $secs): $this`：`Task` 新增 `soft_timeout: Option<u64>`（默认 None）：
  - shell 任务：先在 `soft_timeout` 秒时发 SIGTERM（优雅退出），等待 `(timeout - soft_timeout)` 秒后若仍未退出则 SIGKILL（硬超时）
  - HTTP 任务：HTTP 客户端不支持优雅中断，设置 `soft_timeout` 时 warn 并忽略
  - `soft_timeout >= timeout` 时 warn 并忽略（soft_timeout 必须严格小于 timeout）
  - `soft_timeout = 0` 等同于 None（不启用软超时）
  - 适用场景：shell 任务需要 cleanup 时间（如刷新缓冲区、写完日志），避免硬 kill 导致数据丢失
- **A13. misfire_grace_time 每作业级（参考 APScheduler `misfire_grace_time` per-job override）**：APScheduler 中每个 job 都可单独设 `misfire_grace_time`（默认 1 秒）覆盖 scheduler 全局值；当 `now - next_run_time > grace_time` 时视为 misfire，按 `coalesce` 规则合并或丢弃。当前 xhjob 全局默认 60s 但无 per-job 字段，关键任务（如订单状态同步）与普通任务（如清理临时文件）共用同一窗口不合理。新增 `TaskBuilder::misfire_grace_time(secs: u64)` / PHP `misfireGraceTime(int $secs): $this`：`Task` 新增 `misfire_grace_time: u64`（默认 0 = 使用全局默认 60s）；`scan_once` 检测 `now > next_fire` 时按 `grace_time` 判定：在窗口内仍触发；超出窗口按 `coalesce=true` 合并为最后一次执行、`coalesce=false` 丢弃全部错过的触发并推进 next_fire。区别于现有"misfire 显式化"（A4 仅补 README 说明），A13 让 per-job 字段真正生效
- **A14. replace_existing 幂等 dispatch（参考 APScheduler `add_job(..., replace_existing=True)`）**：APScheduler 的 `add_job` 接受 `replace_existing` 参数（默认 True），同 id 调用会替换旧 job。当前 xhjob dispatch 时 id 由系统生成 UUID，用户无法自定义；运维场景（如部署脚本注册 cron）需要"同 id 自动覆盖旧任务"避免重复注册。新增两个字段：
  - `TaskBuilder::id(s: impl Into<String>)` / PHP `withId(string $id): $this`：自定义 task id（不调用则系统生成 UUID，向后兼容）
  - `TaskBuilder::replace_existing(bool)` / PHP `replaceExisting(bool $on): $this`：默认 false（id 冲突时返回 error，与当前行为一致）；true 时同 id 先 remove 旧 task 再 insert 新 task（完全替换，state/attempts/execution_count 不保留）
  - 不设置 id 时 `replace_existing` 无效果（系统生成 UUID 必然不冲突）
  - 适用场景：部署脚本重跑幂等、运维脚本批量覆盖 cron 注册
- **A15. tags 作业分组（参考 APScheduler job tags）**：APScheduler 中每个 job 可附加 tags（如 `'monitoring'` / `'reports'`），通过 `get_jobs(tags=...)` 按 tag 过滤。当前 xhjob 无标签概念，运维查询任务列表只能按 state 过滤，无法按业务标签批量查询。新增 `TaskBuilder::tags(&[&str])` / PHP `tags(array $tags): $this`：`Task` 新增 `tags: Vec<String>`（默认空，serde 序列化为 JSON 数组）；`xhjob_list($name, $state_filter=null, $tag=null)` 新增可选 `$tag` 参数过滤 `tags` 数组包含该值的所有任务；SQLite schema 新增 `tags TEXT` 列存 JSON 字符串。适用场景：业务侧给任务打标签（如 `['reports', 'critical']`），运维按标签批量过滤管理
- **C12. rate_limit 每任务限流（参考 Celery `task.rate_limit`）**：Celery 中 `task.rate_limit = '10/m'` 限制任务执行速率为每分钟 10 次，常用于外部 API 调用避免触发 429。当前 xhjob 无速率限制概念，cron 高频任务直接调用外部 API 容易被限流。新增 `TaskBuilder::rate_limit(max_count: u32, window_secs: u64)` / PHP `rateLimit(int $maxCount, int $windowSecs): $this`：`Task` 新增 `rate_limit_count: u32`（默认 0=不限流）+ `rate_limit_window: u64`（默认 0）；`OverlapController::should_fire` 或 scan_once 触发前查询：在 `window_secs` 滑动窗口内已开始执行的实例数 >= `max_count` 时跳过本次触发并推进 next_fire；用 in-memory 滑动窗口计数器（daemon 重启后从 `started_at` 重建）。区别于 `max_instances`（限制并发实例数），rate_limit 限制单位时间内总执行次数（无论是否并发）
- **C13. acks_on_failure 失败时是否确认（参考 Celery `task_acks_on_failure_or_timeout`）**：Celery 中 `task_acks_on_failure_or_timeout = True`（默认）表示即使失败也确认（task 进入终态）；`False` 表示失败不确认，任务会被重新分配给其他 worker（类似 retry 但不消耗 retry_max）。当前 xhjob 失败任务严格按 `retry_max` 上限重试，retry_max 耗尽后终态 Failed，关键任务无法"永不放弃"。新增 `TaskBuilder::acks_on_failure(bool)` / PHP `acksOnFailure(bool $on): $this`：`Task` 新增 `acks_on_failure: bool`（默认 true 保持当前行为）；`process_one` 失败路径：若 `acks_on_failure=false` 则重置 state=Pending + next_fire=now+retry_delay（指数退避若启用），**忽略 retry_max 上限**，任务持续重试直到成功或被 cancel/remove。与 acksLate（C10）互补：acksLate 处理"daemon 崩溃后 Running 任务重排"，acks_on_failure 处理"任务执行失败后是否放弃"
- **C14. worker_max_tasks_per_child daemon 自我回收（参考 Celery `worker_max_tasks_per_child`）**：Celery 中 `worker_max_tasks_per_child = 1000` 让 worker 进程在执行 N 个任务后自动回收重启，防止内存泄漏累积。当前 xhjob daemon 长跑后无回收机制，长时间运行可能因 Rust 代码或 PHP 扩展内部分配累积导致内存增长。新增环境变量 `XHJOB_MAX_TASKS_PER_CHILD=N`（默认 0=不回收，daemon 启动时读取一次）；运行时维护原子计数器 `tasks_executed_since_start`，每次 `process_one` 完成后递增；当达到 N 时 daemon 优雅退出（关闭 scheduler + queue + 等待 in-flight 任务完成 + flush store + exit 0）；由外部进程管理器（systemd / supervisor / docker restart=always）自动重启接续。不引入 PHP API（纯环境变量配置，因 daemon 自我回收是进程级策略而非任务级）
- **A16. timezone per-job（参考 APScheduler `CronTrigger(timezone=...)`）**：APScheduler 的 `CronTrigger` 接受 `timezone` 参数（IANA 标识如 `"America/New_York"`），允许每个 job 独立指定 cron 表达式评估时区；调度器自身有全局 `timezone` 默认值，job 级 timezone 覆盖全局。当前 xhjob 所有 cron 任务用同一全局时区评估，跨国应用（如"纽约时间 9 点日报" vs "北京时间 9 点日报"）必须为每个任务写不同 cron 表达式或手工换算 UTC 偏移，运维极易出错。新增 `TaskBuilder::timezone(tz: impl Into<String>)` / PHP `timezone(string $tz): $this`：`Task` 新增 `timezone: Option<String>` 字段（默认 None = 使用 daemon 全局时区，向后兼容）；`scan_once` 中 cron 表达式评估时使用 `task.timezone` 解析（用 `chrono-tz` crate 提供 IANA 时区数据库），解析失败时 warn 并回退到全局时区；非 cron 任务（interval / runAt）设置 `timezone` 时 warn 并忽略（interval/runAt 用绝对 Unix 时间戳，与时区无关）。适用场景：跨国应用、跨时区 cron 任务、SaaS 多租户场景（不同租户不同时区）
- **A17. Event listener API（参考 APScheduler `add_listener` + `EVENT_JOB_*` 事件流）**：APScheduler 提供 `scheduler.add_listener(callback, EVENT_JOB_EXECUTED | EVENT_JOB_ERROR | EVENT_JOB_MISSED)` 让业务侧订阅任务执行事件，常用于监控告警与运维集成。当前 xhjob 任务执行事件仅写入 `tracing::info!` 日志，运维只能 grep 日志文件，无法用统一 API 查询任务执行事件流（如"过去 1 小时哪些任务失败"），也无法被业务侧程序化订阅。新增：新模块 `src/scheduler/events.rs` 定义 `TaskEvent` struct（`task_id` / `event_type` / `payload` / `ts`）+ 事件类型枚举（`Started` / `Succeeded` / `Failed` / `Missed` / `Cancelled` / `Paused` / `Resumed` / `Expired` / `MaxInstancesReached` / `RateLimited`）；`TaskStore` trait 新增 `record_event(task_id, event_type, payload, ts) -> Result<()>` / `list_events(since_ts: i64, task_id_filter: Option<&str>) -> Result<Vec<TaskEvent>>`；`process_one` / `scan_once` / `cancel_task` / `set_paused` 等关键路径调用 `record_event`；SQLite schema 新增 `events` 表（`id INTEGER PRIMARY KEY AUTOINCREMENT` / `task_id TEXT NOT NULL` / `event_type TEXT NOT NULL` / `payload TEXT` / `ts INTEGER NOT NULL`，索引 `idx_events_ts` on `ts`）；新增 PHP 顶层函数 `xhjob_events($since_ts, $name='default', $task_id=null, $data_dir=null): string` 返回 JSON 数组（PHP 端 `json_decode` 后遍历）；事件 TTL 自动清理（默认 24h，可通过 `XHJOB_EVENTS_TTL_SECS` 环境变量配置，scan_once 周期性调用 `cleanup_expired_events`）。区别于 `xhjob_state`（仅查当前快照）与 `xhjob_list`（仅查任务列表），`xhjob_events` 查询历史事件流（如"过去 1 小时任务 X 失败 3 次"）。适用场景：监控集成（Prometheus / Grafana 拉取事件指标）、运维告警（失败事件触发 webhook）、业务侧轮询任务状态变化
- **A18. coalesce 显式 per-job 行为（参考 APScheduler `coalesce` 参数）**：APScheduler 中 `add_job(..., coalesce=True)` 让 job 在累积多次 missed 触发时合并为最后一次执行（默认 True），`coalesce=False` 则丢弃所有 missed 触发只保留最新调度。当前 xhjob 的 `Task.coalesce: bool` 字段虽已存在但实际行为未真正按字段值控制 misfire 合并/丢弃，仅在 README 文档说明（A4 仅补文档）。新增 `TaskBuilder::coalesce(on: bool)` / PHP `coalesce(bool $on): $this`：让 `task.coalesce` 字段真正生效；`scan_once` 中 misfire 检测时（配合 A13 `misfire_grace_time` per-job 字段）按 `task.coalesce` 字段控制：`coalesce=true`（默认，向后兼容）→ 合并多次 missed 触发为最后一次执行 + 推进 next_fire；`coalesce=false` → 丢弃所有 missed 触发 + 推进 next_fire 至下一轮 cron 时刻（不执行任何补偿执行）。区别于 A4（仅 README 说明），A18 让字段真正生效。适用场景：批量任务用 `coalesce=true` 避免一次性补跑多次（如日报任务 daemon 重启后不需要补跑多次错过的小时触发）；幂等性任务用 `coalesce=false` 避免重复执行（如发送通知任务，重跑会发多次通知）
- **C15. Task chain 顺序流水线（参考 Celery `chain(t1, t2, t3)`）**：Celery 中 `chain(t1.s(), t2.s(), t3.s())` 让任务按顺序执行，前任务的返回值作为后任务的输入，任一任务失败则整个 chain 终态 Failed。当前 xhjob 用户必须手工 dispatch 第 1 个任务 + 轮询 `xhjob_state` 等待完成 + dispatch 第 2 个任务（手工传递 stdout）+ 轮询 + ...，ETL 管道场景极其繁琐。新增 PHP 顶层函数 `xhjob_chain(array $task_configs, $name='default', $data_dir=null): string` 返回 `chain_id`：参数为 task 配置数组（每个元素是 `Xhjob` 对象的 JSON 配置，含 viaShell / viaHttp / cron / delay 等）；daemon 收到 chain dispatch 后依次 dispatch 每个任务，前一个任务 success 后取其 stdout 作为下一个任务的 input payload（通过 stdin 或 env var `XHJOB_CHAIN_INPUT` 传入）；前任务失败则整个 chain 终态 Failed（剩余任务不再执行）；新增 `xhjob_chain_state($chain_id, $name='default', $data_dir=null): string` 返回 JSON（含 `chain_id` / `tasks` 数组（每个元素含 task_id + state）/ `current_step` / `state`：`pending` / `running` / `succeeded` / `failed`）；SQLite schema 新增 `chains` 表（`chain_id TEXT PRIMARY KEY` / `tasks TEXT NOT NULL` JSON 数组 / `current_step INTEGER NOT NULL DEFAULT 0` / `state TEXT NOT NULL DEFAULT 'pending'` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）；daemon 中新增 `chain_step_watcher`：监听当前 step 任务终态后推进 current_step。适用场景：ETL 管道（extract → transform → load）、多步骤业务流程（订单创建 → 库存扣减 → 通知发送）
- **C16. Task group 并行批处理（参考 Celery `group(t1, t2, t3)`）**：Celery 中 `group(t1.s(), t2.s(), t3.s())` 让任务并行执行，等待所有任务完成后返回 `GroupResult`。当前 xhjob 用户必须循环 dispatch N 个任务 + 手工记录所有 task_id + 循环 `xhjob_state` 检查每个任务状态，批量处理场景极其繁琐且无整体视图。新增 PHP 顶层函数 `xhjob_group(array $task_configs, $name='default', $data_dir=null): string` 返回 `group_id`：daemon 收到 group dispatch 后并行 dispatch 所有任务（同时入队，按 priority 排队执行）；新增 `xhjob_group_state($group_id, $name='default', $data_dir=null): string` 返回 JSON（含 `group_id` / `tasks` 数组（每个元素含 task_id + state）/ `completed_count` / `total_count` / `state`：`pending` / `running` / `succeeded`（全部 success）/ `partial_failed`（部分失败）/ `failed`（全部失败））；SQLite schema 新增 `groups` 表（`group_id TEXT PRIMARY KEY` / `tasks TEXT NOT NULL` JSON 数组 / `state TEXT NOT NULL DEFAULT 'pending'` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）；daemon 中新增 `group_state_watcher`：每次任务终态后更新 group 的 completed_count 与 state。适用场景：批量处理（如同时发送 1000 封邮件）、Map-Reduce 模式（map 阶段并行执行多个独立子任务）、并行抓取多个 URL 后聚合
- **C17. worker_max_memory_per_child 基于 内存 的 daemon 自我回收（参考 Celery `worker_max_memory_per_child`）**：Celery 中 `worker_max_memory_per_child = 100000` (KB) 让 worker 进程在内存使用超过阈值后自动回收重启。当前 xhjob 已有 C14 `worker_max_tasks_per_child` 按"任务数"回收，但内存泄漏可能因单个大 payload 任务（如处理 1GB JSON 文件）而非任务数累积，C14 无法覆盖。新增环境变量 `XHJOB_MAX_MEMORY_PER_CHILD=MB`（默认 0=不回收，daemon 启动时读取一次）；运行时每次 `process_one` 完成后读取 `/proc/self/status` 中 `VmRSS` 字段（KB 单位）转换为 MB；当 `vm_rss_mb >= XHJOB_MAX_MEMORY_PER_CHILD` 时 daemon 优雅退出（同 C14 流程）；与 C14 互补：C14 按"任务数"回收防累积，C17 按"内存用量"回收防内存泄漏，两者独立生效，先到达阈值者先触发退出。Linux 平台用 `/proc/self/status`，macOS 平台用 `mach_task_basic_info` API（用 `sysctl` crate 跨平台），Windows 平台用 `GetProcessMemoryInfo` API。适用场景：daemon 处理大 payload 任务导致内存增长（如处理大 JSON / 大文件 / 大数据库查询结果），无法靠"任务数"回收时按"内存"回收
- **A19. or_ 复合触发器（参考 APScheduler `or_` 复合触发器）**：APScheduler 中 `OrTrigger([CronTrigger(...), CronTrigger(...)])` 让单个 job 用多个 trigger 的并集触发，常用于"9 点 + 18 点各跑一次"、"工作日 9 点 + 周末 10 点"等场景。当前 xhjob 必须为每个 cron 表达式 dispatch 一个独立 task，浪费存储且无法共享 state / execution_count / meta。新增 `TaskBuilder::or_cron(exprs: &[&str])` / PHP `orCron(array $exprs): $this`：`Task` 新增 `cron_expressions: Vec<String>` 字段（默认空 Vec，与 `cron` 字段互斥）；`scan_once` 中：若 `cron_expressions` 非空，对每个表达式独立计算 next_fire，取最小值作为本任务 next_fire；触发后推进所有匹配的表达式（其他未匹配的表达式保持原 next_fire）；与 `cron` 字段同时设置时 `cron` 优先（向后兼容），`or_cron` 被忽略并 warn；与 `interval` / `runAt` 互斥；非法表达式在 `build()` 时即返回 `Err(XhjobError::CronParse(...))`（与 A6 一致）；SQLite schema 新增 `cron_expressions TEXT` 列存 JSON 数组（旧库 ALTER TABLE 兼容）；`task_from_row` / `insert_task` 同步读写；StateInfo 新增 `cron_expressions` 字段。区别于 A4 misfire（容错窗口）与 A13 misfire_grace_time（per-job），A19 是触发时间合并而非触发容错。适用场景：日报任务"9 点 + 18 点各跑一次"、运维任务"工作日 9 点 + 周末 10 点"、清理任务"凌晨 3 点 + 中午 12 点各跑一次"
- **A20. calendar 触发器 + holiday skipping（参考 APScheduler `CalendarTrigger` + holiday skipping）**：APScheduler 中可通过 `CalendarTrigger` 或在 cron 表达式中排除特定日期实现"工作日报告"或"节假日跳过"。当前 xhjob 必须为每个节假日写 cron 排除规则或 cron 表达式 hack（如 `0 9 * * 1-5` 排除周末但无法排除春节）。新增 `TaskBuilder::skip_dates(dates: &[&str])` / PHP `skipDates(array $dates): $this` + `TaskBuilder::workdays_only(on: bool)` / PHP `workdaysOnly(bool $on): $this`：`Task` 新增 `skip_dates: Vec<String>`（YYYY-MM-DD 格式日期列表，默认空 Vec）+ `workdays_only: bool`（默认 false）字段；`scan_once` 中 cron / interval 触发前检查 `now` 的本地日期（用 daemon 全局时区）：若日期在 `skip_dates` 列表中 或 `workdays_only=true` 且是周末（周六/周日，用 `chrono::Weekday::Sat` / `Sun` 判定），则跳过本次触发并推进 next_fire 至下一轮 cron 时刻；不引入第三方日历库（避免依赖，节假日数据需用户手动维护，如每年初配置春节/国庆 7 天为 `skip_dates`）；与 cron / interval 配合生效，与 runAt 一次性任务互斥（runAt 是绝对时刻不跳过）；`skip_dates` 与 `workdays_only` 可同时设置（取并集跳过）；SQLite schema 新增 `skip_dates TEXT` 列存 JSON 数组 + `workdays_only INTEGER NOT NULL DEFAULT 0` 列（旧库 ALTER TABLE 兼容）；StateInfo 新增 `skip_dates` / `workdays_only` 字段。适用场景：工作日报告跳过周末、节假日跳过（如春节/国庆手动配置 skip_dates）、特殊维护日跳过（如系统升级日 skip_dates=['2026-08-15']）
- **A21. modify_job 任意字段在线修改（参考 APScheduler `modify_job(job_id, changes)`）**：APScheduler 中 `modify_job(job_id, changes_dict)` 允许在线修改 job 的任意字段（next_run_time / args / kwargs / misfire_grace_time / coalesce / max_instances / tags 等）而不丢失 job 历史。当前 xhjob 只有 `xhjob_reschedule`（仅改 cron 字段），改 priority / retry_max / timeout / max_instances / tags 等必须删旧 dispatch 新，丢失 state / attempts / execution_count / meta。新增 `xhjob_modify($id, array $changes)` / `Xhjob::modify($id, array $changes): bool`：`TaskStore` trait 新增 `modify_task(id, changes: &serde_json::Value) -> Result<bool>` 方法；允许在线更新任意 task 配置字段（priority / retry_max / retry_delay / timeout / soft_timeout / max_instances / coalesce / max_executions / start_date / end_date / result_ttl / meta / ignore_result / acks_late / tags / rate_limit_count / rate_limit_window / acks_on_failure / timezone / misfire_grace_time / expires / skip_dates / workdays_only / cron_expressions / interval / run_at 等），但**不可更新** state / attempts / execution_count / next_fire / id / created_at / started_at / finished_at / last_error / paused / cancel_requested（这些是运行时状态字段，应由系统管理）/ `cron` 字段（用 `xhjob_reschedule` 专门处理 cron 变更并保留 execution_count）；更新成功后若涉及调度字段（interval / run_at / start_date / end_date / max_executions / tags / timezone / skip_dates / workdays_only / cron_expressions / misfire_grace_time / coalesce）则重新计算 next_fire；对终态任务（Success / Failed / Cancelled / Expired）调用返回 false（需先 `xhjob_requeue` 重置为 Pending）；对 Running 任务仅更新非调度字段（如 priority / timeout / tags）允许，调度字段（如 cron / interval / run_at）更新返回 false 并 warn "cannot modify schedule fields while running"；区别于 `xhjob_reschedule`（仅改 cron）与 `xhjob_requeue`（重置终态任务），`xhjob_modify` 是通用字段更新。SQLite 实现用动态 SQL `UPDATE tasks SET <field>=?, <field>=? ... WHERE id=?`（白名单字段过滤防 SQL 注入）；失败原因：id 不存在（返回 false）/ 字段非法（返回 false + 错误信息）/ 值类型不匹配（返回 false + 错误信息）/ 字段不允许更新（如 state，返回 false + 错误信息）。适用场景：运维发现任务 timeout 过短在线调整（无需重新 dispatch 保留 execution_count）、调整 priority 让重要任务优先（动态调度）、更新 tags 重新分组（业务变更）、调整 max_instances 控制并发（负载变化）
- **C18. chord（header group + callback，参考 Celery `chord(header)(callback)`）**：Celery 中 `chord(header.s(), body.s())` 让 header group 并行执行，所有 header 完成后将 results 数组传给 callback 执行，是 Map-Reduce 模式核心：map = header group 并行处理分片，reduce = callback 聚合所有 results。当前 xhjob 已有 C15 chain（顺序执行）与 C16 group（并行无 callback），但缺 chord（并行 header + 顺序 callback）。新增 PHP 顶层函数 `xhjob_chord(array $header_tasks, $callback_config, $name='default', $data_dir=null): string` 返回 `chord_id`：daemon 收到 chord dispatch 后并行 dispatch 所有 header tasks（同时入队，按 priority 排队执行，受 max_instances / rate_limit / worker_concurrency 限制）；header task 完成后：completed_count += 1，记录每个 task 的 result（stdout）到 chords 表的 results 数组（按完成顺序而非 dispatch 顺序）；全部 header tasks 完成后：将所有 results 作为 callback 的 input payload（JSON 数组 `[{task_id, state, stdout, exit_code}, ...]`，通过 `XHJOB_CHORD_INPUT` 环境变量传入）→ dispatch callback → update_chord_state("running")；callback 完成后：update_chord_state("succeeded") 或 "failed"；header 部分失败时：state 变为 `partial_failed`，但 callback 仍触发（callback 接收的 results 包含失败任务的 error 信息，callback 自行决定是否处理）；新增 `xhjob_chord_state($chord_id, $name='default', $data_dir=null): string` 返回 JSON（含 `chord_id` / `header_tasks` 数组（每个元素含 task_id + state + result）/ `callback` 对象（含 task_id + state + result）/ `state`：`pending` / `running` / `succeeded` / `failed` / `partial_failed` / `completed_count` / `total_count`）；SQLite schema 新增 `chords` 表（`chord_id TEXT PRIMARY KEY` / `header_tasks TEXT NOT NULL` JSON 数组（含 task_id 与原始 config）/ `callback_config TEXT NOT NULL` JSON / `state TEXT NOT NULL DEFAULT 'pending'` / `results TEXT` JSON 数组（按完成顺序存 header results，每个元素含 task_id / state / stdout / exit_code）/ `completed_count INTEGER NOT NULL DEFAULT 0` / `total_count INTEGER NOT NULL` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）；daemon 重启时从 chords 表恢复未完成 chord（state=pending 或 running），继续等待 header task 完成。区别于 C15 chain（顺序执行）与 C16 group（仅并行无 callback），chord 是"并行 header + 顺序 callback"模式。适用场景：Map-Reduce 聚合（map 100 个分片计算 → reduce 求和）、批量抓取 + 汇总（抓 100 个 URL → 汇总去重）、分布式计算（并行 N 个独立计算 → 合并结果）
- **C19. chunks（大列表切分并行批处理，参考 Celery `chunks(task, items, chunk_size)`）**：Celery 中 `task.chunks(items, chunk_size).group()` 自动将大列表切分为 N 个等大 chunk 并行执行，简化批量处理代码。当前 xhjob 必须手工切分大列表为 N 个子列表 + 循环 dispatch N 个 task + 手工记录所有 task_id，无法整体管理。新增 PHP 顶层函数 `xhjob_chunks(array $task_template, array $items, int $chunk_size, $name='default', $data_dir=null): string` 返回 `chunks_id`：daemon 收到 chunks dispatch 后按 `chunk_size` 将 `items` 切分为 N 块（最后一块可能不足 chunk_size），每块作为一个独立 task dispatch（task payload 为 JSON `{chunk_index: N, items: [...], total_chunks: M, chunks_id: "..."}`，通过 `XHJOB_CHUNK_INPUT` 环境变量传入）；并行 dispatch 所有 chunk tasks（同时入队，按 priority 排队执行，受 max_instances / rate_limit / worker_concurrency 限制）；每个 chunk task 完成后：completed_count += 1，记录每个 chunk 的 result（stdout）到 chunks 表；全部 chunk tasks 完成后：update_chunks_state("succeeded")；新增 `xhjob_chunks_state($chunks_id, $name='default', $data_dir=null): string` 返回 JSON（含 `chunks_id` / `total_chunks` / `completed_count` / `state`：`pending` / `running` / `succeeded` / `partial_failed` / `failed` / `items_total` / `task_template`）；SQLite schema 新增 `chunks` 表（`chunks_id TEXT PRIMARY KEY` / `task_template TEXT NOT NULL` JSON / `items TEXT NOT NULL` JSON 数组 / `chunk_size INTEGER NOT NULL` / `total_chunks INTEGER NOT NULL` / `state TEXT NOT NULL DEFAULT 'pending'` / `completed_count INTEGER NOT NULL DEFAULT 0` / `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL`）；daemon 重启时从 chunks 表恢复未完成 chunks（state=pending 或 running），继续等待 chunk task 完成。区别于 C16 group（用户自己组织 task_configs 数组），chunks 自动按 chunk_size 切分大列表为 N 个等大 task，简化批量处理代码。适用场景：批量发送 10000 封邮件（chunk_size=100 → 100 个 task 各发 100 封）、批量抓取 1000 个 URL（chunk_size=50 → 20 个 task 各抓 50 个）、批量更新 10000 条数据库记录（chunk_size=500 → 20 个 task 各更新 500 条）
- **C20. worker_concurrency daemon 全局并发上限（参考 Celery `worker_concurrency`）**：Celery 中 `worker_concurrency = N` 限制 worker 进程同时执行的任务数为 N（默认 CPU 核心数），防止突发流量撑爆系统资源。当前 xhjob 已有 per-task `maxInstances`（A10，限制同一 task_id 的并发实例数），但无 daemon 全局并发限制——N 个不同 task 可同时 Running 撑爆 CPU / 内存 / 数据库连接池。新增环境变量 `XHJOB_WORKER_CONCURRENCY=N`（默认 0=不限制，daemon 启动时读取一次）；daemon 维护全局原子计数器 `current_running_count: AtomicU32`；`process_one` 取任务前检查 `if worker_concurrency > 0 && current_running_count >= worker_concurrency`：等待（短时间 sleep 100ms 或 skip 本次取任务循环，让出 CPU 给已 Running 任务）；任务开始时 `current_running_count.fetch_add(1, Ordering::Relaxed)`，任务完成时 `fetch_sub(1, Ordering::Relaxed)`；区别于 per-task `maxInstances`（限制同一 task_id 的并发实例数），`worker_concurrency` 是 daemon 全局限制（所有任务合计至多 N 个并发实例），两者独立生效；与 `maxInstances` 配合：如 `XHJOB_WORKER_CONCURRENCY=10` + 任务 A `maxInstances(3)` + 任务 B `maxInstances(5)` → 任务 A 至多 3 个并发、任务 B 至多 5 个并发、A+B 合计至多 10 个并发；与 C14 `max_tasks_per_child` / C17 `max_memory_per_child` 互补：C14/C17 是 daemon 自我回收（执行 N 个任务或内存超阈值后退出重启），C20 是 daemon 运行时并发限制（不退出，仅等待）。适用场景：daemon 资源有限（CPU 4 核 / 内存 4GB），限制总并发避免 OOM、配合 systemd CPUQuota 限制、防止突发流量撑爆数据库连接池

#### 不对齐的成熟框架能力（明确排除，避免过度设计）

- ❌ 分布式 worker / broker（Celery Redis/RabbitMQ 依赖）——超出单机目标
- ❌ beat 调度器（Celery beat）——cron 调度已实现，不重复
- ❌ APScheduler data store 抽象（SQLAlchemy/MongoDB/Redis）——只保留 SQLite
- ❌ 任务事件流 events（Celery events 远程推送）——`Event` dead code 已保留为未来接口，A17 仅本地 SQLite 查询不推送
- ❌ Celery canvas 复杂组合（如 starmap / send_task / Signature 远程调用）——单机版不适用

### 文档与示例

- README "API 参考" 表新增 `maxExecutions` / `startAt` / `endAt` / `resultTtl` / `withMeta` / `every` / `runAt` / `jitter` / `expires` / `retryBackoff` / `xhjob_remove` / `xhjob_pause` / `xhjob_resume` / `xhjob_cancel` / `xhjob_list` / `xhjob_requeue` 行
- README "API 参考" 表新增第四轮 `misfireGraceTime(int $secs)` / `withId(string $id)` / `replaceExisting(bool $on)` / `tags(array $tags)` / `rateLimit(int $maxCount, int $windowSecs)` / `acksOnFailure(bool $on)` 行
- README "API 参考" 表新增第五轮 `timezone(string $tz)` / `coalesce(bool $on)` 行 + 顶层函数 `xhjob_events` / `xhjob_chain` / `xhjob_chain_state` / `xhjob_group` / `xhjob_group_state` 行
- README "环境变量" 表 `XHJOB_PERSIST` 行明确"仅在 daemon 启动时读取一次，运行中修改需 restart 生效"
- README "环境变量" 表新增 `XHJOB_MAX_TASKS_PER_CHILD` 行说明"daemon 执行 N 个任务后自我回收，由外部进程管理器重启"
- README "环境变量" 表新增第五轮 `XHJOB_MAX_MEMORY_PER_CHILD` 行说明"daemon 内存超过 N MB 后自我回收，与 max_tasks_per_child 互补"
- README "环境变量" 表新增第五轮 `XHJOB_EVENTS_TTL_SECS` 行说明"events 表 TTL 自动清理，默认 86400（24h）"
- README "Cron 自定义时区" 一节明确 5/6 段表达式格式（6 段含秒）
- README 新增"Cron 执行次数限制"小节
- README 新增"任务暂停/恢复/取消"小节
- README 新增"任务起始/结束时间"小节
- README 新增"任务列表查询"小节
- README 新增"misfire 处理"小节
- README 新增"任务结果过期清理"小节
- README 新增"任务元数据"小节
- README 新增"Interval 周期触发（every）"小节（A7 对齐说明）
- README 新增"DateTrigger 一次性触发（runAt）"小节（A8 对齐说明）
- README 新增"Jitter 随机抖动"小节（A9 对齐说明）
- README 新增"任务级过期（expires）"小节（C6 对齐说明）
- README 新增"任务重新入队（requeue）"小节（C7 对齐说明）
- README 新增"Retry 指数退避（retryBackoff）"小节（C8 对齐说明）
- README 新增"对照 APScheduler / Celery 的功能对齐"小节扩展为 29 项（A1-A15 + C1-C14）
- README 新增"max_instances 并发实例数（A10）"小节
- README 新增"reschedule 在线修改 cron（A11）"小节
- README 新增"xhjob_get 单任务详情查询（A12）"小节
- README 新增"ignoreResult fire-and-forget（C9）"小节
- README 新增"acksLate 崩溃恢复（C10）"小节
- README 新增"softTimeout 软超时（C11）"小节
- README 新增"misfire_grace_time 每作业级（A13）"小节：说明 per-job 字段 vs 全局默认、与 coalesce 配合规则
- README 新增"replace_existing 幂等 dispatch（A14）"小节：说明 withId / replaceExisting 用法 + 部署脚本幂等场景
- README 新增"tags 作业分组（A15）"小节：说明 tags 用法 + xhjob_list 按 tag 过滤
- README 新增"rateLimit 每任务限流（C12）"小节：说明滑动窗口算法 + 与 maxInstances 区别
- README 新增"acksOnFailure 失败不放弃（C13）"小节：说明与 acksLate 互补 + 关键任务永不放弃场景
- README 新增"worker_max_tasks_per_child daemon 自我回收（C14）"小节：说明环境变量配置 + 与 systemd 配合
- `examples/cron_http.php` 增加 maxExecutions 示例 + dispatch 后提示 daemon 持续触发需手动 stop
- 新增 `examples/cron_lifecycle.php`：演示 pause/resume/cancel/remove/list API 完整用法
- 新增 `examples/cron_interval.php`：演示 every() 周期触发
- 新增 `examples/cron_runAt.php`：演示 runAt() 一次性触发
- 新增 `examples/cron_jitter.php`：演示 jitter() 多任务分散触发
- 新增 `examples/cron_maxInstances.php`：演示 maxInstances(2) 并发执行（A10）
- 新增 `examples/cron_reschedule.php`：演示 xhjob_reschedule 在线修改 cron（A11）
- 新增 `examples/cron_replace_existing.php`：演示 withId + replaceExisting 部署脚本幂等注册（A14）
- 新增 `examples/cron_tags.php`：演示 tags 标记 + xhjob_list 按 tag 过滤（A15）
- 新增 `examples/cron_rateLimit.php`：演示 rateLimit 限流外部 API 调用（C12）
- 新增 `examples/cron_timezone.php`：演示 timezone per-job 跨时区 cron 任务（A16）
- 新增 `examples/cron_events.php`：演示 xhjob_events 任务执行事件流查询（A17）
- 新增 `examples/cron_coalesce.php`：演示 coalesce 显式 per-job 行为（A18）
- 新增 `examples/cron_chain.php`：演示 xhjob_chain ETL 顺序流水线（C15）
- 新增 `examples/cron_group.php`：演示 xhjob_group 并行批处理（C16）

### 测试

- 新增 `tests/boundary_cases.php`：invalid JSON / invalid service name / daemon not running / invalid cron 四条失败路径
- 新增 `tests/max_executions.php`：cron 每 1s 触发 + maxExecutions(3)，验证执行 3 次后 state=SUCCESS 且不再触发（**修复 bug 后预期 PASS**）
- 新增 `tests/lifecycle_api.php`：pause/resume/cancel/remove/list 五个新 API 端到端验证
- 新增 `tests/start_end_date.php`：startAt/endAt 时间窗口验证
- 新增 `tests/result_ttl.php`：resultTtl 后 result 被清理但 task 保留
- 新增 `tests/meta_field.php`：withMeta 写入 + xhjob_state 读取
- 新增 `tests/interval_trigger.php`：every(2) 周期触发验证（A7）
- 新增 `tests/run_at_trigger.php`：runAt(time()+3) 一次性触发验证（A8）
- 新增 `tests/jitter_test.php`：jitter(5) 多任务触发时刻分散验证（A9）
- 新增 `tests/expires_test.php`：expires(2) 后未执行任务被置 Expired 验证（C6）
- 新增 `tests/requeue_test.php`：requeue Cancelled/Failed/Expired 任务验证（C7）
- 新增 `tests/retry_backoff.php`：retryBackoff(true) 指数退避时间序列验证（C8）
- 新增 `tests/max_instances_test.php`：maxInstances(2) 允许 2 个并发实例 + 第 3 个被跳过验证（A10）
- 新增 `tests/reschedule_test.php`：reschedule 修改 cron + 保留 execution_count 验证（A11）
- 新增 `tests/get_job_test.php`：xhjob_get 返回完整 Task JSON 字段验证（A12）
- 新增 `tests/ignore_result_test.php`：ignoreResult(true) 不存 result + state 仍正常验证（C9）
- 新增 `tests/acks_late_test.php`：模拟 daemon 崩溃重启后 acks_late 任务被重排验证（C10）
- 新增 `tests/soft_timeout_test.php`：softTimeout(5)+timeout(10) shell 任务 SIGTERM 优雅退出验证（C11）
- 新增 `tests/misfire_grace_time_test.php`：misfireGraceTime(5) 与全局默认对比 + coalesce 配合验证（A13）
- 新增 `tests/replace_existing_test.php`：withId + replaceExisting(true) 同 id 重跑覆盖验证（A14）
- 新增 `tests/tags_test.php`：tags 标记 + xhjob_list 按 tag 过滤验证（A15）
- 新增 `tests/rate_limit_test.php`：rateLimit(3, 10) 在 10s 窗口内最多 3 次触发验证（C12）
- 新增 `tests/acks_on_failure_test.php`：acksOnFailure(false) 失败任务永不放弃持续重试验证（C13）
- 新增 `tests/max_tasks_per_child_test.php`：XHJOB_MAX_TASKS_PER_CHILD=5 daemon 执行 5 个任务后优雅退出验证（C14）
- 新增 `tests/timezone_test.php`：timezone('America/New_York') cron 任务按纽约时区评估验证（A16）
- 新增 `tests/events_test.php`：xhjob_events 查询任务执行事件流 + TTL 清理验证（A17）
- 新增 `tests/coalesce_test.php`：coalesce(true) 合并 missed / coalesce(false) 丢弃 missed 行为验证（A18）
- 新增 `tests/chain_test.php`：xhjob_chain 顺序流水线 + 前任务输出作为后任务输入 + 失败中断验证（C15）
- 新增 `tests/group_test.php`：xhjob_group 并行批处理 + group_state 完成率验证（C16）
- 新增 `tests/max_memory_per_child_test.php`：XHJOB_MAX_MEMORY_PER_CHILD=100 daemon 内存超阈值后优雅退出验证（C17）
- 既有测试全部不回归

### 编译 + 提交

- 重新编译（两种 feature 零警告）
- 全量测试通过（含新增 24 个对齐测试文件：6 项 round 2 + 6 项 round 3 + 6 项 round 4 + 6 项 round 5）
- 提交并 push 到远程主分支

## Impact

- Affected specs:
  - `audit-and-polish-from-user-perspective`（上一轮产物，本轮在其基础上做深度功能完善，不冲突）
  - `implement-async-task-scheduler`（初始实现，本轮启用其预留接口、新增字段、对齐 APScheduler/Celery）
- Affected code:
  - `src/task/mod.rs`（TaskBuilder 新增 `max_executions` / `start_date` / `end_date` / `result_ttl` / `meta` / `every` / `run_at` / `jitter` / `expires` / `retry_backoff` / `ignore_result` / `acks_late` / `soft_timeout` / `misfire_grace_time` / `id` / `replace_existing` / `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` / `timezone` / `coalesce` builder 方法；build 中 cron 解析失败返回 Err；build 中校验 `soft_timeout < timeout`；build 中支持自定义 id 或 UUID 生成）
  - `src/store/mod.rs`（Task 新增 `max_executions` / `execution_count` / `paused` / `start_date` / `end_date` / `result_ttl` / `meta` / `interval` / `run_at` / `jitter` / `expires` / `retry_backoff` / `ignore_result` / `acks_late` / `soft_timeout` / `misfire_grace_time` / `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` / `timezone` 字段；TaskState 新增 `Cancelled` / `Expired`；TaskStore trait 新增 `increment_execution_count` / `remove_task` / `set_paused` / `cancel_task` / `requeue_task` / `cleanup_expired_results` / `list_tasks`（含可选 tag 过滤）/ `reschedule_task` / `reset_running_to_pending`（acks_late 启动恢复用）/ `record_event` / `list_events` / `cleanup_expired_events`（A17 events 流）/ `create_chain` / `get_chain` / `update_chain_step` / `list_chain_tasks`（C15 chain）/ `create_group` / `get_group` / `update_group_state` / `list_group_tasks`（C16 group）；保留 make_store 加注释）
  - `src/store/sqlite.rs`（schema 新增 25 列 + 旧库 ALTER TABLE 兼容；实现新 trait 方法；新增 `ignore_result` / `acks_late` / `soft_timeout` / `misfire_grace_time` / `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` / `timezone` 九列；list_tasks 支持 tag JSON LIKE 过滤；**新增 `events` 表（A17）**：`id` / `task_id` / `event_type` / `payload` / `ts` + 索引；**新增 `chains` 表（C15）**：`chain_id` / `tasks` JSON / `current_step` / `state` / `created_at` / `updated_at`；**新增 `groups` 表（C16）**：`group_id` / `tasks` JSON / `state` / `created_at` / `updated_at`）
  - `src/store/in_memory.rs`（无需 schema 改动，字段自动同步；实现新 trait 方法，包括 `reschedule_task` / `reset_running_to_pending` / `list_tasks` tag 过滤 / events / chains / groups 方法）
  - `src/scheduler/cron.rs`（next_fire 删 seconds 参数；scan_once 中检查 max_executions / paused / start_date / end_date / expires / jitter / interval / run_at；优先级 runAt > cron > interval；**新增 misfire_grace_time 检测**：`now > next_fire` 时按 grace_time + coalesce 规则处理；**新增 timezone 解析（A16）**：cron 表达式评估时使用 `task.timezone` 解析（用 `chrono-tz` crate），失败回退全局；**新增 coalesce 真正生效（A18）**：按 `task.coalesce` 字段控制合并/丢弃）
  - `src/scheduler/queue.rs`（**修复 maxExecutions bug**：cron 任务 success 后不立即 Success 终态，仅当 execution_count >= max_executions 才 Success；按 priority 排序；终态后触发 result_ttl 清理；**新增 ignore_result 跳过 save_result**；**新增 soft_timeout SIGTERM 优雅退出逻辑**；**新增 acks_on_failure=false 失败不终态逻辑**：失败时若 `acks_on_failure=false` 则重置为 Pending + next_fire=now+retry_delay，忽略 retry_max；**新增 events 写入**：process_one 各路径调用 `record_event`）
  - `src/scheduler/overlap.rs`（**升级 should_fire**：检查 `count_running_instances(id) < max_instances`（max_instances > 0 时）；allow_overlap=true 单独设置时保持无限并发（向后兼容）；**新增 rate_limit 检测**：触发前查询 sliding window 内已开始执行实例数；保留 should_fire_missed 加注释；**新增 rate_limited 事件写入**）
  - `src/scheduler/events.rs`（**新增模块 A17**：`TaskEvent` struct + `EventType` 枚举 + `EventRecorder` 工具结构封装 record_event 逻辑）
  - `src/scheduler/chain.rs`（**新增模块 C15**：`ChainExecutor` 负责推进 chain 的 current_step / 监听当前 step 任务终态 / 触发下一步任务 dispatch / 处理 chain state 转换）
  - `src/scheduler/group.rs`（**新增模块 C16**：`GroupWatcher` 负责监听 group 内任务终态 / 更新 completed_count / 计算 group state（succeeded / partial_failed / failed））
  - `src/retry/mod.rs`（should_retry 真正按错误类型判断；删 sleep_for_retry；新增 exponential backoff 计算逻辑；**acks_on_failure=false 时跳过 retry_max 上限检查**）
  - `src/ipc/mod.rs`（保留 Event 加注释；新增 IPC 命令 `remove` / `pause` / `resume` / `cancel` / `list`（含 tag 参数）/ `requeue` / `reschedule` / `get` / `events` / `chain` / `chain_state` / `group` / `group_state`）
  - `src/pool/mod.rs` + `src/pool/thread_pool.rs`（保留加注释）
  - `src/lib.rs`（Xhjob 类新增 `maxExecutions` / `startAt` / `endAt` / `resultTtl` / `withMeta` / `every` / `runAt` / `jitter` / `expires` / `retryBackoff` / `ignoreResult` / `acksLate` / `softTimeout` / `misfireGraceTime` / `withId` / `replaceExisting` / `tags` / `rateLimit` / `acksOnFailure` / `timezone` / `coalesce`；新增顶层函数 `xhjob_remove` / `xhjob_pause` / `xhjob_resume` / `xhjob_cancel` / `xhjob_list`（含可选 tag）/ `xhjob_requeue` / `xhjob_reschedule` / `xhjob_get` / `xhjob_events` / `xhjob_chain` / `xhjob_chain_state` / `xhjob_group` / `xhjob_group_state`；`xhjob_state` 返回新增字段；dispatch 处理 id 冲突 / replace_existing）
  - `src/daemon_main.rs`（dispatch handler 透传新字段；新增 remove/pause/resume/cancel/list/requeue/reschedule/get/events/chain/chain_state/group/group_state handler；**新增 daemon 启动时 acks_late 重排 Running 任务逻辑**；**新增 tasks_executed_since_start 原子计数器 + max_tasks_per_child 检查 + 优雅退出逻辑**；**新增 max_memory_per_child 内存监控 + 优雅退出逻辑（C17）**；**新增 ChainExecutor 与 GroupWatcher 初始化与运行**）
  - `src/outcome/mod.rs`（StateInfo 新增 `execution_count` / `max_executions` / `next_fire` / `paused` / `start_date` / `end_date` / `meta` / `interval` / `run_at` / `jitter` / `expires` / `retry_backoff` / `ignore_result` / `acks_late` / `soft_timeout` / `misfire_grace_time` / `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` / `timezone` 字段）
  - `Cargo.toml`（新增依赖：`chrono-tz` 用于 A16 timezone 解析）
  - `README.md`（新增 API 行 + 多处说明 + 多个新小节，含 A7-A9 + C6-C8 + A10-A12 + C9-C11 + A13-A15 + C12-C14 + A16-A18 + C15-C17 共 25 项新对齐；环境变量表新增 `XHJOB_MAX_TASKS_PER_CHILD` / `XHJOB_MAX_MEMORY_PER_CHILD` / `XHJOB_EVENTS_TTL_SECS`）
  - `examples/cron_http.php`（增加 maxExecutions 示例 + 退出提示）
  - `examples/cron_lifecycle.php`（新增）
  - `examples/cron_interval.php`（新增，演示 every() 用法）
  - `examples/cron_runAt.php`（新增，演示 runAt() 一次性触发）
  - `examples/cron_jitter.php`（新增，演示 jitter() 多任务分散触发）
  - `examples/cron_maxInstances.php`（新增，演示 maxInstances(2) 并发执行 A10）
  - `examples/cron_reschedule.php`（新增，演示 xhjob_reschedule 在线修改 cron A11）
  - `examples/cron_replace_existing.php`（新增，演示 withId + replaceExisting 部署脚本幂等 A14）
  - `examples/cron_tags.php`（新增，演示 tags 标记 + 按 tag 过滤 A15）
  - `examples/cron_rateLimit.php`（新增，演示 rateLimit 限流外部 API 调用 C12）
  - `examples/cron_timezone.php`（新增，演示 timezone per-job 跨时区 cron 任务 A16）
  - `examples/cron_events.php`（新增，演示 xhjob_events 任务执行事件流查询 A17）
  - `examples/cron_coalesce.php`（新增，演示 coalesce 显式 per-job 行为 A18）
  - `examples/cron_chain.php`（新增，演示 xhjob_chain ETL 顺序流水线 C15）
  - `examples/cron_group.php`（新增，演示 xhjob_group 并行批处理 C16）
  - `tests/boundary_cases.php`（新增）
  - `tests/max_executions.php`（新增，**修复后重跑通过**）
  - `tests/lifecycle_api.php`（新增）
  - `tests/start_end_date.php`（新增）
  - `tests/result_ttl.php`（新增）
  - `tests/meta_field.php`（新增）
  - `tests/interval_trigger.php`（新增，验证 every() 周期触发）
  - `tests/run_at_trigger.php`（新增，验证 runAt() 一次性触发）
  - `tests/jitter_test.php`（新增，验证 jitter() 分散触发）
  - `tests/expires_test.php`（新增，验证 expires() 超时丢弃）
  - `tests/requeue_test.php`（新增，验证 requeue() 重置终态）
  - `tests/retry_backoff.php`（新增，验证 retryBackoff(true) 指数退避）
  - `tests/max_instances_test.php`（新增，验证 maxInstances(2) 并发 A10）
  - `tests/reschedule_test.php`（新增，验证 reschedule 修改 cron A11）
  - `tests/get_job_test.php`（新增，验证 xhjob_get 完整字段 A12）
  - `tests/ignore_result_test.php`（新增，验证 ignoreResult(true) 不存 result C9）
  - `tests/acks_late_test.php`（新增，验证 daemon 重启后 acks_late 重排 C10）
  - `tests/soft_timeout_test.php`（新增，验证 softTimeout SIGTERM 优雅退出 C11）
  - `tests/misfire_grace_time_test.php`（新增，验证 misfireGraceTime per-job 与全局默认对比 A13）
  - `tests/replace_existing_test.php`（新增，验证 withId + replaceExisting 同 id 重跑覆盖 A14）
  - `tests/tags_test.php`（新增，验证 tags 标记 + xhjob_list 按 tag 过滤 A15）
  - `tests/rate_limit_test.php`（新增，验证 rateLimit(3, 10) 滑动窗口限流 C12）
  - `tests/acks_on_failure_test.php`（新增，验证 acksOnFailure(false) 失败不放弃 C13）
  - `tests/max_tasks_per_child_test.php`（新增，验证 XHJOB_MAX_TASKS_PER_CHILD daemon 自我回收 C14）
  - `tests/timezone_test.php`（新增，验证 timezone('America/New_York') per-job 时区 cron 评估 A16）
  - `tests/events_test.php`（新增，验证 xhjob_events 查询任务执行事件流 + TTL 清理 A17）
  - `tests/coalesce_test.php`（新增，验证 coalesce(true) 合并 missed / coalesce(false) 丢弃 missed 行为 A18）
  - `tests/chain_test.php`（新增，验证 xhjob_chain 顺序流水线 + 前任务输出作为后任务输入 + 失败中断 C15）
  - `tests/group_test.php`（新增，验证 xhjob_group 并行批处理 + group_state 完成率 C16）
  - `tests/max_memory_per_child_test.php`（新增，验证 XHJOB_MAX_MEMORY_PER_CHILD daemon 内存超阈值后退出 C17）
- **BREAKING**: 无（新字段默认值保证向后兼容：max_executions=0 / paused=false / start_date/end_date=None / result_ttl=0 / meta=None / interval=None / run_at=None / jitter=0 / expires=0 / retry_backoff=false / ignore_result=false / acks_late=false / soft_timeout=None / misfire_grace_time=0（用全局默认）/ id=None（系统生成 UUID）/ replace_existing=false / tags=空数组 / rate_limit_count=0 / rate_limit_window=0 / acks_on_failure=true / timezone=None（用全局时区）/ coalesce=true（保持当前默认行为）；旧 task JSON 反序列化保持工作；SQLite 旧库通过 ALTER TABLE 自动迁移；新增 events / chains / groups 表为新建表，不影响旧库；max_instances=1 默认值保持当前"不并发"行为；allow_overlap=true 单独设置保持无限并发；XHJOB_MAX_TASKS_PER_CHILD 默认 0 不回收；XHJOB_MAX_MEMORY_PER_CHILD 默认 0 不回收；XHJOB_EVENTS_TTL_SECS 默认 86400 不影响默认清理行为）

## ADDED Requirements

### Requirement: Cron 执行次数限制
The system SHALL 支持为 cron 任务指定最大执行次数，到达上限后任务自动终止。

#### Scenario: 设置 maxExecutions(3) 后执行 3 次停止
- **WHEN** 用户 `Xhjob::task()->cron('* * * * * *')->maxExecutions(3)->viaShell('echo hi')->dispatch()`
- **THEN** cron 触发 3 次后任务 state 变为 `SUCCESS`，`execution_count=3`
- **AND** 第 4 次 cron tick 不再触发该任务
- **AND** 持久化场景下 restart 后 execution_count 保留

#### Scenario: 不设置 maxExecutions 时无限触发
- **WHEN** 用户 `Xhjob::task()->cron('* * * * *')->viaShell('echo hi')->dispatch()`（未调用 maxExecutions）
- **THEN** `max_executions=0`，cron 持续触发直至 daemon 停止
- **AND** 行为与当前一致（向后兼容）

#### Scenario: maxExecutions(0) 显式表示无限
- **WHEN** 用户 `maxExecutions(0)`
- **THEN** 等同于未设置，cron 持续触发

### Requirement: Retry 按错误类型真实判断
The system SHALL 仅对可重试错误重试，避免对永久性错误（如 HTTP 4xx）无意义重试。

#### Scenario: HTTP 500 重试
- **WHEN** 任务 `withRetry(3, 1)`，HTTP 返回 500
- **THEN** 重试至多 3 次，最终 state=FAILED，attempts>=3

#### Scenario: HTTP 404 不重试
- **WHEN** 任务 `withRetry(3, 1)`，HTTP 返回 404
- **THEN** 不重试，立即 state=FAILED，attempts=1，last_error 包含 "not retryable" 或 "http 404"

#### Scenario: shell 非零 exit 仍重试
- **WHEN** 任务 `withRetry(3, 1)`，shell exit_code=7
- **THEN** 重试至多 3 次（保持当前行为）

### Requirement: 非法 cron 表达式立即失败
The system SHALL 在 dispatch 时立即拒绝非法 cron 表达式，而非静默入队后永不触发。

#### Scenario: 非法 cron dispatch 返回 error
- **WHEN** 用户 `Xhjob::task()->viaShell('echo hi')->cron('not a cron')->dispatch()`
- **THEN** 返回 `error: invalid cron: ...` 字符串
- **AND** 任务不入队（store 中无此 task_id）

### Requirement: Dead code 评估与保留决策
The system SHALL 对未使用代码做出明确决策：启用 / 保留为未来扩展 / 删除。

#### Scenario: 启用 is_retryable_* 函数
- **WHEN** `should_retry` 被调用
- **THEN** 真正调用 `is_retryable_http_status` / `is_retryable_shell_exit` 判断错误类型

#### Scenario: 保留 Event / should_fire_missed / thread_pool / make_store 并加注释
- **WHEN** 开发者阅读这些代码
- **THEN** 看到清晰的 doc comment 说明"保留原因 + 未来启用路径"

#### Scenario: 删除 sleep_for_retry
- **WHEN** 重新编译
- **THEN** `sleep_for_retry` 不再存在，retry 通过 next_fire 机制实现延迟

### Requirement: 任务暂停/恢复/取消/删除（对齐 APScheduler pause/resume/remove + Celery revoke）
The system SHALL 支持对已注册的 cron 作业进行生命周期管理：暂停、恢复、取消、删除。

#### Scenario: 暂停后 cron 不再触发
- **WHEN** 用户 `xhjob_pause($id)` 后等待下一次 cron tick
- **THEN** 任务不被触发，`xhjob_state($id)['paused'] = true`
- **AND** 任务定义保留，重启后仍为 paused

#### Scenario: 恢复后 cron 恢复触发
- **WHEN** 用户 `xhjob_resume($id)`
- **THEN** `paused = false`，下一次 tick 恢复触发

#### Scenario: 取消 Pending 任务
- **WHEN** 用户 `xhjob_cancel($id)` 且任务 state=Pending
- **THEN** state 变为 `Cancelled`（新终态），不再被触发或重试
- **AND** `xhjob_state($id)['state'] = 'CANCELLED'`

#### Scenario: 取消 Running 任务
- **WHEN** 用户 `xhjob_cancel($id)` 且任务 state=Running
- **THEN** 不强制 kill 子进程（保持当前行为）
- **AND** 标记 `cancel_requested = true`，单次执行结束后不再重试，且若为 cron 则后续 tick 不再触发

#### Scenario: 删除 cron 作业
- **WHEN** 用户 `xhjob_remove($id)`
- **THEN** 任务定义从 store 中删除（state / cron / next_fire 全部清掉）
- **AND** 正在执行的实例不受影响（继续运行至结束）
- **AND** 后续 cron tick 不再触发该 id

### Requirement: 任务起始/结束时间窗口（对齐 APScheduler start_date/end_date）
The system SHALL 支持为 cron 任务指定起始与结束时间，仅在窗口内触发。

#### Scenario: start_date 之前不触发
- **WHEN** 任务 `startAt(time() + 60)` 且 cron 每 5s 触发
- **THEN** 60s 内不被触发，但 `next_fire` 仍按 cron 推进
- **AND** 到达 `start_date` 后开始触发

#### Scenario: end_date 之后停止触发
- **WHEN** 任务 `endAt(time() + 30)` 且 cron 每 5s 触发
- **THEN** 30s 后任务 state 变为 `Success`（终态），后续 tick 不再触发
- **AND** `execution_count` 为触发期间实际执行次数

### Requirement: 任务列表查询（对齐 APScheduler get_jobs）
The system SHALL 提供查询当前 service 所有任务摘要的能力。

#### Scenario: 列出全部任务
- **WHEN** 用户 `xhjob_list('default')`
- **THEN** 返回数组，每个元素含 `id` / `type` / `state` / `cron` / `attempts` / `next_fire` / `paused` / `max_executions` / `execution_count`

#### Scenario: 按状态过滤
- **WHEN** 用户 `xhjob_list('default', 'PENDING')`
- **THEN** 仅返回 state=Pending 的任务

### Requirement: 任务结果过期清理（对齐 Celery result_expires）
The system SHALL 支持为任务指定结果保留时长，超时后自动清理 result 行但保留 task 行。

#### Scenario: resultTtl(5) 后 5 秒清理
- **WHEN** 任务终态 SUCCESS 后 `now - finished_at > 5`（秒）
- **THEN** `xhjob_result($id)` 返回空结果（body/stdout 等字段为 null）
- **AND** `xhjob_state($id)` 仍可查到任务摘要

#### Scenario: 不设置 resultTtl 时永久保留
- **WHEN** 任务未设置 `resultTtl`（默认 0）
- **THEN** result 永久保留，与当前行为一致（向后兼容）

### Requirement: 任务元数据 meta（对齐 Celery update_state meta）
The system SHALL 支持为任务附加任意用户元数据，便于业务侧追踪。

#### Scenario: withMeta 写入与读取
- **WHEN** 用户 `withMeta('{"order_id":"A123"}')->dispatch()`
- **THEN** `xhjob_state($id)['meta'] = '{"order_id":"A123"}'`
- **AND** meta 持久化到 SQLite，重启后保留

### Requirement: 任务优先级队列生效（对齐 Celery priority）
The system SHALL 在队列层按 priority 数值倒序处理任务。

#### Scenario: 高优先级先执行
- **WHEN** 同时 dispatch priority=10 与 priority=1 两个 Pending 任务
- **THEN** priority=10 的任务先被取出执行
- **AND** priority=1 的任务随后执行

### Requirement: 修复 maxExecutions 终态判定时机 bug
The system SHALL 让 cron 任务在 `max_executions` 未到达上限前保持可触发状态，不立即标记 Success 终态。

#### Scenario: cron + maxExecutions(3) 执行 3 次后停止
- **WHEN** 用户 `cron('*/1 * * * * *')->maxExecutions(3)->viaShell('echo hi')->dispatch()`，等待 cron 触发
- **THEN** 第 1 次执行后 `execution_count=1` 且 `state` 仍为 Pending（可触发，非 Success 终态）
- **AND** 第 2 次执行后 `execution_count=2` 且 `state` 仍为 Pending
- **AND** 第 3 次执行后 `execution_count=3` 且 `state` 变为 `Success` 终态（max_executions 到达）
- **AND** 第 4 次 cron tick 不再触发该任务

#### Scenario: cron 不设 maxExecutions 时持续触发
- **WHEN** cron 任务未设 maxExecutions（默认 0），执行成功
- **THEN** `execution_count` 持续递增
- **AND** `state` 不变为 Success 终态，cron 持续触发（向后兼容）

#### Scenario: 非 cron 任务 success 后立即 Success 终态
- **WHEN** 非 cron 任务（仅 delay 或立即执行）执行成功
- **THEN** 立即标记为 `Success` 终态（保持当前行为，向后兼容）

### Requirement: IntervalTrigger（对齐 APScheduler IntervalTrigger）
The system SHALL 支持按固定秒数间隔周期触发任务，无需写 cron 表达式。

#### Scenario: every(30) 每 30 秒触发一次
- **WHEN** 用户 `Xhjob::task()->every(30)->viaShell('echo hi')->dispatch()`
- **THEN** 任务入队，`next_fire = now + 30`
- **AND** 30 秒后被触发，触发后 `next_fire` 推进为 `now + 30`（下一轮 30 秒）
- **AND** 持续触发直至 daemon 停止（与 maxExecutions 配合可限制次数）

#### Scenario: every 与 cron 同时设置时 cron 优先
- **WHEN** 用户同时设置 `every(30)` 与 `cron('*/1 * * * *')`
- **THEN** 使用 cron 调度（cron 优先），every 被忽略
- **AND** tracing::warn 输出"both every() and cron() set, cron takes precedence"

#### Scenario: every 与 runAt 同时设置时 runAt 优先
- **WHEN** 用户同时设置 `every(30)` 与 `runAt(time() + 60)`
- **THEN** 使用 runAt 调度（一次性），every 被忽略
- **AND** tracing::warn 输出"both every() and runAt() set, runAt takes precedence"

### Requirement: DateTrigger（对齐 APScheduler DateTrigger）
The system SHALL 支持在指定绝对 Unix 时刻触发一次任务，触发后立即终止。

#### Scenario: runAt 指定时刻一次性触发
- **WHEN** 用户 `Xhjob::task()->runAt(time() + 60)->viaShell('echo hi')->dispatch()`
- **THEN** 任务入队，`next_fire = time() + 60`，`run_at = time() + 60`
- **AND** 60 秒后被触发执行
- **AND** 执行结束后（无论成功失败）state 立即变为 `Success` 或 `Failed` 终态，不再触发下一轮

#### Scenario: runAt 已过期时刻立即触发
- **WHEN** 用户 `runAt(time() - 30)`（已过去的时刻）
- **THEN** next_fire 已过期，下一轮 scan_once 立即触发
- **AND** 执行后立即终态

### Requirement: Jitter（对齐 APScheduler jitter）
The system SHALL 支持为周期任务（cron/interval）添加随机抖动，避免多任务同时触发造成惊群。

#### Scenario: jitter(10) 后 next_fire 0-10 秒随机偏移
- **WHEN** 任务 `cron('*/1 * * * *')->jitter(10)`
- **THEN** scan_once 计算 next_fire 后追加 `0..10` 秒随机偏移
- **AND** 多个相同 cron 表达式 + jitter(10) 的任务，触发时刻分散在 10 秒窗口内

#### Scenario: jitter 默认 0 无偏移
- **WHEN** 任务未设置 jitter（默认 0）
- **THEN** next_fire 不偏移，行为与当前一致（向后兼容）

#### Scenario: runAt 一次性任务不应用 jitter
- **WHEN** 任务 `runAt(time() + 60)->jitter(10)`
- **THEN** jitter 不生效（runAt 是一次性绝对时刻，不抖动）
- **AND** tracing::warn 输出"jitter ignored for runAt task"

### Requirement: Task expires（对齐 Celery expires）
The system SHALL 支持任务级过期：入队后若未在指定秒数内开始执行，则丢弃为 Expired 终态。

#### Scenario: expires(60) 后 60 秒未执行则 Expired
- **WHEN** 任务 `expires(60)->dispatch()`，但因队列拥堵或 maxInstances 限制 60 秒内未被取出执行
- **THEN** scan_once 检测到 `task.expires > 0 && task.created_at + 60 < now && task.state == Pending`
- **AND** 将 state 置为 `Expired`（新终态），不再被触发
- **AND** `xhjob_state($id)['state'] = 'EXPIRED'`

#### Scenario: expires 默认 0 不过期
- **WHEN** 任务未设置 expires（默认 0）
- **THEN** 任务永不因超时丢弃，与当前行为一致（向后兼容）

#### Scenario: 已 Running 的任务不受 expires 影响
- **WHEN** 任务已 Running（state=Running），且 `now > created_at + expires`
- **THEN** 不被 expires 触发为 Expired（已开始执行的任务不受 pre-execution TTL 影响）

### Requirement: Task requeue（对齐 Celery retry 运维场景）
The system SHALL 支持将终态任务（Cancelled/Failed/Expired）重置为 Pending 并重新入队。

#### Scenario: requeue Cancelled 任务
- **WHEN** 用户 `xhjob_cancel($id)` 后任务为 Cancelled，再调用 `xhjob_requeue($id)`
- **THEN** state 重置为 `Pending`，attempts 清零，next_fire = now
- **AND** 任务定义（cron / interval / meta / max_executions 等）保留不变
- **AND** execution_count 不清零（仍累计执行次数，避免 maxExecutions 被绕过）

#### Scenario: requeue 失败任务
- **WHEN** 任务 state=Failed，调用 `xhjob_requeue($id)`
- **THEN** state 重置为 Pending，attempts 清零，next_fire = now，重新入队

#### Scenario: requeue Running/Pending 任务被拒绝
- **WHEN** 任务 state=Running 或 Pending，调用 `xhjob_requeue($id)`
- **THEN** 返回 false（拒绝 requeue），state 不变

### Requirement: Retry exponential backoff（对齐 Celery retry_backoff）
The system SHALL 支持为任务重试启用指数退避，避免固定间隔对网络抖动场景不友好。

#### Scenario: retryBackoff(true) + retry_delay=1 + retry_max=5
- **WHEN** 任务 `withRetry(5, 1)->retryBackoff(true)`，第 1 次失败后 schedule_retry
- **THEN** 第 1 次重试 next_fire = now + 1 * 2^0 = now + 1
- **AND** 第 2 次重试 next_fire = now + 1 * 2^1 = now + 2
- **AND** 第 3 次重试 next_fire = now + 1 * 2^2 = now + 4
- **AND** 第 4 次重试 next_fire = now + 1 * 2^3 = now + 8
- **AND** 第 5 次重试 next_fire = now + 1 * 2^4 = now + 16
- **AND** 上限为 retry_delay * 60 = 60 秒（若计算值超过 60 则截断为 60）

#### Scenario: retryBackoff 默认 false 保持固定 delay
- **WHEN** 任务未设置 retryBackoff（默认 false）
- **THEN** schedule_retry 使用固定 retry_delay（保持当前行为，向后兼容）

### Requirement: max_instances(N) 真实生效（对齐 APScheduler max_instances）
The system SHALL 让 `Task.max_instances` 字段真正作为"并发实例上限"使用，解耦自 `allow_overlap` 二元开关，支持 N>1 真实并发实例。

#### Scenario: max_instances(2) 允许 2 个并发实例
- **WHEN** 用户 `cron('*/1 * * * *')->maxInstances(2)->viaShell('sleep 90')->dispatch()`
- **AND** 第 1 次触发后任务进入 Running，第 2 次 cron tick 到来时第 1 次仍未结束
- **THEN** `OverlapController::should_fire` 检查 `count_running_instances(id) < 2`，允许第 2 次并发触发
- **AND** 第 3 次 cron tick 到来时已有 2 个 Running 实例，`should_fire` 返回 false，跳过本次触发
- **AND** 第 1 个实例结束后释放槽位，下次 tick 可再次触发

#### Scenario: max_instances=1（默认）保持当前行为
- **WHEN** 用户不设置 maxInstances（默认 1），cron 任务执行慢
- **THEN** 第 1 次触发后 Running，第 2 次 tick 时 `count_running_instances(id) = 1 >= 1`，跳过触发
- **AND** 行为与当前 `allow_overlap=false` 一致（向后兼容）

#### Scenario: allow_overlap=true + max_instances=N 取 N
- **WHEN** 用户同时设置 `allowOverlap(true)` 与 `maxInstances(3)`
- **THEN** 以 `max_instances=3` 为准（max_instances 优先级高于 allow_overlap），允许至多 3 个并发实例
- **AND** tracing::warn 输出"both allowOverlap and maxInstances set, maxInstances takes precedence"

#### Scenario: allow_overlap=true 单独设置（无 maxInstances）保持无限并发
- **WHEN** 用户只设置 `allowOverlap(true)` 不设置 maxInstances（默认 1）
- **THEN** 保持当前行为（无限并发，向后兼容）
- **AND** 不受 `max_instances=1` 限制（兼容老代码）

### Requirement: reschedule_job（对齐 APScheduler reschedule_job）
The system SHALL 支持在线修改 cron 任务的 cron 表达式，不丢失 state / attempts / execution_count / meta 等历史字段。

#### Scenario: reschedule 修改 cron 频率
- **WHEN** 用户 dispatch 一个 `cron('*/5 * * * *')` 任务并执行 3 次（execution_count=3），然后调用 `xhjob_reschedule($id, '*/1 * * * *')`
- **THEN** `task.cron` 更新为 `*/1 * * * *`
- **AND** `next_fire` 重新计算为 `cron_next(now, tz)`
- **AND** `execution_count` 仍为 3（不清零）
- **AND** `state` 仍为 Pending（不重置为初始态）
- **AND** `meta` / `max_executions` / `start_date` / `end_date` 等其他字段保留不变

#### Scenario: reschedule 非 cron 任务返回 false
- **WHEN** 用户对 interval / runAt / one-shot 任务调用 `xhjob_reschedule($id, $cron)`
- **THEN** 返回 false，task.cron 不变
- **AND** tracing::warn 输出"reschedule only applicable to cron tasks"

#### Scenario: reschedule 终态任务返回 false
- **WHEN** 用户对 Success / Failed / Cancelled / Expired 任务调用 `xhjob_reschedule($id, $cron)`
- **THEN** 返回 false，task.cron 不变
- **AND** tracing::warn 输出"cannot reschedule terminal task, call xhjob_requeue first"

#### Scenario: reschedule 非法 cron 返回 false
- **WHEN** 用户调用 `xhjob_reschedule($id, 'not a cron')`
- **THEN** 返回 false，task.cron 不变
- **AND** 错误信息含 `invalid cron: ...`

### Requirement: get_job（对齐 APScheduler get_job）
The system SHALL 提供单任务详情查询 API，返回完整 Task JSON 含所有配置 + 状态字段。

#### Scenario: xhjob_get 返回完整 Task JSON
- **WHEN** 用户 dispatch 一个任务并调用 `xhjob_get($id)`
- **THEN** 返回 JSON 字符串，包含所有配置字段（task_type / payload / cron / interval / run_at / retry_max / retry_delay / timeout / soft_timeout / priority / allow_overlap / max_instances / coalesce / max_executions / start_date / end_date / result_ttl / meta / ignore_result / acks_late / jitter / expires / retry_backoff / timezone）
- **AND** 包含所有状态字段（state / attempts / execution_count / next_fire / created_at / started_at / finished_at / last_error / paused / cancel_requested）
- **AND** PHP 端 `json_decode` 后可访问所有字段

#### Scenario: xhjob_get 不存在的任务返回 null
- **WHEN** 用户调用 `xhjob_get('non-existent-id')`
- **THEN** 返回 null 或 false（任务不存在）

#### Scenario: xhjob_get 区别于 xhjob_state
- **WHEN** 用户分别调用 `xhjob_state($id)` 与 `xhjob_get($id)`
- **THEN** `xhjob_state` 返回 StateInfo 摘要（state / attempts / next_fire / paused / start_date / end_date / meta 等运行时字段）
- **AND** `xhjob_get` 返回完整 Task JSON（额外含 cron / retry_max / retry_delay / timeout / soft_timeout / priority / max_instances / coalesce / max_executions / result_ttl / ignore_result / acks_late / jitter / expires / retry_backoff / timezone 等配置字段）

### Requirement: ignoreResult（对齐 Celery task_ignore_result）
The system SHALL 支持 fire-and-forget 任务不存储执行结果，节省磁盘与 IPC 流量。

#### Scenario: ignoreResult(true) 不存储结果
- **WHEN** 用户 `Xhjob::task()->viaShell('echo hi')->ignoreResult(true)->dispatch()`，等待执行完成
- **THEN** `xhjob_result($id)` 返回 null（结果未存储）
- **AND** `xhjob_state($id)` 仍正常返回 state=Success / attempts=1 / execution_count=1 等状态字段
- **AND** SQLite results 表中无此 task_id 的行

#### Scenario: ignoreResult 默认 false 仍存储结果
- **WHEN** 用户不设置 ignoreResult（默认 false）
- **THEN** `process_one` 正常调用 `save_result`，`xhjob_result($id)` 返回完整结果
- **AND** 行为与当前一致（向后兼容）

#### Scenario: ignoreResult 与 resultTtl 同时设置
- **WHEN** 用户同时设置 `ignoreResult(true)` 与 `resultTtl(60)`
- **THEN** ignoreResult 优先（根本不存 result），resultTtl 无效果
- **AND** tracing::warn 输出"both ignoreResult and resultTtl set, ignoreResult takes precedence"

### Requirement: acksLate（对齐 Celery task_acks_late）
The system SHALL 支持延迟确认模式：daemon 启动时扫描 Running 任务，对 `acks_late=true` 的任务重置为 Pending 重新入队。

#### Scenario: acksLate(true) + daemon 崩溃后重启
- **WHEN** 用户 `Xhjob::task()->viaShell('long-running-cmd')->acksLate(true)->dispatch()`，任务进入 Running，然后 daemon 进程被 kill -9 崩溃
- **AND** daemon 重启
- **THEN** daemon 启动初始化阶段扫描所有 Running 任务
- **AND** 对 `acks_late=true` 的任务重置 state=Pending + next_fire=now
- **AND** scan_once 立即触发重新执行该任务

#### Scenario: acksLate=false（默认）崩溃后卡死
- **WHEN** 用户不设置 acksLate（默认 false），任务 Running 中 daemon 崩溃后重启
- **THEN** daemon 启动时不重置该 Running 任务（保持当前行为）
- **AND** 任务永久卡在 Running 状态（向后兼容，不破坏现有行为）

#### Scenario: acksLate 对 Pending / Success 任务无影响
- **WHEN** daemon 启动时扫描 Running 任务，对 Pending / Success / Failed 等非 Running 状态任务不做任何操作
- **THEN** 仅 state=Running + acks_late=true 的任务被重置为 Pending

### Requirement: softTimeout（对齐 Celery task_soft_time_limit）
The system SHALL 支持为 shell 任务设置软超时，超时后先发 SIGTERM 优雅退出，等待宽限期后再 SIGKILL 硬超时。

#### Scenario: softTimeout(5) + timeout(10) shell 任务优雅退出
- **WHEN** 用户 `Xhjob::task()->viaShell('trap cleanup TERM; ...')->softTimeout(5)->timeout(10)->dispatch()`
- **AND** 任务执行 5 秒未结束
- **THEN** 在第 5 秒时向 shell 进程发 SIGTERM（让 trap 处理 cleanup）
- **AND** shell 进程在 5 秒宽限期内（5s ~ 10s）退出，state=Success 或 Failed（取决于 exit_code）
- **AND** 不发 SIGKILL（已优雅退出）

#### Scenario: softTimeout(5) + timeout(10) shell 任务未响应 SIGTERM
- **WHEN** 任务执行 5 秒未结束，SIGTERM 后进程未在 5 秒宽限期内退出
- **THEN** 在第 10 秒时（hard timeout）发 SIGKILL 强制 kill
- **AND** state=Failed，last_error 含 "killed by SIGKILL after soft timeout"

#### Scenario: HTTP 任务设置 softTimeout 被忽略
- **WHEN** 用户 `Xhjob::task()->viaHttp('GET', 'http://...')->softTimeout(5)->timeout(10)`
- **THEN** soft_timeout 被忽略（HTTP 客户端不支持优雅中断）
- **AND** tracing::warn 输出"softTimeout ignored for HTTP task"
- **AND** 仍按 `timeout=10` 硬超时执行

#### Scenario: softTimeout >= timeout 被忽略
- **WHEN** 用户 `softTimeout(15)->timeout(10)`
- **THEN** soft_timeout 被忽略（必须严格小于 timeout）
- **AND** tracing::warn 输出"softTimeout must be less than timeout, ignoring softTimeout"
- **AND** 仍按 `timeout=10` 硬超时执行

#### Scenario: softTimeout 默认 None 保持当前行为
- **WHEN** 用户不设置 softTimeout（默认 None）
- **THEN** 任务超时直接 SIGKILL（保持当前行为，向后兼容）

### Requirement: misfire_grace_time 每作业级（对齐 APScheduler misfire_grace_time per-job override）
The system SHALL 支持为单个 cron 任务设置独立的 misfire 容错窗口，覆盖全局默认 60s。

#### Scenario: misfireGraceTime(5) 短窗口跳过 6s 延迟的 missed fire
- **WHEN** 用户 `cron('*/1 * * * * *')->misfireGraceTime(5)->viaShell('echo hi')->dispatch()`，且 daemon 因 GC 停顿 6 秒未触发 cron tick
- **THEN** scan_once 检测 `now - next_fire > 5s`，视为 misfire
- **AND** 按 `coalesce=true` 规则跳过本次触发，推进 next_fire 至下一轮 cron 时刻
- **AND** 若 `coalesce=false` 则丢弃所有 missed 触发并推进 next_fire

#### Scenario: misfireGraceTime(300) 长窗口仍触发 60s 延迟的 missed fire
- **WHEN** 用户 `cron('*/1 * * * * *')->misfireGraceTime(300)->viaShell('echo hi')->dispatch()`，且 daemon 因 GC 停顿 60 秒
- **THEN** scan_once 检测 `now - next_fire < 300s`，仍触发本次执行
- **AND** execution_count 递增 1（视为正常触发）

#### Scenario: misfireGraceTime(0) 使用全局默认 60s
- **WHEN** 用户 `misfireGraceTime(0)` 或不调用（默认 0）
- **THEN** scan_once 使用全局默认 `misfire_grace_time = 60s` 判定 misfire
- **AND** 行为与当前一致（向后兼容）

#### Scenario: 非 cron 任务 misfireGraceTime 被忽略
- **WHEN** 用户 `every(30)->misfireGraceTime(5)` 或 `runAt(time()+60)->misfireGraceTime(5)`
- **THEN** misfire_grace_time 字段保留但不生效（interval / runAt 不存在 misfire 概念）
- **AND** tracing::warn 输出"misfireGraceTime ignored for non-cron task"

### Requirement: replace_existing 幂等 dispatch（对齐 APScheduler replace_existing）
The system SHALL 支持用户自定义 task id 并在 dispatch 同 id 时按 replace_existing 字段决定覆盖或报错。

#### Scenario: withId + replaceExisting(true) 同 id 重跑覆盖
- **WHEN** 用户首次 `Xhjob::task()->withId('cron:cleanup-temp')->cron('*/5 * * * *')->viaShell('rm -rf /tmp/*')->dispatch()`
- **AND** 第二次同 id 同 cron 不同 payload `withId('cron:cleanup-temp')->replaceExisting(true)->cron('*/5 * * * *')->viaShell('rm -rf /var/tmp/*')->dispatch()`
- **THEN** 第二次 dispatch 时 store 中已存在同 id 任务
- **AND** 先 remove 旧 task 再 insert 新 task（完全替换）
- **AND** state / attempts / execution_count 重置为新 task 的初始值（不保留旧任务历史）
- **AND** 后续 cron tick 按新 payload 触发

#### Scenario: withId + replaceExisting(false)（默认）同 id 报错
- **WHEN** 用户首次 `withId('cron:cleanup-temp')->dispatch()` 成功，第二次 `withId('cron:cleanup-temp')->replaceExisting(false)->dispatch()`
- **THEN** 第二次 dispatch 返回 `error: task id already exists: cron:cleanup-temp`
- **AND** store 中只有首次的任务，未被覆盖

#### Scenario: 不设置 withId 时 replaceExisting 无效果
- **WHEN** 用户不调用 `withId()`（系统生成 UUID），同时设置 `replaceExisting(true)`
- **THEN** 系统生成 UUID 必然不冲突，replaceExisting 无效果
- **AND** 行为与当前一致（每次 dispatch 都创建新 task）

#### Scenario: replaceExisting(true) 覆盖终态任务
- **WHEN** 用户 `withId('cron:cleanup-temp')->cron('*/1 * * * * *')->maxExecutions(3)->dispatch()`，等任务执行 3 次终态后再次 `withId('cron:cleanup-temp')->replaceExisting(true)->cron('*/5 * * * *')->dispatch()`
- **THEN** 终态 task 被 remove 后 insert 新 task
- **AND** 新 task 状态为 Pending，execution_count 重置为 0

### Requirement: tags 作业分组（对齐 APScheduler job tags）
The system SHALL 支持为任务附加任意标签数组，并允许通过 xhjob_list 按标签过滤查询。

#### Scenario: tags 标记 + xhjob_list 按 tag 过滤
- **WHEN** 用户 dispatch 3 个任务：`tags(['reports'])->cron('*/5 * * * *')->dispatch()` / `tags(['reports', 'critical'])->cron('*/1 * * * * *')->dispatch()` / `tags(['cleanup'])->cron('0 3 * * *')->dispatch()`
- **AND** 调用 `xhjob_list('default', null, 'reports')`
- **THEN** 返回数组包含前 2 个任务（都含 'reports' tag）
- **AND** 第 3 个任务不含 'reports' tag 不返回

#### Scenario: 多 tag 任务在任一匹配 tag 时返回
- **WHEN** 任务 `tags(['reports', 'critical'])->dispatch()`，调用 `xhjob_list('default', null, 'critical')`
- **THEN** 该任务被返回（tags 数组包含 'critical'）

#### Scenario: tags 默认空数组（向后兼容）
- **WHEN** 用户不调用 `tags()`（默认空数组）
- **THEN** task.tags = []
- **AND** `xhjob_list('default', null, 'reports')` 不返回该任务
- **AND** `xhjob_list('default', null, null)`（不过滤 tag）返回该任务

#### Scenario: 持久化后 tags 保留
- **WHEN** 用户 `tags(['critical'])->dispatch()`，daemon restart
- **THEN** `xhjob_get($id)` 返回 `tags: ["critical"]`
- **AND** `xhjob_list('default', null, 'critical')` 仍返回该任务

### Requirement: rate_limit 每任务限流（对齐 Celery task.rate_limit）
The system SHALL 支持为单个任务设置速率限制，限制其在指定时间窗口内最多执行的实例数。

#### Scenario: rateLimit(3, 10) 在 10s 窗口内最多 3 次触发
- **WHEN** 用户 `cron('*/1 * * * * *')->rateLimit(3, 10)->viaShell('curl https://api.example.com')->dispatch()`，daemon 在 10 秒内 cron tick 5 次
- **THEN** 第 1-3 次 tick 正常触发执行
- **AND** 第 4-5 次 tick 时 scan_once 检测 `window=10s 内已开始 3 个实例 >= max_count=3`
- **AND** 跳过本次触发并推进 next_fire 至下一轮 cron 时刻
- **AND** 第 11 秒时（窗口滑过）第 4 次触发可执行

#### Scenario: rateLimit 默认 0 不限流
- **WHEN** 用户不调用 rateLimit（默认 rate_limit_count=0 / rate_limit_window=0）
- **THEN** 任务不受速率限制，按 cron 频率正常触发（向后兼容）

#### Scenario: rateLimit 与 maxInstances 区别
- **WHEN** 用户 `cron('*/1 * * * * *')->maxInstances(1)->rateLimit(3, 60)->viaShell('sleep 30')->dispatch()`
- **THEN** maxInstances(1) 限制并发实例数 <= 1（30s 内只能 1 个 Running 实例）
- **AND** rateLimit(3, 60) 限制 60s 内最多触发 3 次（无论是否并发）
- **AND** 两个限制独立生效，取较严格者

#### Scenario: daemon 重启后 sliding window 重建
- **WHEN** 任务 `rateLimit(5, 60)->dispatch()`，daemon 在执行 3 次后崩溃重启
- **THEN** daemon 启动时从 store 中 `started_at > now - 60` 的任务记录重建 sliding window 计数
- **AND** 重启后窗口内已有 3 次执行记录，继续允许 2 次触发

### Requirement: acks_on_failure 失败时是否确认（对齐 Celery task_acks_on_failure_or_timeout）
The system SHALL 支持任务执行失败时不进入终态，而是重置为 Pending 持续重试，忽略 retry_max 上限，直到成功或被 cancel/remove。

#### Scenario: acksOnFailure(false) 失败任务持续重试忽略 retry_max
- **WHEN** 用户 `withRetry(3, 1)->acksOnFailure(false)->viaShell('exit 7')->dispatch()`，任务执行失败
- **THEN** 第 1 次失败后 state=Pending（非 Failed），attempts=1，next_fire=now+1s（指数退避若启用）
- **AND** 第 2、3、4、5 次失败后仍 state=Pending，attempts 持续递增
- **AND** **忽略 retry_max=3 上限**，任务持续重试直到成功或被 cancel/remove

#### Scenario: acksOnFailure(true)（默认）保持当前行为
- **WHEN** 用户 `withRetry(3, 1)->viaShell('exit 7')->dispatch()`（默认 acksOnFailure=true），任务执行失败
- **THEN** 第 1-3 次失败后 state=Pending（重试中），attempts 递增
- **AND** 第 4 次失败后 attempts=4 > retry_max=3，state=Failed（终态）
- **AND** 行为与当前一致（向后兼容）

#### Scenario: acksOnFailure(false) + retryBackoff(true) 指数退避持续重试
- **WHEN** 用户 `withRetry(3, 1)->retryBackoff(true)->acksOnFailure(false)->viaShell('exit 7')->dispatch()`
- **THEN** 失败后按指数退避（1, 2, 4, 8, 16, 60, 60, ... 秒）持续重试
- **AND** 不受 retry_max=3 限制
- **AND** 不影响其他任务的 retry 行为

#### Scenario: acksOnFailure(false) 任务被 cancel 终止
- **WHEN** 任务 `acksOnFailure(false)` 持续重试中，用户调用 `xhjob_cancel($id)`
- **THEN** state=Cancelled（终态），停止重试
- **AND** 即使 `acks_on_failure=false` 也尊重 cancel 信号（cancel 优先于 acks_on_failure）

#### Scenario: acksOnFailure(false) + acksLate(true) 互补
- **WHEN** 任务 `acksOnFailure(false)->acksLate(true)->viaShell('exit 7')->dispatch()`
- **AND** 任务失败后 daemon 崩溃重启
- **THEN** daemon 重启时 acksLate 重排 Running 任务为 Pending
- **AND** acksOnFailure=false 让失败任务持续重试忽略 retry_max
- **AND** 两者互补：acksLate 处理崩溃恢复，acksOnFailure 处理失败重试

### Requirement: worker_max_tasks_per_child daemon 自我回收（对齐 Celery worker_max_tasks_per_child）
The system SHALL 支持通过环境变量配置 daemon 进程在执行 N 个任务后自我回收优雅退出，由外部进程管理器重启。

#### Scenario: XHJOB_MAX_TASKS_PER_CHILD=5 daemon 执行 5 个任务后退出
- **WHEN** 用户 `XHJOB_MAX_TASKS_PER_CHILD=5 xhjob_daemon start`，dispatch 5 个 shell 任务
- **THEN** daemon 内部 `tasks_executed_since_start` 原子计数器递增至 5
- **AND** 第 5 个任务完成后检测 `tasks_executed_since_start >= 5`
- **AND** daemon 优雅退出（关闭 scheduler + queue + 等待 in-flight 任务完成 + flush store + exit 0）

#### Scenario: XHJOB_MAX_TASKS_PER_CHILD 默认 0 不回收
- **WHEN** 用户不设置环境变量（默认 0）
- **THEN** daemon 不进行自我回收，持续运行直到手动 stop 或外部 kill
- **AND** 行为与当前一致（向后兼容）

#### Scenario: 优雅退出等待 in-flight 任务完成
- **WHEN** daemon 已执行 N-1 个任务，第 N 个任务正在 Running 中
- **THEN** daemon 等待第 N 个任务完成（不强制 kill）
- **AND** 完成后 tasks_executed_since_start=N，触发退出
- **AND** flush store 后 exit 0

#### Scenario: 计数器在 daemon 重启后重置
- **WHEN** daemon 执行 5 个任务退出后由 systemd 自动重启
- **THEN** 新 daemon 进程的 `tasks_executed_since_start` 从 0 开始
- **AND** 再次执行 5 个任务后退出

### Requirement: timezone per-job（对齐 APScheduler CronTrigger timezone）
The system SHALL 支持为单个 cron 任务设置独立时区，覆盖 daemon 全局时区。

#### Scenario: timezone('America/New_York') 按纽约时区评估 cron
- **WHEN** 用户 `Xhjob::task()->cron('0 9 * * *')->timezone('America/New_York')->viaShell('echo daily report')->dispatch()`，daemon 全局时区为 `Asia/Shanghai`
- **THEN** cron 表达式 `0 9 * * *` 按纽约时区评估，即在北京时间 21:00（夏令时）或 22:00（冬令时）触发
- **AND** `xhjob_state($id)['timezone']` 返回 `"America/New_York"`
- **AND** 持久化后重启 daemon 仍按纽约时区评估

#### Scenario: timezone 默认 None 使用 daemon 全局时区
- **WHEN** 用户不调用 `timezone()`（默认 None）
- **THEN** cron 表达式按 daemon 全局时区评估（保持当前行为，向后兼容）
- **AND** `xhjob_state($id)['timezone']` 返回 null

#### Scenario: timezone 非法时区回退全局并 warn
- **WHEN** 用户 `timezone('Invalid/Zone')`
- **THEN** 解析失败，`tracing::warn` 输出 "invalid timezone 'Invalid/Zone', falling back to global timezone"
- **AND** cron 表达式按 daemon 全局时区评估（不中断 dispatch）
- **AND** task.timezone 字段仍保留原值（用户可见其配置错误）

#### Scenario: timezone 对非 cron 任务被忽略
- **WHEN** 用户 `every(30)->timezone('America/New_York')` 或 `runAt(time()+60)->timezone('America/New_York')`
- **THEN** timezone 字段保留但 scan_once 忽略（interval / runAt 用绝对 Unix 时间戳，与时区无关）
- **AND** tracing::warn 输出 "timezone ignored for non-cron task"
- **AND** 行为与未设置 timezone 一致

### Requirement: Event listener API 任务执行事件流（对齐 APScheduler add_listener + EVENT_JOB_*）
The system SHALL 在任务执行关键路径写入事件到 events 表，提供 PHP API 查询事件流。

#### Scenario: xhjob_events 查询过去 1 小时事件
- **WHEN** 用户 dispatch 一个任务并执行（started → succeeded），调用 `xhjob_events(time() - 3600)`
- **THEN** 返回 JSON 数组，包含 `started` 与 `succeeded` 事件
- **AND** 每个事件含 `task_id` / `event_type` / `payload` / `ts` 字段
- **AND** 按 ts 升序排列

#### Scenario: xhjob_events 按 task_id 过滤
- **WHEN** 用户 dispatch 3 个任务，调用 `xhjob_events(time() - 3600, 'default', $task_id_of_task_2)`
- **THEN** 仅返回 task_2 相关事件（不返回 task_1 / task_3 事件）

#### Scenario: events TTL 自动清理
- **WHEN** 用户设置 `XHJOB_EVENTS_TTL_SECS=3600`（1 小时），dispatch 任务后等待 1 小时
- **THEN** scan_once 周期性调用 `cleanup_expired_events`，删除 `now - ts > 3600` 的事件行
- **AND** `xhjob_events(time() - 7200)` 返回空数组（已清理）

#### Scenario: events 默认 TTL 24h
- **WHEN** 用户不设置 `XHJOB_EVENTS_TTL_SECS`（默认 86400）
- **THEN** events 表保留 24 小时内事件，超过自动清理
- **AND** 行为是预期的默认值（不影响向后兼容，events 表为新增）

#### Scenario: events 类型覆盖关键路径
- **WHEN** 任务经历 started → succeeded / failed / missed / cancelled / paused / resumed / expired / max_instances_reached / rate_limited 全部路径
- **THEN** 每个路径均调用 `record_event` 写入对应类型事件
- **AND** `xhjob_events` 返回的事件类型覆盖所有关键路径

### Requirement: coalesce 显式 per-job 行为（对齐 APScheduler coalesce 参数）
The system SHALL 让 `Task.coalesce` 字段真正控制 misfire 合并/丢弃行为，而非仅作为默认值。

#### Scenario: coalesce(true) 合并 missed 触发为最后一次执行
- **WHEN** 用户 `cron('*/1 * * * * *')->coalesce(true)->misfireGraceTime(60)->viaShell('echo hi')->dispatch()`，daemon 因 GC 停顿 5 秒未触发 cron tick
- **THEN** scan_once 检测到 `now - next_fire > 0`（misfire，但在 grace_time 内）
- **AND** 按 `coalesce=true` 合并 5 次 missed 触发为最后一次执行（只执行一次，不补跑 5 次）
- **AND** 推进 next_fire 至下一轮 cron 时刻
- **AND** execution_count 仅递增 1（而非 5）

#### Scenario: coalesce(false) 丢弃 missed 触发不执行补偿
- **WHEN** 用户 `cron('*/1 * * * * *')->coalesce(false)->misfireGraceTime(60)->viaShell('echo hi')->dispatch()`，daemon 因 GC 停顿 5 秒
- **THEN** scan_once 检测到 misfire（在 grace_time 内）
- **AND** 按 `coalesce=false` 丢弃所有 missed 触发（不执行任何补偿）
- **AND** 推进 next_fire 至下一轮 cron 时刻
- **AND** execution_count 不递增（不执行）

#### Scenario: coalesce 默认 true 保持当前行为
- **WHEN** 用户不调用 `coalesce()`（默认 true）
- **THEN** misfire 时按合并规则处理（与 A13 misfire_grace_time 配合）
- **AND** 行为与当前一致（向后兼容）

#### Scenario: coalesce 与 misfire_grace_time 配合
- **WHEN** 用户 `coalesce(false)->misfireGraceTime(5)`，daemon 停顿 6 秒（超出 grace_time）
- **THEN** 视为完全 misfire，按 `coalesce=false` 丢弃所有 missed 触发并推进 next_fire
- **AND** 不执行任何补偿
- **AND** 若 `coalesce=true` + `misfireGraceTime(5)` + 6 秒停顿：完全 misfire，但仍按合并规则执行最后一次

### Requirement: Task chain 顺序流水线（对齐 Celery chain）
The system SHALL 支持按顺序执行一系列任务，前任务 stdout 作为后任务 input，任一任务失败则整个 chain 终态 Failed。

#### Scenario: xhjob_chain 3 步 ETL 流水线
- **WHEN** 用户 `xhjob_chain([['viaShell' => 'curl http://api/data'], ['viaShell' => 'jq .transform'], ['viaShell' => 'curl -X POST http://api/load -d @-']])`
- **THEN** daemon 收到 chain dispatch 后依次 dispatch 第 1 个任务
- **AND** 第 1 个任务 success 后取 stdout 作为第 2 个任务的 stdin（通过 `XHJOB_CHAIN_INPUT` 环境变量或管道）
- **AND** 第 2 个任务 success 后取 stdout 作为第 3 个任务的 stdin
- **AND** 第 3 个任务 success 后 chain state 变为 `succeeded`，current_step = 2 (0-indexed)

#### Scenario: chain 失败中断
- **WHEN** chain 第 2 个任务失败（state=Failed）
- **THEN** chain state 变为 `failed`，current_step 停在 1
- **AND** 第 3 个任务不再 dispatch
- **AND** `xhjob_chain_state($chain_id)` 返回 `{state: "failed", current_step: 1, tasks: [{...success...}, {...failed...}, {...pending...}]}`

#### Scenario: xhjob_chain_state 查询 chain 状态
- **WHEN** 用户 dispatch 一个 3 步 chain 并调用 `xhjob_chain_state($chain_id)`
- **THEN** 返回 JSON，含 `chain_id` / `tasks` 数组（每个元素含 task_id + state）/ `current_step` / `state`
- **AND** state 为 `pending` / `running` / `succeeded` / `failed` 之一

#### Scenario: chain 持久化
- **WHEN** 用户 dispatch 一个 3 步 chain，第 1 步 success 后 daemon 崩溃重启
- **THEN** daemon 重启后从 chains 表恢复 chain 状态
- **AND** current_step = 1，继续 dispatch 第 2 步任务
- **AND** chain 整体流程不中断

#### Scenario: chain 空 tasks 数组报错
- **WHEN** 用户 `xhjob_chain([])`
- **THEN** 返回 `error: chain tasks cannot be empty`
- **AND** 不创建 chain 记录

### Requirement: Task group 并行批处理（对齐 Celery group）
The system SHALL 支持并行 dispatch 一组任务，等待所有任务终态后 group 整体完成，提供整体状态查询 API。

#### Scenario: xhjob_group 3 任务并行执行
- **WHEN** 用户 `xhjob_group([['viaShell' => 'curl http://api/1'], ['viaShell' => 'curl http://api/2'], ['viaShell' => 'curl http://api/3']])`
- **THEN** daemon 收到 group dispatch 后并行 dispatch 3 个任务（同时入队，按 priority 排队）
- **AND** 3 个任务并行执行（受 max_instances / rate_limit 限制）
- **AND** 全部 success 后 group state 变为 `succeeded`

#### Scenario: group 部分失败
- **WHEN** group 内 3 个任务中 1 个失败
- **THEN** group state 变为 `partial_failed`
- **AND** `completed_count = 3`（全部终态）/ `total_count = 3`
- **AND** `xhjob_group_state($group_id)` 返回 `{state: "partial_failed", completed_count: 3, total_count: 3, tasks: [{...success...}, {...failed...}, {...success...}]}`

#### Scenario: group 全部失败
- **WHEN** group 内 3 个任务全部失败
- **THEN** group state 变为 `failed`
- **AND** `completed_count = 3` / `total_count = 3`

#### Scenario: xhjob_group_state 查询 group 状态
- **WHEN** 用户 dispatch 一个 3 任务 group 并调用 `xhjob_group_state($group_id)`
- **THEN** 返回 JSON，含 `group_id` / `tasks` 数组 / `completed_count` / `total_count` / `state`
- **AND** state 为 `pending` / `running` / `succeeded` / `partial_failed` / `failed` 之一

#### Scenario: group 持久化
- **WHEN** 用户 dispatch 一个 3 任务 group，2 个 success 后 daemon 崩溃重启
- **THEN** daemon 重启后从 groups 表恢复 group 状态
- **AND** 已 success 的任务不重跑，等待第 3 个任务终态
- **AND** group 整体流程不中断

#### Scenario: group 空 tasks 数组报错
- **WHEN** 用户 `xhjob_group([])`
- **THEN** 返回 `error: group tasks cannot be empty`
- **AND** 不创建 group 记录

### Requirement: worker_max_memory_per_child 基于内存的 daemon 自我回收（对齐 Celery worker_max_memory_per_child）
The system SHALL 支持通过环境变量配置 daemon 进程在内存使用超过阈值后自我回收优雅退出，与 worker_max_tasks_per_child 互补。

#### Scenario: XHJOB_MAX_MEMORY_PER_CHILD=100 daemon 内存超 100MB 后退出
- **WHEN** 用户 `XHJOB_MAX_MEMORY_PER_CHILD=100 xhjob_daemon start`，dispatch 处理大 payload 任务使 daemon 内存使用超过 100MB
- **THEN** daemon 在 `process_one` 完成后读取 `/proc/self/status` 中 `VmRSS`（KB）转换为 MB
- **AND** 检测 `vm_rss_mb >= 100`
- **AND** daemon 优雅退出（关闭 scheduler + queue + 等待 in-flight 任务完成 + flush store + exit 0）

#### Scenario: XHJOB_MAX_MEMORY_PER_CHILD 默认 0 不回收
- **WHEN** 用户不设置环境变量（默认 0）
- **THEN** daemon 不检查内存使用，持续运行直到手动 stop 或外部 kill
- **AND** 行为与当前一致（向后兼容）

#### Scenario: max_memory_per_child 与 max_tasks_per_child 互补
- **WHEN** 用户同时设置 `XHJOB_MAX_MEMORY_PER_CHILD=100` 与 `XHJOB_MAX_TASKS_PER_CHILD=1000`
- **THEN** 两个检查独立生效
- **AND** daemon 在任务数达 1000 或内存达 100MB 时退出（先到达阈值者先触发）
- **AND** 适用于"任务数累积 + 内存泄漏"双重场景

#### Scenario: 内存监控跨平台
- **WHEN** daemon 运行在 Linux / macOS / Windows 平台
- **THEN** Linux 使用 `/proc/self/status` 的 `VmRSS` 字段
- **AND** macOS 使用 `mach_task_basic_info` API（通过 `sysctl` crate）
- **AND** Windows 使用 `GetProcessMemoryInfo` API
- **AND** 跨平台统一返回 MB 单位

#### Scenario: 优雅退出等待 in-flight 任务完成
- **WHEN** daemon 检测到内存超阈值，但当前有 2 个任务 Running
- **THEN** daemon 等待 2 个 Running 任务完成（不强制 kill）
- **AND** 完成后 flush store 并 exit 0
- **AND** 由外部进程管理器（systemd / supervisor）自动重启

## MODIFIED Requirements

### Requirement: 测试覆盖度
在原有测试基础上，要求：
- 全量测试在本机 PHP 8.2 环境实际执行（非仅代码审查）
- 边界场景测试与 maxExecutions 测试在本机实际执行并 PASS
- 新增 lifecycle_api / start_end_date / result_ttl / meta_field 测试在本机实际执行并 PASS
- 新增 interval_trigger / run_at_trigger / jitter_test / expires_test / requeue_test / retry_backoff 测试在本机实际执行并 PASS
- 新增 max_instances_test / reschedule_test / get_job_test / ignore_result_test / acks_late_test / soft_timeout_test 测试在本机实际执行并 PASS
- 新增 misfire_grace_time_test / replace_existing_test / tags_test / rate_limit_test / acks_on_failure_test / max_tasks_per_child_test 测试在本机实际执行并 PASS
- 新增 timezone_test / events_test / coalesce_test / chain_test / group_test / max_memory_per_child_test 测试在本机实际执行并 PASS
- 既有测试（cargo + .phpt + business + examples）不回归

### Requirement: 文档与代码一致性
- README API 参考表与实际 `Xhjob` 类方法一致（含新增 `maxExecutions` / `startAt` / `endAt` / `resultTtl` / `withMeta` / `every` / `runAt` / `jitter` / `expires` / `retryBackoff` / `ignoreResult` / `acksLate` / `softTimeout` / `maxInstances` 升级说明 / `misfireGraceTime` / `withId` / `replaceExisting` / `tags` / `rateLimit` / `acksOnFailure` / `timezone` / `coalesce`）
- README 顶层函数表与实际函数一致（含新增 `xhjob_remove` / `xhjob_pause` / `xhjob_resume` / `xhjob_cancel` / `xhjob_list`（含可选 tag 参数）/ `xhjob_requeue` / `xhjob_reschedule` / `xhjob_get` / `xhjob_events` / `xhjob_chain` / `xhjob_chain_state` / `xhjob_group` / `xhjob_group_state`）
- README cron 章节明确 5/6 段表达式格式
- README 环境变量表标注 `XHJOB_PERSIST` 读取时机 + 新增 `XHJOB_MAX_TASKS_PER_CHILD` / `XHJOB_MAX_MEMORY_PER_CHILD` / `XHJOB_EVENTS_TTL_SECS` 行
- README 新增"Cron 执行次数限制"小节
- README 新增"任务暂停/恢复/取消"小节
- README 新增"任务起始/结束时间"小节
- README 新增"任务列表查询"小节
- README 新增"misfire 处理"小节
- README 新增"任务结果过期清理"小节
- README 新增"任务元数据"小节
- README 新增"Interval 周期触发（every）"小节
- README 新增"DateTrigger 一次性触发（runAt）"小节
- README 新增"Jitter 随机抖动"小节
- README 新增"任务级过期（expires）"小节
- README 新增"任务重新入队（requeue）"小节
- README 新增"Retry 指数退避（retryBackoff）"小节
- README 新增"max_instances 并发实例数（A10）"小节
- README 新增"reschedule 在线修改 cron（A11）"小节
- README 新增"xhjob_get 单任务详情查询（A12）"小节
- README 新增"ignoreResult fire-and-forget（C9）"小节
- README 新增"acksLate 崩溃恢复（C10）"小节
- README 新增"softTimeout 软超时（C11）"小节
- README 新增"misfire_grace_time 每作业级（A13）"小节
- README 新增"replace_existing 幂等 dispatch（A14）"小节
- README 新增"tags 作业分组（A15）"小节
- README 新增"rateLimit 每任务限流（C12）"小节
- README 新增"acksOnFailure 失败不放弃（C13）"小节
- README 新增"worker_max_tasks_per_child daemon 自我回收（C14）"小节
- README 新增"timezone per-job 每作业独立时区（A16）"小节
- README 新增"Event listener API 任务执行事件流（A17）"小节
- README 新增"coalesce 显式 per-job 行为（A18）"小节
- README 新增"Task chain 顺序流水线（C15）"小节
- README 新增"Task group 并行批处理（C16）"小节
- README 新增"worker_max_memory_per_child 基于内存 daemon 自我回收（C17）"小节
- README 新增"对照 APScheduler / Celery 的功能对齐"小节扩展为 35 项（A1-A18 + C1-C17）

### Requirement: 对照 Python 成熟框架对齐
- 当前实现与 [APScheduler](https://apscheduler.readthedocs.io/)（cron 触发 + 作业管理）和 [Celery](https://docs.celeryq.dev/)（任务队列 + 重试 + 结果回查）对比
- 引入单机版合理可用的对齐特性（详见上文"对照 Python 成熟框架对齐"章节），共 **35 项**：
  - **APScheduler 对齐 18 项（A1-A18）**：作业删除 / 暂停恢复 / 起止时间 / misfire 显式化 / 任务列举 / next_run_time 暴露 / IntervalTrigger / DateTrigger / Jitter / max_instances 真实生效 / reschedule_job / get_job / misfire_grace_time 每作业级 / replace_existing 幂等 dispatch / tags 作业分组 / **timezone per-job** / **Event listener API** / **coalesce 显式 per-job 行为**
  - **Celery 对齐 17 项（C1-C17）**：任务取消 / 重试时间上限说明 / 优先级队列 / 结果过期清理 / 元数据 meta / Task expires / Task requeue / Retry exponential backoff / ignoreResult / acksLate / softTimeout / rate_limit 每任务限流 / acks_on_failure 失败不放弃 / worker_max_tasks_per_child daemon 自我回收 / **Task chain 顺序流水线** / **Task group 并行批处理** / **worker_max_memory_per_child 基于内存自我回收**
- 明确不对齐的特性（分布式 broker / beat 多进程调度 / 多种 datastore 后端 / 远程控制命令 / Django-CELERY 结果后端），避免过度设计

## REMOVED Requirements

### Requirement: 删除未使用 dead code
**Reason**: 用户反馈要求评估而非删除，应启用有价值的（`is_retryable_*`）、保留有未来扩展价值的（`Event` / `should_fire_missed` / `thread_pool` / `make_store`）、仅删除真正无用的（`sleep_for_retry`）
**Migration**: 已在新 spec 中按此原则重新规划
