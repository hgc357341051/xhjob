# 任务触发器过滤功能（or_cron / skip_dates / workdays_only）Spec

## Why
当前 xhjob 调度器仅支持单一 cron 表达式触发，且无法在指定日期或周末跳过触发。对标 APScheduler 的 CronTrigger，需要补齐"多表达式 OR 触发"、"按日历日跳过日期"、"工作日限定"三项过滤能力，以支持"工作日每天 9 点但跳过节假日"这类企业级常见调度场景。三个功能均修改 `Task` 模型、`TaskBuilder`、cron 调度器、SQLite 持久化、StateInfo、PHP API 等相似代码路径，必须作为一组原子变更同时落地以避免接口错位与编译冲突。

## What Changes
- **F-1 or_cron**：`Task` 新增 `or_cron: Option<Vec<String>>` 字段；`TaskBuilder.or_cron(Vec<String>)` 链式方法；调度器把 `cron + or_cron` 合并为触发集，任一表达式匹配即触发，`next_fire` 取所有表达式最近触发的最小值
- **F-2 skip_dates**：`Task` 新增 `skip_dates: Vec<i64>` 字段（Unix 时间戳列表）；`TaskBuilder.skip_dates(Vec<i64>)` 链式方法；调度器在任务时区下按日历日（年-月-日）匹配跳过触发，跳过时 `next_fire` 仍向前滚动以避免反复评估同一触发时刻
- **F-3 workdays_only**：`Task` 新增 `workdays_only: bool` 字段；`TaskBuilder.workdays_only()` 链式方法；调度器在任务时区下判定周末（Sat/Sun）即跳过触发，跳过时同样向前滚动 `next_fire`
- **持久化**：SQLite `tasks` 表新增 `or_cron TEXT`、`skip_dates TEXT`、`workdays_only INTEGER NOT NULL DEFAULT 0` 三列，旧库通过 `ensure_column` 自动迁移
- **状态查询**：`StateInfo` 新增上述 3 字段，`xhjob_state` 返回值透传
- **PHP API**：`Xhjob` 类新增 `orCron` / `skipDates` / `workdaysOnly` 3 个链式方法，经 `TaskBuilder` JSON 序列化透明传递到 daemon
- **BREAKING**：无（所有新字段加 `#[serde(default)]`，仅新增方法，不修改既有公共 API 签名；旧 JSON / 旧 SQLite 库可无升级直接加载）

## Impact
- Affected specs：
  - `implement-async-task-scheduler`（Cron 调度器在 `next_fire` 计算路径上叠加过滤层）
  - `enhance-async-queue-and-benchmark-apscheduler-celery`（`TaskBuilder` / `Task` / `StateInfo` / SQLite schema 沿同一套字段加列模式扩展）
- Affected code：
  - `src/store/mod.rs`：`Task` 结构体 + `Task::new()` + `TaskSummary` + `TaskSummary::from(&Task)`
  - `src/task/mod.rs`：`TaskBuilder` 结构体 + `Default` impl + 3 个链式方法 + `build()` 中 `next_fire` 联合计算
  - `src/scheduler/cron.rs`：新增 4 个 helper（`date_components_in_tz` / `should_skip_trigger` / `collect_cron_exprs` / `min_next_fire_across` / `has_cron_trigger`）；`scan_once` 的 runAt / interval / cron 三个分支均接过滤逻辑
  - `src/store/sqlite.rs`：CREATE TABLE 加 3 列；3 个 `ensure_column` 迁移；`task_from_row` 读取；`insert_task` 写入；`reschedule_task` 更新
  - `src/outcome/mod.rs`：`StateInfo` 加 3 字段 + `from_task` 拷贝
  - `src/lib.rs`：`Xhjob` 类加 3 方法；`xhjob_state` 返回新字段
  - `src/store/in_memory.rs`：无需改动（直接 clone `Task`，新字段随 clone 携带）

## ADDED Requirements

### Requirement: F-1 or_cron 多表达式 OR 触发
系统 SHALL 支持 `or_cron(Vec<String>)` 链式方法与 `Task.or_cron` 字段。当任务同时设置 `cron` 与/或 `or_cron` 时，调度器把所有非空表达式合并为触发集；任一表达式匹配即触发任务；`next_fire` 取所有表达式下一次触发时间的最小值。空字符串表达式被静默忽略。仅设置 `or_cron` 而不设置 `cron` 亦有效。

