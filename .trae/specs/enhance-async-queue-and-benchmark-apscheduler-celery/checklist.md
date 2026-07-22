# Checklist

## Task 模型 + TaskStore 扩展
- [x] `Task` 结构新增 `progress: Option<u8>` / `progress_meta: Option<String>` / `chord_id: Option<String>` 字段
- [x] `TaskSummary` 新增 `progress` / `chord_id`
- [x] `WorkerStats` 结构定义（total/pending/running/success/failed/queue_depth）
- [x] `TaskStore` trait 新增 `update_progress` / `list_active_summary` / `list_registered_summary` / `list_scheduled_summary` / `worker_stats` 方法
- [x] `InMemoryStore` 实现 5 个新方法
- [x] `SqliteStore` 实现 5 个新方法
- [x] sqlite tasks 表 CREATE TABLE 含 `progress` / `progress_meta` / `chord_id` 列
- [x] sqlite 通过 `ensure_column` 兼容旧库（缺列时 ALTER TABLE ADD COLUMN）
- [x] `xhjob_state` 返回 `progress` / `progress_meta` 字段（已上报时才有值）

## Countdown 便捷派发
- [x] `TaskBuilder` 新增 `countdown: Option<u64>` 字段与 `countdown(secs)` 链式方法
- [x] `build()` 中 countdown→run_at 归一化（countdown.is_some() 且 run_at.is_none() 时 run_at = now + countdown）
- [x] countdown 与 run_at 同时设置时发出 warn（run_at 优先）
- [x] PHP `TaskBuilder::countdown(int $s): self` 链式方法
- [x] 测试：countdown(3) 后前 3 秒 state=pending，3 秒后 running→success

## 任务进度上报
- [x] `xhjob_report_progress(id, percent, meta_json, name, data_dir) -> bool` 函数注册
- [x] daemon op `report_progress` 调用 `store.update_progress`，校验 percent ∈ [0,100]
- [x] xhjobctl `report-progress <id> <percent> [meta]` 子命令
- [x] PHP `TaskManager::reportProgress(string $id, int $percent, ?string $meta = null): bool`
- [x] 测试：shell 任务执行中调 report_progress(50)，xhjob_state 返回 progress=50
- [x] 测试：HTTP 任务无进度上报，progress 字段为 null/0，不影响完成

## 事件拉取
- [x] `xhjob_pull_events(since_ts, event_type, name, data_dir) -> String` 函数注册
- [x] daemon 端按 event_type 客户端过滤（EventType::as_str() 匹配）
- [x] PHP `TaskManager::pullEvents(int $sinceTs = 0, ?string $eventType = null): array`
- [x] 测试：拉取某 succeeded 事件返回非空数组
- [x] 测试：event_type=null 返回全部类型

## Inspect 聚合查询
- [x] `xhjob_inspect(mode, name, data_dir) -> String` 函数注册
- [x] daemon op `inspect` 按 mode 分支：active/registered/scheduled/stats
- [x] PHP `TaskManager::inspect(string $mode = 'stats'): array`
- [x] 测试：inspect('active') 返回 running 任务列表
- [x] 测试：inspect('registered') 返回 cron/interval 注册任务
- [x] 测试：inspect('scheduled') 返回 next_fire > now 的任务
- [x] 测试：inspect('stats') 返回 total/pending/running/success/failed/queue_depth 聚合

## Chord 原语
- [x] `src/scheduler/chord.rs` 定义 `refresh_state(store, chord_id) -> ChordRefreshResult`
- [x] `ChordRecord` 结构（id/header_task_ids/callback_json/callback_task_id/state/created_at/updated_at）
- [x] `TaskStore` 新增 `create_chord` / `get_chord` / `update_chord_state` 方法
- [x] InMemoryStore + SqliteStore 实现 chord 存储（sqlite 新增 chords 表）
- [x] queue `notify_chain_and_group` 检查 task.chord_id 并联动调用 chord::refresh_state（设计调整为在 queue 完成时检查，每个 header task 携带 chord_id）
- [x] 全部 header success 时派发回调任务，回调 meta 携带所有子任务结果
- [x] 任一 header failed/expired/cancelled 时 chord 转 partial_failed，不派发回调
- [x] `xhjob_chord(header_json, callback_json, name, data_dir) -> String` 函数注册
- [x] `xhjob_chord_state(chord_id, name, data_dir) -> Option<String` 函数注册
- [x] PHP `TaskBuilder::chord(array $headerBuilders, self $callback)` 静态工厂
- [x] PHP `TaskManager::createChord(array $headerBuilders, TaskBuilder $callback): string`
- [x] PHP `TaskManager::chordState(string $chordId): ?array`
- [x] 测试：chord refresh 全部 success 派发 callback（单元测试 test_refresh_all_success_dispatches_callback）
- [x] 测试：chord refresh 部分失败不派发 callback（单元测试 test_refresh_partial_failure_no_callback）
- [x] 测试：chord refresh 空 chord 返回 pending（单元测试 test_refresh_empty_chord_returns_pending）
- [x] 测试：chord refresh 进行中返回 running（单元测试 test_refresh_in_flight_returns_running）
- [x] 测试：chord 终态幂等（单元测试 test_refresh_terminal_chord_is_idempotent）
- [x] 编译验证：cargo check --features persist 通过（exit code 0）
- [x] 单元测试：cargo test --lib 全部 125 个通过（含 5 个 chord 测试）
- [x] PHP 语法检查：TaskBuilder.php / TaskManager.php / facade/Xhjob.php 全部通过

## 编译 + 部署
- [x] `cargo build --release --features persist` 编译通过（无 warning）
- [x] .so 部署到 `/workspace/releases/xhjob-php8.2-linux-x86_64.so`
- [x] .so 部署到 PHP 扩展目录（`/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so`）
- [x] 修改的 PHP 文件同步到 `/workspace/releases/xhjob-thinkphp8-extend/Xhjob/`（TaskBuilder.php / TaskManager.php）

## 后台任务队列异步功能测试套件
- [x] `/workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php` 创建
- [x] 测试步骤覆盖：非阻塞派发、批量并发、进度上报、countdown、chord 成功、chord 失败、inspect 四模式、pull_events
- [x] 运行测试全部 PASS，exit code 0
- [x] 无 PHP warning

## APScheduler/Celery 特性矩阵评估文档
- [x] `/workspace/releases/xhjob-thinkphp8-extend/EVALUATION.md` 创建
- [x] APScheduler 特性矩阵覆盖 15+ 项（Triggers/Executors/Jobstores/Listeners/Job 控制/coalesce/misfire/max_instances/jitter/replace_existing/timezone/tags/modify_job/remove_job/pause/resume）
- [x] Celery 特性矩阵覆盖 20+ 项（Queues/routing/retry/timeout/soft_timeout/expires/result/acks/rate_limit/chain/group/chord/chunks/map/starmap/events/inspect/control/countdown/eta/result_expires/track_started）
- [x] 每项标注 ✅已实现 / ⚠️部分实现 / ❌未实现 / ➖不适用（单机）
- [x] 对未实现项给出是否值得实现的建议（基于单机 PHP 场景适用性）
- [x] 末尾给出"单机 PHP 场景不值得实现"清单（多 broker / 集群 / 远程 worker / 多 result backend / 多 jobstore）

## 回归
- [x] 现有 `test_xhjob_deep.php` 29 步仍全部 PASS（新字段不破坏旧 API）
- [x] 现有 `test_xhjob_persist.php` 8 步仍全部 PASS（chord 表 / progress 列不破坏持久化）
- [x] cargo test 单元测试全部通过
