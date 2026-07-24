---
title: 触发器
parent: 核心能力
nav_order: 31
---

# 触发器

Xhjob 提供四种基础触发器（cron / interval / runAt / countdown）以及一组触发增强选项（or_cron / skip_dates / workdays_only / jitter / misfire_grace_time / start_date / end_date）。所有触发器通过 `TaskBuilder` 链式方法配置，最终由 daemon 内的调度器（`src/scheduler/`）统一计算 `next_fire` 并派发。

触发器之间的优先级关系：`run_at` > `cron` > `interval`。同时设置多个时，优先级高的生效，其余被忽略并记录一条 warning。

---

## cron（周期表达式）

cron 是最常用的周期触发器，支持标准的 **5 字段**与扩展的 **6 字段**两种写法：

- **5 字段**：`分 时 日 月 周`（标准 Unix cron），解析器会自动在最前面补一个 `0` 秒字段，等价于"整秒触发"。
- **6 字段**：`秒 分 时 日 月 周`，支持秒级精度。

> 解析逻辑位于 `src/scheduler/cron.rs`。底层依赖 `cron 0.12`，该库要求 6 字段输入；当用户传入 5 字段时，Xhjob 自动 prepend `0` 作为秒字段。

### 常用表达式示例

| 表达式 | 含义 |
|--------|------|
| `*/5 * * * *` | 每 5 分钟 |
| `0 3 * * *` | 每天 03:00 |
| `0 0 * * 0` | 每周日 00:00 |
| `0 */2 * * *` | 每 2 小时整点 |
| `0 9 * * 1-5` | 工作日（周一至周五）09:00 |
| `30 7 1 * *` | 每月 1 号 07:30 |
| `0 0 1 1 *` | 每年 1 月 1 日 00:00 |
| `*/30 * * * * *` | 每 30 秒（6 字段，秒级精度） |

### 时区

cron 默认按系统本地时区匹配。通过 `withTimezone()` 可指定 IANA 时区名（如 `Asia/Shanghai`、`America/New_York`），daemon 会用该时区解释 `next_fire`。无效时区名会在 `dispatch()` 阶段 fail-fast 抛错。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 每天凌晨 3 点执行备份脚本，使用上海时区，启用持久化
$taskId = TaskBuilder::shell('backup.sh')
    ->cron('0 3 * * *')
    ->withTimezone('Asia/Shanghai')
    ->persist(true)
    ->dispatch();
```

```php
// 秒级精度：每 30 秒触发一次健康检查
TaskBuilder::shell('healthcheck.sh')
    ->cron('*/30 * * * * *')
    ->dispatch();
```

---

## interval（固定间隔）

`every(n)` 让任务以固定 `n` 秒的间隔重复触发，首次立即触发，参考 APScheduler `IntervalTrigger`。

- 与 `cron` / `run_at` 互斥；同时设置时，`cron` / `run_at` 优先，`interval` 被忽略并记录 warning。
- `every(0)` 等价于未设置（无意义，会被归一化为 None）。

### 代码演示

```php
// 每 60 秒拉取一次队列
TaskBuilder::shell('pull-queue.sh')
    ->every(60)
    ->dispatch();
```

---

## runAt（一次性绝对时间戳）

`runAt($ts)` 接收一个 **Unix 绝对时间戳**（秒），任务在该时刻触发一次后立即进入 `Success` 终态，参考 APScheduler `DateTrigger`。

- 优先级最高，覆盖 `cron` 与 `interval`。
- 适合"在某个具体时刻执行一次"的场景，如活动开始、定时提醒。

### 代码演示

```php
// 5 分钟后执行一次
TaskBuilder::shell('notify.sh')
    ->runAt(time() + 300)
    ->dispatch();
```

---

## countdown（相对延迟秒数）

`countdown($secs)` 接收一个 **相对延迟秒数**，语义等价于 `run_at(now + countdown)`，参考 Celery `apply_async(countdown=N)`。

- 与 `run_at` 互斥；**当两者同时设置时 `run_at` 优先**，并记录一条 warning，`countdown` 被忽略。
- `countdown(0)` 视为未设置（无延迟）。
- 内部实现：`build()` 时若 `run_at` 未设置但 `countdown` 已设置，则转换为 `run_at = now + countdown`，复用 DateTrigger 路径。

### 代码演示

```php
// 60 秒后执行
TaskBuilder::shell('cleanup.sh')
    ->countdown(60)
    ->dispatch();