#### Scenario: 仅 or_cron 多表达式取最近触发
- **WHEN** PHP 调用 `TaskBuilder::shell('echo a')->or_cron(vec!["0 9 * * *", "0 21 * * *"])->dispatch()`
- **THEN** `next_fire` 为 9:00 与 21:00 中较早者
- **AND** 两个时间点都会触发（每日两次）

#### Scenario: cron + or_cron 合并去重空串
- **WHEN** `cron = "*/5 * * * *"`，`or_cron = ["", "0 * * * *"]`
- **THEN** 调度集为 `["*/5 * * * *", "0 * * * *"]`（空串被过滤）
- **AND** `next_fire` 取两者最小值

#### Scenario: 仅 cron 时行为不变
- **WHEN** 任务仅设置 `cron`，`or_cron` 为 `None` 或空 `Vec`
- **THEN** 行为与未引入 or_cron 前完全一致

### Requirement: F-2 skip_dates 按日历日跳过
系统 SHALL 支持 `skip_dates(Vec<i64>)` 链式方法与 `Task.skip_dates` 字段。每个元素为 Unix 时间戳，调度器在任务时区下将时间戳转换为 (年, 月, 日)，与当前触发时刻 `now` 的 (年, 月, 日) 比较；匹配则跳过本次触发，并将 `next_fire` 向前滚动到下一触发点（避免反复评估同一被跳过时刻）。`skip_dates` 为空时无影响。

#### Scenario: 工作日 9 点但跳过指定节假日
- **WHEN** `cron = "0 9 * * 1-5"`，`skip_dates = [<某周三节假日 9:00 的 ts>]`
- **THEN** 该周三 9:00 不触发
- **AND** 下一触发点（次日 9:00）的 `next_fire` 被正确滚动
- **AND** 非节假日工作日照常触发

#### Scenario: 跨时区日历匹配
- **WHEN** 任务 `timezone = "Asia/Shanghai"`，`skip_dates` 含 `1735660800`（对应上海 2025-01-01 00:00）
- **THEN** 上海时间 2025-01-01 全天触发均被跳过（按年月日匹配，与时分秒无关）

#### Scenario: skip_dates 为空时无影响
- **WHEN** 任务未调用 `skip_dates` 或传入空 `Vec`
- **THEN** 行为与未引入该功能前完全一致

### Requirement: F-3 workdays_only 工作日限定
系统 SHALL 支持 `workdays_only()` 链式方法与 `Task.workdays_only: bool` 字段。当为 `true` 时，调度器在任务时区下判定 `now` 的星期；若为周六或周日（chrono `num_days_from_monday() >= 5`）则跳过本次触发并将 `next_fire` 向前滚动。`false`（默认）时无影响。

#### Scenario: 仅工作日触发的 cron
- **WHEN** `cron = "0 9 * * *"`，`workdays_only = true`
- **THEN** 周一至周五 9:00 触发
- **AND** 周六周日的 9:00 不触发，`next_fire` 滚动到下周一 9:00

#### Scenario: workdays_only=false 时无影响
- **WHEN** 任务未调用 `workdays_only()`
- **THEN** 周末照常触发，行为与未引入该功能前完全一致

### Requirement: 时区解析失败时的 fail-open 语义
系统 SHALL 在 `skip_dates` 或 `workdays_only` 启用但任务 `timezone` 字符串无法被 `chrono-tz` 解析（或时间戳超出范围）时，对本次触发**不跳过**（fail-open），并应通过 `tracing::` 记录告警。此约束确保一个错误的时区配置不会静默吞掉所有触发。

#### Scenario: 无效时区不跳过
- **WHEN** 任务 `timezone = "Invalid/Zone"`，`workdays_only = true`，触发时刻为周日
- **THEN** 调度器**不跳过**本次触发（fail-open）
- **AND** 不会因为时区错误导致任务永久静默

