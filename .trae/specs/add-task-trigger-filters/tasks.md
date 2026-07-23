# Tasks

- [ ] Task 1: 在 `src/store/mod.rs` 的 `Task` 结构体 + `Task::new()` + `TaskSummary` + `TaskSummary::from` 中新增 `or_cron` / `skip_dates` / `workdays_only` 3 字段（均带 `#[serde(default)]`）
  - [ ] SubTask 1.1: 在 `Task` 结构体 `cron` 字段后新增 3 字段
  - [ ] SubTask 1.2: 在 `Task::new()` 初始化列表加入 3 字段默认值
  - [ ] SubTask 1.3: 在 `TaskSummary` 结构体加入同 3 字段
  - [ ] SubTask 1.4: 在 `TaskSummary::from(&Task)` impl 中拷贝 3 字段

- [ ] Task 2: 在 `src/task/mod.rs` 的 `TaskBuilder` 中新增 3 字段 + `Default` impl + 3 链式方法 + `build()` 联合 next_fire 计算
  - [ ] SubTask 2.1: 在 `TaskBuilder` 结构体 `cron` 字段后新增 3 字段
  - [ ] SubTask 2.2: 在 `Default` impl 初始化列表加入 3 字段默认值
  - [ ] SubTask 2.3: 在 `cron()` 方法后新增 `or_cron()` / `skip_dates()` / `workdays_only()` 3 链式方法
  - [ ] SubTask 2.4: 在 `build()` 中拷贝 3 字段到 `Task`，并把 `cron + or_cron` 合并为 `cron_exprs` 取最小 next_fire

- [ ] Task 3: 在 `src/scheduler/cron.rs` 中新增过滤 helper + 修改 `scan_once` 的 runAt / interval / cron 三个分支
  - [ ] SubTask 3.1: 新增 `date_components_in_tz(ts, tz_str) -> Option<(i32, u32, u32, u32)>`（返回 year/month/day/weekday_from_monday）
  - [ ] SubTask 3.2: 新增 `should_skip_trigger(task, now) -> bool`（F-2 + F-3 合并；fail-open on bad tz）
  - [ ] SubTask 3.3: 新增 `collect_cron_exprs(task) -> Vec<String>`（cron + 非空 or_cron）
  - [ ] SubTask 3.4: 新增 `min_next_fire_across(exprs, from, tz) -> Result<Option<u64>, XhjobError>`
  - [ ] SubTask 3.5: 新增 `has_cron_trigger(task) -> bool`（修复编译错误：被 interval 分支引用但未定义）
  - [ ] SubTask 3.6: 修改 runAt 分支：触发前检查 `should_skip_trigger`，跳过时仍设 `u64::MAX` 哨兵
  - [ ] SubTask 3.7: 修改 interval 分支：用 `!has_cron_trigger(&task)` 替代 `task.cron.is_none()`；触发前检查 skip，跳过时仍滚动 next_fire
  - [ ] SubTask 3.8: 修改 cron 分支：用 `collect_cron_exprs` + `min_next_fire_across` 替换单一表达式；触发前检查 skip，跳过时仍滚动 next_fire

- [ ] Task 4: 在 `src/store/sqlite.rs` 中新增 3 列 + 迁移 + 读写
  - [ ] SubTask 4.1: CREATE TABLE 语句新增 `or_cron TEXT, skip_dates TEXT, workdays_only INTEGER NOT NULL DEFAULT 0,`
  - [ ] SubTask 4.2: 新增 3 个 `ensure_column` 迁移调用（在 `init_db` / 表存在性检查后）
  - [ ] SubTask 4.3: `task_from_row` 读取 3 列（or_cron/skip_dates 用 `serde_json::from_str` 反序列化 NULL→None/空；workdays_only 用 `!= 0`）
  - [ ] SubTask 4.4: `insert_task` 写入 3 列（or_cron/skip_dates 序列化为 JSON 字符串；workdays_only 用 `as i64`）
  - [ ] SubTask 4.5: `reschedule_task` 在 patch 含该字段时更新对应列

- [ ] Task 5: 在 `src/outcome/mod.rs` 的 `StateInfo` 新增 3 字段 + `from_task` 拷贝
  - [ ] SubTask 5.1: `StateInfo` 结构体新增 `or_cron` / `skip_dates` / `workdays_only` 3 字段（带 `#[serde(default)]`）
  - [ ] SubTask 5.2: `from_task` 中拷贝 3 字段

- [ ] Task 6: 在 `src/lib.rs` 的 `Xhjob` 类新增 3 方法 + `xhjob_state` 透传
  - [ ] SubTask 6.1: `Xhjob` 类在 `cron()` 方法后新增 `orCron` / `skipDates` / `workdaysOnly` 3 个链式方法
  - [ ] SubTask 6.2: `xhjob_dispatch` 透传（builder JSON 序列化自动携带，无需额外代码改动，仅验证）
  - [ ] SubTask 6.3: `xhjob_state` 返回值含 3 字段（StateInfo 已携带，仅需确认 PHP 端数组映射）

- [ ] Task 7: 编译 + clippy + 测试验证，修复直至全绿
  - [ ] SubTask 7.1: `cargo build --all-features 2>&1 | tail -30` 通过
  - [ ] SubTask 7.2: `cargo clippy --all-features -- -D warnings 2>&1 | tail -30` 无警告
  - [ ] SubTask 7.3: `cargo test --all-features --lib 2>&1 | tail -30` 全部通过（基线 139 项不回归）

# Task Dependencies
- Task 1 → Task 2 → Task 3（cron.rs 依赖 Task 字段与 TaskBuilder）
- Task 1 → Task 4（sqlite 依赖 Task 字段）
- Task 1 → Task 5（StateInfo 依赖 Task 字段）
- Task 5 → Task 6（lib.rs xhjob_state 依赖 StateInfo）
- Task 2 → Task 6（lib.rs Xhjob 类方法依赖 TaskBuilder）
- Task 1..6 → Task 7（编译验证依赖所有改动落地）
- Task 3 中 SubTask 3.5 必须先于 SubTask 3.7（修复编译错误前置）