```

---

## or_cron（多表达式并集）

`orCron(array $exprs)` 追加额外的 cron 表达式，任务在 **任意一个**（主 `cron` + `or_cron` 列表）匹配时触发，参考 APScheduler CronTrigger 的 or-expr 组合。

- `None` 或空数组 = 无附加表达式。
- 适合"每天 1 点和 13 点各跑一次"这类多时段需求，避免拆成两个任务。

> **注意**：`orCron()` 仅在 Rust 扩展导出的 `Xhjob` 类上可用（`Xhjob::task()->viaShell(...)->orCron(...)`），PHP 纯类 `Xhjob\TaskBuilder` 暂未封装此方法。

### 代码演示

```php
// 每天 01:00 和 13:00 各执行一次报表生成
Xhjob::task()
    ->viaShell('report.sh')
    ->orCron(['0 1 * * *', '0 13 * * *'])
    ->dispatch();
```

---

## skip_dates + workdays_only（跳过与工作日限制）

- `skipDates(array $dates)`：传入一组 Unix 时间戳列表，其所在日历日（按任务时区）会被 **跳过**，不触发。空数组 = 不跳过。
- `workdaysOnly()`：设为 true 后，任务仅在 **周一至周五** 触发，周末跳过。

两者可组合使用，例如工作日运行但跳过法定节假日。

> **注意**：`skipDates()` / `workdaysOnly()` 仅在 Rust 扩展导出的 `Xhjob` 类上可用（`Xhjob::task()->viaShell(...)->workdaysOnly()`），PHP 纯类 `Xhjob\TaskBuilder` 暂未封装这两个方法。

### 代码演示

```php
// 工作日每天 9 点发晨报，但跳过节假日列表（时间戳）
$holidays = [1735660800, 1735747200]; // 2025-01-01, 2025-01-02 等
Xhjob::task()
    ->viaShell('morning-report.sh')
    ->cron('0 9 * * *')
    ->workdaysOnly()
    ->skipDates($holidays)
    ->dispatch();
```

---

## jitter + misfire_grace_time（抖动与迟到宽限）

### jitter（随机抖动）

`jitter($secs)` 给每次 `next_fire` 叠加一个 `[0, secs]` 范围内的随机偏移，参考 APScheduler `jitter`。用于 **防止惊群**——大量同时触发的任务被均匀打散，避免瞬时负载尖峰。

### misfire_grace_time（迟到宽限）

`misfireGraceTime($secs)` 设定单任务的迟到宽限窗口（秒）。当 `now - next_fire > grace_time` 时，该次触发被判定为 misfire（错过）：

- `coalesce=true`（默认）：错过的触发合并为一次，仍执行一次。
- `coalesce=false`：错过的触发直接丢弃（跳过执行），`next_fire` 滚动到下一个周期。
- `0` = 使用全局默认值 **60 秒**（`MISFIRE_GRACE_TIME_SECS`，定义于 `src/scheduler/cron.rs`）。

### 代码演示

```php
// 每 5 分钟触发，加 0-10 秒抖动防惊群，迟到超过 30 秒则视为 misfire
TaskBuilder::shell('poll.sh')
    ->cron('*/5 * * * *')
    ->jitter(10)
    ->misfireGraceTime(30)
    ->dispatch();
```

---

## start_date / end_date（生效区间）

`startAt($ts)` 与 `endAt($ts)` 设定任务的 **生效时间区间**（Unix 时间戳），参考 APScheduler `start_date` / `end_date`：

- 早于 `start_date` 的触发会被跳过，等到 `start_date` 到达后才正常派发。
- 超过 `end_date` 后任务不再触发。

> 对"仅设置 `start_date` 而无 cron/interval/run_at"的一次性任务尤为关键：daemon 会将其延迟重新入队，等到 `start_date` 到达后再派发，避免被消费后永远丢失。

### 代码演示

```php
// 仅在 2025-08-01 ~ 2025-08-31 期间每天 0 点运行活动结算
$start = mktime(0, 0, 0, 8, 1, 2025);
$end   = mktime(23, 59, 59, 8, 31, 2025);
TaskBuilder::shell('settle.sh')
    ->cron('0 0 * * *')
    ->startAt($start)
    ->endAt($end)
    ->dispatch();
```
