# 增强后台任务队列异步功能并对标 APScheduler/Celery 评估优化 Spec

## Why
xhjob Rust 扩展已实现 20 个 PHP 函数 + 36 个 TaskBuilder 链式方法，覆盖 cron/interval/date 触发器、retry/timeout/soft_timeout、chain/group、persist、rate_limit 等核心能力，并通过了 29 项深度功能测试 + 8 项持久化测试。但对标 Python 成熟生态 APScheduler（executors/jobstores/listeners/coalesce/misfire）与 Celery（chord/countdown/eta/inspect/update_state/events/routing）后，仍存在若干标志性缺失：**chord（group+回调）、countdown 便捷派发、任务进度上报（update_state with meta）、事件监听器（register_listener）、inspect 聚合查询**。同时 ThinkPHP 8 集成包 `/workspace/releases/xhjob-thinkphp8-extend` 缺少专门针对"后台任务队列异步特性"的测试套件。本 spec 在不破坏现有能力的前提下补齐这些差距，并交付一份特性矩阵评估文档与异步队列测试套件。

## What Changes
- 新增 **chord** 原语（Celery 标志性）：并行执行一组任务，全部完成后触发一个回调任务，回调任务的 meta 携带所有子任务结果
- 新增 **countdown(seconds)** 便捷派发：相对当前时间的延迟执行（等价于 `run_at(now + seconds)` 的语法糖），补齐 Celery `apply_async(countdown=N)` 高频用法
- 新增 **任务进度上报**（Celery `update_state(state='PROGRESS', meta=...)`）：任务执行中通过 IPC 上报进度，`xhjob_state` 返回 `progress` 与 `progress_meta` 字段
- 新增 **事件监听器**（APScheduler `add_listener`）：PHP 端注册回调函数监听事件类型，daemon 通过 IPC 推送匹配事件（短轮询拉取模式，避免长连接复杂性）
- 新增 **inspect 聚合查询**（Celery `inspect`）：`xhjob_inspect('active'|'registered'|'scheduled'|'stats')` 一次返回 daemon 全局视图
- 新增 **后台任务队列异步功能测试套件**：`/workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php`，覆盖非阻塞派发、批量并发、进度查询、回调通知、chord、countdown、inspect、队列深度、worker 负载
- 新增 **APScheduler/Celery 特性矩阵评估文档**：`/workspace/releases/xhjob-thinkphp8-extend/EVALUATION.md`，逐项标注 已实现/部分实现/未实现/不适用（单机场景），并给出优化建议
- **BREAKING**：无（所有新功能为新增函数与新增字段，不修改现有 API 签名）

## Impact
- Affected specs：
  - `implement-async-task-scheduler`（基础能力已实现，本 spec 在其上增量）
  - `build-production-daemon-framework`（daemon 框架已交付，本 spec 新增 inspect/chord 进程内逻辑）
- Affected code：
  - `src/lib.rs`：新增 `xhjob_chord` / `xhjob_chord_state` / `xhjob_inspect` / `xhjob_report_progress` / `xhjob_pull_events` / `xhjob_countdown`（dispatch 糖）/ 注册函数表
  - `src/task/mod.rs`：`TaskBuilder` 新增 `chord_callback: Option<serde_json::Value>` / `countdown: Option<u64>` 字段；`build()` 中 countdown→run_at 归一化
  - `src/store/mod.rs`：`Task` 新增 `progress: Option<u8>` / `progress_meta: Option<String>` / `chord_id: Option<String>`；`TaskStore` trait 新增 `update_progress` / `list_active_summary` / `list_registered_summary` / `list_scheduled_summary` / `worker_stats` 方法
  - `src/store/in_memory.rs` + `src/store/sqlite.rs`：实现上述新 trait 方法；sqlite tasks 表新增 `progress` / `progress_meta` / `chord_id` 列（CREATE TABLE 含字段，向后兼容旧库通过 ensure_column）
  - `src/daemon_main.rs`：新增 op 处理 `chord` / `inspect` / `report_progress` / `pull_events`；chord 完成判定在 group refresh 后触发回调
  - `src/scheduler/chord.rs`（新文件）：chord 记录管理 + `refresh_state`（全部 success→派发回调任务）
  - `src/scheduler/group.rs`：`refresh_state` 后联动 chord 检查
  - `/workspace/tp/extend/Xhjob/TaskBuilder.php`：新增 `chord(array $builders, $callback)` / `countdown(int $s)` / `reportProgress()` 静态工厂与链式方法
  - `/workspace/tp/extend/Xhjob/TaskManager.php`：新增 `createChord` / `chordState` / `inspect` / `reportProgress` / `pullEvents` 方法
  - `/workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php`（新文件）：异步队列测试套件
  - `/workspace/releases/xhjob-thinkphp8-extend/EVALUATION.md`（新文件）：特性矩阵评估

