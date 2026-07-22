# Tasks

- [x] Task 1: Rust Task 模型 + TaskStore trait 扩展（chord/progress/countdown 基础）
  - [x] SubTask 1.1: `src/store/mod.rs` `Task` 新增 `progress: Option<u8>` / `progress_meta: Option<String>` / `chord_id: Option<String>` 字段；`TaskSummary` 新增 `progress` / `chord_id`；新增 `WorkerStats` 结构
  - [x] SubTask 1.2: `src/store/mod.rs` `TaskStore` trait 新增方法：`update_progress` / `list_active_summary` / `list_registered_summary` / `list_scheduled_summary` / `worker_stats`
  - [x] SubTask 1.3: `src/store/in_memory.rs` 实现上述 5 个新方法
  - [x] SubTask 1.4: `src/store/sqlite.rs` 实现上述 5 个新方法；tasks 表 CREATE TABLE 新增 `progress` / `progress_meta` / `chord_id` 列；通过 `ensure_column` 兼容旧库
  - [x] SubTask 1.5: `src/lib.rs` `xhjob_state` 返回新增 `progress` / `progress_meta` 字段（已上报时才有值）

- [x] Task 2: Countdown 便捷派发
  - [x] SubTask 2.1: `src/task/mod.rs` `TaskBuilder` 新增 `countdown: Option<u64>` 字段（`#[serde(default)]`）与 `countdown(secs)` 链式方法
  - [x] SubTask 2.2: `TaskBuilder::build()` 中若 `countdown.is_some()` 且 `run_at.is_none()`，则 `run_at = Some(now_ts() + countdown)`，并发出 warn 提示 countdown 与 run_at 同时设置时 run_at 优先
  - [x] SubTask 2.3: `src/lib.rs` 无需新增函数（countdown 走 dispatch 路径）；PHP 端 `TaskBuilder::countdown` 链式方法
  - [x] SubTask 2.4: `/workspace/tp/extend/Xhjob/TaskBuilder.php` 新增 `countdown(int $s): self` 链式方法

- [x] Task 3: 任务进度上报（report_progress）
  - [x] SubTask 3.1: `src/lib.rs` 新增 `xhjob_report_progress(id: String, percent: i64, meta_json: Option<String>, name: Option<String>, data_dir: Option<String>) -> bool`
  - [x] SubTask 3.2: `src/daemon_main.rs` 新增 op `report_progress`，调用 `store.update_progress`，校验 percent 在 0-100
  - [x] SubTask 3.3: shell 任务执行体可通过 `xhjobctl report-progress <id> <percent> [meta]` 子命令上报（已有 xhjobctl CLI）
  - [x] SubTask 3.4: `/workspace/tp/extend/Xhjob/TaskManager.php` 新增 `reportProgress(string $id, int $percent, ?string $meta = null): bool`

- [x] Task 4: 事件拉取接口（pull_events）
  - [x] SubTask 4.1: `src/lib.rs` 新增 `xhjob_pull_events(since_ts: i64, event_type: Option<String>, name: Option<String>, data_dir: Option<String>) -> String`（返回 JSON 数组）
  - [x] SubTask 4.2: 复用 `store.list_events(since_ts, task_id_filter)`，在 daemon 端按 event_type 客户端过滤（事件类型字符串匹配 `EventType::as_str()`）
  - [x] SubTask 4.3: `/workspace/tp/extend/Xhjob/TaskManager.php` 新增 `pullEvents(int $sinceTs = 0, ?string $eventType = null): array`

- [x] Task 5: Inspect 聚合查询
  - [x] SubTask 5.1: `src/lib.rs` 新增 `xhjob_inspect(mode: String, name: Option<String>, data_dir: Option<String>) -> String`（返回 JSON）
  - [x] SubTask 5.2: `src/daemon_main.rs` 新增 op `inspect`，按 mode 分支调用 `list_active_summary` / `list_registered_summary` / `list_scheduled_summary` / `worker_stats`
  - [x] SubTask 5.3: `/workspace/tp/extend/Xhjob/TaskManager.php` 新增 `inspect(string $mode = 'stats'): array`

