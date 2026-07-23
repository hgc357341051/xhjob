# Checklist

## 字段与序列化
- [ ] `Task` 结构体含 `or_cron: Option<Vec<String>>`、`skip_dates: Vec<i64>`、`workdays_only: bool` 3 字段
- [ ] 所有 3 字段均带 `#[serde(default)]`，保证旧 JSON 反序列化不报错
- [ ] `Task::new()` 初始化 3 字段默认值（None / 空 Vec / false）
- [ ] `TaskSummary` 同步含 3 字段并带 `#[serde(default)]`
- [ ] `TaskSummary::from(&Task)` 拷贝 3 字段

## TaskBuilder
- [ ] `TaskBuilder` 结构体含 3 字段并带 `#[serde(default)]`
- [ ] `Default` impl 初始化 3 字段默认值
- [ ] 新增 `or_cron(Vec<String>) -> Self` 链式方法
- [ ] 新增 `skip_dates(Vec<i64>) -> Self` 链式方法
- [ ] 新增 `workdays_only() -> Self` 链式方法（无参，置 true）
- [ ] `build()` 拷贝 3 字段到 `Task`
- [ ] `build()` 中 `next_fire` 计算使用 `cron + or_cron` 合并集，过滤空串，取最小值
- [ ] 仅 `cron` 或仅 `or_cron` 时计算路径均正确

## 调度器
- [ ] 新增 `date_components_in_tz(ts, tz_str) -> Option<(i32, u32, u32, u32)>`（year/month/day/weekday_from_monday 0=Mon）
- [ ] 新增 `should_skip_trigger(task, now) -> bool` 实现 F-2 + F-3 合并
- [ ] `should_skip_trigger` 在 `skip_dates` 与 `workdays_only` 均未启用时快速返回 false（不触碰 chrono）
- [ ] `should_skip_trigger` 在时区解析失败或时间戳越界时 fail-open 返回 false
- [ ] F-2 比较按 (year, month, day) 三元组，忽略 weekday
- [ ] F-3 用 `weekday_from_monday >= 5` 判定周末
- [ ] 新增 `collect_cron_exprs(task) -> Vec<String>`（cron + 非空 or_cron）
- [ ] 新增 `min_next_fire_across(exprs, from, tz) -> Result<Option<u64>, XhjobError>`
- [ ] 新增 `has_cron_trigger(task) -> bool`（修复编译错误）
- [ ] runAt 分支：触发前 `should_skip_trigger` 检查
- [ ] interval 分支：用 `!has_cron_trigger(&task)` 替代 `task.cron.is_none()`
- [ ] interval 分支：触发前 `should_skip_trigger` 检查，跳过时仍滚动 next_fire
- [ ] cron 分支：用 `collect_cron_exprs` + `min_next_fire_across` 替换单一表达式
- [ ] cron 分支：触发前 `should_skip_trigger` 检查，跳过时仍滚动 next_fire
- [ ] 所有 `tracing::` 宏（warn / info / error）使用规范

## SQLite 持久化
- [ ] CREATE TABLE 语句含 `or_cron TEXT,` `skip_dates TEXT,` `workdays_only INTEGER NOT NULL DEFAULT 0,` 3 列
- [ ] 3 个 `ensure_column` 迁移调用（旧库自动升级）
- [ ] `task_from_row` 读取 or_cron：`serde_json::from_str` NULL→None
- [ ] `task_from_row` 读取 skip_dates：`serde_json::from_str` NULL→空 Vec
- [ ] `task_from_row` 读取 workdays_only：`!= 0` 转 bool
- [ ] `insert_task` 写入 or_cron：序列化为 JSON 字符串
- [ ] `insert_task` 写入 skip_dates：序列化为 JSON 字符串
- [ ] `insert_task` 写入 workdays_only：`as i64`
- [ ] `reschedule_task` 在 patch 含字段时更新对应列

## StateInfo 与 PHP API
- [ ] `StateInfo` 结构体含 3 字段并带 `#[serde(default)]`
- [ ] `from_task` 拷贝 3 字段
- [ ] `Xhjob` 类含 `orCron` / `skipDates` / `workdaysOnly` 3 链式方法
- [ ] `xhjob_dispatch` 透传新字段（builder 序列化自动携带）
- [ ] `xhjob_state` 返回值含 3 字段

## 约束与编码规范
- [ ] 所有新字段使用 `#[serde(default)]`，不破坏旧 JSON / 旧 SQLite 库加载
- [ ] 不修改任何既有公共 API 签名（仅新增方法）
- [ ] SQLite 自动迁移通过 `ensure_column`，无手工迁移脚本
- [ ] 4 空格缩进
- [ ] `tracing::` 宏使用规范（不使用 println / eprintln）
- [ ] 无未使用变量 / dead_code 警告

## 编译与测试
- [ ] `cargo build --all-features` 通过
- [ ] `cargo clippy --all-features -- -D warnings` 无警告
- [ ] `cargo test --all-features --lib` 全部通过（基线 139 项不回归）