## ADDED Requirements

### Requirement: Chord 原语（Celery chord）
系统 SHALL 提供 chord 原语：并行执行一组 header 任务，当且仅当全部 header 任务成功后，触发一个 body 回调任务；回调任务的 meta 携带所有 header 任务的结果数组。部分 header 失败时，chord 进入 `partial_failed` 终态，不触发回调（或触发一个 error 回调，本 spec 采用前者：失败即终止 chord）。

#### Scenario: 全部成功的 chord
- **WHEN** PHP 代码调用 `TaskBuilder::chord([t1, t2, t3], $callbackBuilder)->dispatch()`
- **THEN** daemon 并行派发 t1/t2/t3（复用 group 派发逻辑）
- **AND** 三者全部 Success 后，daemon 自动派发回调任务，其 meta 为 `[{"id":"t1","result":{...}},{"id":"t2","result":{...}},{"id":"t3","result":{...}}]`
- **AND** `xhjob_chord_state($chord_id)` 在回调前返回 `running`，回调完成后返回 `success`

#### Scenario: 部分 header 失败
- **WHEN** chord 中 t2 失败（Failed 终态）
- **THEN** chord 状态变为 `partial_failed`
- **AND** 回调任务不被派发
- **AND** `xhjob_chord_state` 返回 `partial_failed` 与失败的子任务 id 列表

### Requirement: Countdown 便捷派发（Celery countdown）
系统 SHALL 提供 `countdown(seconds)` 链式方法，等价于 `run_at(time() + seconds)`，补齐 Celery `apply_async(countdown=N)` 高频用法。

#### Scenario: countdown 延迟派发
- **WHEN** PHP 代码调用 `TaskBuilder::shell('echo late')->countdown(5)->dispatch()`
- **THEN** 任务在 5 秒后触发
- **AND** `xhjob_state` 在前 5 秒内返回 `pending`
- **AND** 5 秒后任务进入 `running` → `success`

### Requirement: 任务进度上报（Celery update_state）
系统 SHALL 允许任务执行体通过 `xhjob_report_progress($id, $percent, $meta_json)` 上报进度，`xhjob_state` 返回 `progress`（0-100）与 `progress_meta`（任意 JSON 字符串）字段。

#### Scenario: shell 任务上报进度
- **WHEN** shell 命令执行中调用 `xhjob_report_progress $task_id 50 '{"step":"half"}'`
- **THEN** `xhjob_state($task_id)` 返回 `progress=50` 与 `progress_meta={"step":"half"}`
- **AND** 进度上报不影响任务状态机（仍是 `running`）
- **AND** 进度数据持久化到 store（sqlite / in-memory）

#### Scenario: HTTP 任务无进度
- **WHEN** HTTP 任务执行（无法上报进度）
- **THEN** `progress` 字段为 null 或 0
- **AND** 不影响任务完成

### Requirement: 事件监听器（APScheduler add_listener）
系统 SHALL 提供事件拉取接口 `xhjob_pull_events($since_ts, $event_type_filter, $name, $data_dir)`，PHP 端可短轮询拉取自指定时间戳后的匹配事件，实现监听器模式（避免长连接/回调的跨进程复杂性）。