- [x] Task 6: Chord 原语（group + 回调）
  - [x] SubTask 6.1: 新增 `src/scheduler/chord.rs`，定义 `refresh_state(store, chord_id) -> ChordRefreshResult`：若全部 header success 则派发回调任务（meta 携带所有子任务结果），若任一 failed/expired/cancelled 则 chord 转为 `partial_failed` 终态
  - [x] SubTask 6.2: `src/store/mod.rs` 新增 `ChordRecord { id, header_task_ids: Vec<String>, callback_json: String, callback_task_id: Option<String>, state, created_at, updated_at }` 与 `create_chord` / `get_chord` / `update_chord_state` 方法
  - [x] SubTask 6.3: `src/store/in_memory.rs` + `src/store/sqlite.rs` 实现 chord 存储（sqlite 新增 chords 表）
  - [x] SubTask 6.4: `src/scheduler/queue.rs` `notify_chain_and_group` 中检查 `task.chord_id`，调用 `chord::refresh_state`（设计调整：在 queue 完成时联动而非 group.rs，每个 header task 携带 chord_id）
  - [x] SubTask 6.5: `src/lib.rs` 新增 `xhjob_chord(header_json: String, callback_json: String, name, data_dir) -> String` 与 `xhjob_chord_state(chord_id, name, data_dir) -> Option<String>`
  - [x] SubTask 6.6: `src/daemon_main.rs` 新增 op `chord`：解析 header（TaskBuilder 数组）+ callback（单个 TaskBuilder），派发所有 header 任务，创建 chord 记录关联 header 与 callback 配置
  - [x] SubTask 6.7: `/workspace/tp/extend/Xhjob/TaskBuilder.php` 新增 `chord(array $headerBuilders, self $callbackBuilder): self` 静态工厂与 `dispatch` 路由到 `xhjob_chord`
  - [x] SubTask 6.8: `/workspace/tp/extend/Xhjob/TaskManager.php` 新增 `createChord(array $headerBuilders, TaskBuilder $callback): string` 与 `chordState(string $chordId): ?array`

- [x] Task 7: 编译 + 部署
  - [x] SubTask 7.1: `cargo build --release --features persist` 编译通过
  - [x] SubTask 7.2: 部署 .so 到 `/workspace/releases/xhjob-php8.2-linux-x86_64.so` 与 PHP 扩展目录
  - [x] SubTask 7.3: 同步修改的 PHP 文件到 `/workspace/releases/xhjob-thinkphp8-extend/Xhjob/`

- [x] Task 8: 后台任务队列异步功能测试套件
  - [x] SubTask 8.1: 创建 `/workspace/releases/xhjob-thinkphp8-extend/test_xhjob_async_queue.php`，覆盖 12 步：
    1. 启动 daemon
    2. 非阻塞派发（dispatch 立即返回 task_id，不等执行）
    3. 批量并发派发（10 任务并发，验证全部 success）
    4. 进度上报（shell 任务中调 report_progress，验证 state.progress 字段）
    5. countdown 延迟派发（3 秒后触发）
    6. chord 全部成功（3 header + 1 callback，验证回调 meta 含 3 结果）
    7. chord 部分失败（1 header 失败，chord 转 partial_failed，回调不触发）
    8. inspect active（查询运行中任务）
    9. inspect registered（查询 cron 注册任务）
    10. inspect scheduled（查询未来触发任务）
    11. inspect stats（查询聚合统计）
    12. pull_events（拉取事件列表）
    13. 停止 daemon
  - [x] SubTask 8.2: 运行测试套件，全部 PASS，exit code 0

- [x] Task 9: APScheduler/Celery 特性矩阵评估文档
  - [x] SubTask 9.1: 创建 `/workspace/releases/xhjob-thinkphp8-extend/EVALUATION.md`，包含 APScheduler 特性矩阵（15+ 项）与 Celery 特性矩阵（20+ 项）
  - [x] SubTask 9.2: 每项标注 ✅/⚠️/❌/➖ 与优化建议
  - [x] SubTask 9.3: 末尾给出"单机 PHP 场景不值得实现"的清单（如多 broker / 集群 / 远程 worker）

# Task Dependencies
- Task 2、Task 3、Task 4、Task 5 可并行，均依赖 Task 1（Task 模型扩展）
- Task 6 依赖 Task 1（chord_id 字段）+ 现有 group 模块
- Task 7 依赖 Task 2-6 全部完成
- Task 8 依赖 Task 7（需新 .so 部署后运行）
- Task 9 独立，可与 Task 2-6 并行（纯文档）