### Requirement: 跳过时仍滚动 next_fire
系统 SHALL 在 `should_skip_trigger` 返回 `true` 时，对所有触发类型（runAt / interval / cron）将 `next_fire` 向前滚动到下一触发点，而不是保留在当前已过时刻。此约束确保被跳过的触发不会被同一时刻反复评估，且任务在跳过后能继续参与后续调度。

#### Scenario: skip 后 cron 任务继续调度
- **WHEN** `cron = "0 9 * * *"`，`skip_dates` 含今日 9:00，扫描在今日 9:00 触发
- **THEN** 本次不触发，`next_fire` 滚动到明日 9:00
- **AND** 下一扫描 tick 不会再评估今日 9:00

### Requirement: cron / or_cron 触发集对 interval 的优先级
系统 SHALL 在任务同时设置 `interval` 与（`cron` 或非空 `or_cron`）时，以 cron 触发集为准，interval 被静默忽略（与既有"cron 优先于 interval"语义一致，但触发集扩展到 `or_cron`）。仅当触发集为空时才走 interval 分支。

#### Scenario: 同时设置 cron 与 interval
- **WHEN** `cron = "0 9 * * *"`，`interval = Some(60)`，`or_cron = None`
- **THEN** 走 cron 分支，interval 被忽略
- **AND** 与既有行为一致

#### Scenario: 仅 or_cron 时 interval 仍被忽略
- **WHEN** `cron = None`，`or_cron = Some(["0 9 * * *"])`，`interval = Some(60)`
- **THEN** 走 cron 分支（触发集非空），interval 被忽略

#### Scenario: 仅 interval 时行为不变
- **WHEN** `cron = None`，`or_cron = None`，`interval = Some(60)`
- **THEN** 走 interval 分支，每 60 秒触发

### Requirement: SQLite 自动迁移
系统 SHALL 在 SQLite 启用时通过 `ensure_column` 自动为旧库补齐 `or_cron` / `skip_dates` / `workdays_only` 三列，无需手工迁移脚本。新增列默认值：`or_cron = NULL`，`skip_dates = NULL`（读回时回退为空 `Vec`），`workdays_only = 0`（读回为 `false`）。

#### Scenario: 旧库升级
- **WHEN** daemon 启动时检测到 SQLite 库缺少 `workdays_only` 列
- **THEN** 自动 `ALTER TABLE tasks ADD COLUMN workdays_only INTEGER NOT NULL DEFAULT 0`
- **AND** 既有任务的 `workdays_only` 读回为 `false`
- **AND** 既有任务的 `or_cron` / `skip_dates` 读回为 `None` / 空 `Vec`
- **AND** 不破坏既有任务调度

## MODIFIED Requirements

### Requirement: Task 模型新增 3 字段
`Task` 结构在 `cron` 字段后新增 3 个字段（均带 `#[serde(default)]` 向后兼容）：
- `or_cron: Option<Vec<String>>`（None 或空 Vec 表示无附加表达式）
- `skip_dates: Vec<i64>`（空 Vec 表示无跳过）
- `workdays_only: bool`（false 表示不限定）

`Task::new()` 默认值为 `or_cron: None, skip_dates: Vec::new(), workdays_only: false`。
`TaskSummary` 同步新增 3 字段并经 `from(&Task)` 拷贝。

### Requirement: StateInfo 新增 3 字段
`StateInfo` 结构新增 `or_cron` / `skip_dates` / `workdays_only` 3 字段（均带 `#[serde(default)]`），`from_task` 同步拷贝。`xhjob_state` PHP 返回值透传这 3 字段，使调度配置在状态查询时可见。

### Requirement: TaskBuilder 链式方法
`TaskBuilder` 新增 3 个链式方法（均返回 `Self`，不修改既有方法签名）：
- `or_cron(Vec<String>) -> Self`：设置 `or_cron = Some(exprs)`
- `skip_dates(Vec<i64>) -> Self`：设置 `skip_dates = dates`
- `workdays_only() -> Self`：设置 `workdays_only = true`

`build()` 中将 3 字段从 builder 拷贝到 `Task`；`next_fire` 计算路径用 `collect_cron_exprs` 联合 `cron` 与非空 `or_cron` 取最小值。

## REMOVED Requirements
无（本 spec 仅新增，不删除任何现有能力）。