#### Scenario: 拉取某任务的成功事件
- **WHEN** PHP 代码调用 `xhjob_pull_events(0, 'succeeded', 'default', null)`
- **THEN** 返回所有 `succeeded` 类型事件列表（含 task_id / ts / payload）
- **AND** 支持 `event_type_filter=null` 返回全部类型

#### Scenario: 按任务 id 过滤
- **WHEN** PHP 代码调用 `xhjob_pull_events($since, null, 'default', null)` 后在结果中按 task_id 客户端过滤
- **THEN** 正常工作（服务端已支持 task_id_filter 参数，本 spec 在 PHP 端封装便捷方法）

### Requirement: Inspect 聚合查询（Celery inspect）
系统 SHALL 提供 `xhjob_inspect($mode, $name, $data_dir)` 一次返回 daemon 全局视图，`mode` 取值：`active`（运行中任务）、`registered`（已注册的 cron/interval 任务）、`scheduled`（未来待触发任务）、`stats`（worker 统计：总任务数/运行中/完成/失败/队列深度）。

#### Scenario: 查询活跃任务
- **WHEN** PHP 代码调用 `xhjob_inspect('active')`
- **THEN** 返回当前 `running` 状态的任务列表（id/type/state/started_at/progress）

#### Scenario: 查询 worker 统计
- **WHEN** PHP 代码调用 `xhjob_inspect('stats')`
- **THEN** 返回 `{total, pending, running, success, failed, queue_depth, uptime}` 聚合统计

### Requirement: 后台任务队列异步功能测试套件
系统 SHALL 在 `/workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php` 提供异步队列专项测试，覆盖：非阻塞派发、批量并发、进度查询、chord、countdown、inspect、队列深度、事件拉取。

#### Scenario: 全部测试通过
- **WHEN** 运行 `php -d extension=...so test_xhjob_async_queue.php`
- **THEN** 所有测试步骤 PASS，exit code 0
- **AND** 覆盖非阻塞派发（dispatch 立即返回 task_id）、批量并发（10 任务并发）、进度上报（shell 任务中调 report_progress）、chord（3 header + 1 callback）、countdown（3 秒延迟）、inspect（active/registered/scheduled/stats 四模式）、队列深度查询、事件拉取

### Requirement: APScheduler/Celery 特性矩阵评估文档
系统 SHALL 在 `/workspace/releases/xhjob-thinkphp8-extend/EVALUATION.md` 提供特性矩阵评估，逐项标注实现状态与优化建议。

#### Scenario: 评估文档完整
- **WHEN** 查阅 EVALUATION.md
- **THEN** 包含 APScheduler 特性矩阵（Triggers/Executors/Jobstores/Listeners/Job 控制/coalesce/misfire/max_instances/jitter/replace_existing/timezone/tags）
- **AND** 包含 Celery 特性矩阵（Queues/retry/timeout/expires/result/acks/rate_limit/chain/group/chord/chunks/map/starmap/events/inspect/control）
- **AND** 每项标注：✅已实现 / ⚠️部分实现 / ❌未实现 / ➖不适用（单机场景）
- **AND** 对未实现项给出是否值得实现的建议（基于单机 PHP 场景的适用性）

## MODIFIED Requirements

### Requirement: Task 模型新增字段
`Task` 结构新增 3 个字段（均 Option，向后兼容）：
- `progress: Option<u8>`（0-100，None 表示未上报）
- `progress_meta: Option<String>`（任意 JSON）
- `chord_id: Option<String>`（chord 回调关联）

`xhjob_state` 返回新增 `progress` / `progress_meta` 字段（已上报时才有值）。

### Requirement: TaskStore trait 新增方法
- `update_progress(id, percent: u8, meta: Option<String>)`：更新进度
- `list_active_summary()` → `Vec<TaskSummary>`：running 任务摘要（用于 inspect active）
- `list_registered_summary()` → `Vec<TaskSummary>`：cron/interval 注册任务摘要
- `list_scheduled_summary()` → `Vec<TaskSummary>`：next_fire > now 的待触发任务
- `worker_stats()` → `WorkerStats { total, pending, running, success, failed, queue_depth }`

## REMOVED Requirements
无（本 spec 仅新增，不删除任何现有能力）。
