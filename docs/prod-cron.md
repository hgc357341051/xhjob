---
title: 生产实战：定时任务
parent: 生产实战
nav_order: 52
---

# 生产实战：定时任务

本篇给出一个**生产级定时任务**的完整端到端写法：每天早上 9 点跑报表，仅工作日执行、跳过法定节假日、漏触发合并为一次、崩溃后自动恢复、最多执行 1000 次后停止。涵盖 cron 表达式、时区、持久化、延迟确认、执行次数上限、合并、节假日跳过、仅工作日等全部可靠性配置。

## 两种 Builder API

xhjob 提供两种链式 Builder，本篇定时任务因需用到 `skipDates` / `workdaysOnly`，采用**原生扩展 `Xhjob` 类**（全局命名空间）演示：

| API | 命名空间 | 适用 | 是否支持 skipDates / workdaysOnly |
|------|------|------|------|
| 原生 `Xhjob` 类 | 全局（扩展导出） | 通用 PHP（CLI / FPM） | 支持（`skipDates()` / `workdaysOnly()`） |
| `Xhjob\TaskBuilder` | `Xhjob\`（thinkphp8-extend 包装层） | ThinkPHP 8 集成 / 纯 PHP | 支持常用子集（cron / withTimezone / persist / acksLate / maxExecutions / coalesce 等），暂未暴露 skipDates / workdaysOnly |

两种 API 底层都走同一个 daemon，常用方法名一致（`cron` / `withTimezone` / `persist` / `acksLate` / `maxExecutions` / `coalesce` / `withRetry` / `retryBackoff` / `misfireGraceTime` / `jitter`）。原生 `Xhjob` 类入口为 `Xhjob::task()`，链式配置后调 `dispatch()` 返回 task_id。

## 完整端到端示例

```php
<?php
// === 生产级定时任务：每日 09:00 跑报表 ===
// 先确保 daemon 已启动（幂等）
xhjob_start('cron-svc', '/var/lib/xhjob');

// 法定节假日（Unix 时间戳，取当天 00:00:00 Asia/Shanghai）
// 实际从节假日 API / 配置表加载
$holidays = [
    strtotime('2026-01-01 00:00:00'),   // 元旦
    strtotime('2026-02-17 00:00:00'),   // 春节
    strtotime('2026-04-04 00:00:00'),   // 清明
    strtotime('2026-05-01 00:00:00'),   // 劳动节
    strtotime('2026-06-19 00:00:00'),   // 端午
    strtotime('2026-09-25 00:00:00'),   // 中秋
    strtotime('2026-10-01 00:00:00'),   // 国庆
];

// 原生 Xhjob 类：支持全部调度字段，单链搞定
$taskId = Xhjob::task()
    ->service('cron-svc')                       // 绑定命名服务
    ->dataDir('/var/lib/xhjob')                 // 数据目录
    ->viaShell('php /app/jobs/daily-report.php') // shell 任务
    ->cron('0 9 * * *')                         // 每天 09:00 触发（5 字段）
    ->withTimezone('Asia/Shanghai')             // 时区：必须设置，否则用系统本地
    ->workdaysOnly()                            // 仅工作日（Mon-Fri），周末不触发
    ->skipDates($holidays)                      // 节假日跳过（按日历日期匹配）
    ->coalesce(true)                            // 漏触发合并为 1 次（而非补跑多次）
    ->misfireGraceTime(600)                     // 容忍迟到 10 分钟，超期则视为 misfire
    ->maxExecutions(1000)                       // 最多执行 1000 次后自动停止（0=无限）
    ->persist(true)                             // 持久化（SqliteStore + WAL），重启不丢
    ->acksLate(true)                            // 完成后才 ack，daemon 崩溃后重投
    ->acksOnFailure(false)                      // 失败时不 ack，便于重试
    ->withRetry(3, 60)                          // 失败重试 3 次，间隔 60 秒
    ->retryBackoff(true)                        // 指数退避：60,120,240... 上限 60*60
    ->timeout(1800)                             // 单次执行硬超时 30 分钟
    ->softTimeout(1740)                         // 软超时 29 分钟，先 SIGTERM 再 SIGKILL
    ->maxInstances(1)                           // 同任务最大并发 1（防重复跑）
    ->jitter(5)                                 // 随机抖动 5 秒，避免整点惊群
    ->tag('report')                             // 标签：便于检索
    ->tag('critical')
    ->dispatch();                               // 返回 task_id

echo "定时任务已创建: {$taskId}\n";

// 查询状态
$state = xhjob_state($taskId, 'cron-svc', '/var/lib/xhjob');
print_r($state);
```

## 配置项逐项说明

| 方法 | 作用 | 本例取值 |
|------|------|------|
| `cron('0 9 * * *')` | 5 字段 cron 表达式，每天 09:00 触发 | `0 9 * * *` |
| `withTimezone('Asia/Shanghai')` | cron 求值时区，**必须设置**否则用系统本地时区 | `Asia/Shanghai` |
| `workdaysOnly()` | 仅 Mon-Fri 触发，周末跳过 | 开启 |
| `skipDates([...])` | 跳过指定日历日期（按任务时区的年月日匹配） | 节假日时间戳数组 |
| `coalesce(true)` | 漏触发合并：多次错过只补跑 1 次 | 开启 |
| `misfireGraceTime(600)` | 容忍迟到的宽限时间（秒），超期视为 misfire | 600 |
| `maxExecutions(1000)` | 最大执行次数，达到后转 Success 终态；0=无限 | 1000 |
| `persist(true)` | 持久化任务状态到 SQLite + WAL，daemon 重启后恢复 | 开启 |
| `acksLate(true)` | 完成后才 ack；daemon 在 Running 期间崩溃，重启后重置为 Pending 重投 | 开启 |
| `acksOnFailure(false)` | 失败时不 ack，配合 acksLate 便于重试 | 关闭 |
| `withRetry(3, 60)` | 最多重试 3 次，基础间隔 60 秒 | 3, 60 |
| `retryBackoff(true)` | 指数退避：`min(60*2^(n-1), 60*60)` | 开启 |
| `maxInstances(1)` | 同任务最大并发实例数，防重复执行 | 1 |
| `jitter(5)` | 随机抖动 5 秒，避免整点惊群 | 5 |

### 漏触发合并（coalesce）工作流

当 daemon 因重启 / 阻塞错过了多次触发（例如每天 9 点的任务，daemon 在 9:00-11:00 宕机）：

- `coalesce(true)` + `misfireGraceTime(600)`：在宽限期（10 分钟）内恢复，只补跑 **1 次**，不补跑中间所有错过的次数。
- `coalesce(false)`：超出宽限期则直接跳过本次触发（`next_fire` 后滚到下一个匹配点）。

### 崩溃恢复（acksLate + persist）

1. 任务到点被调度执行，进入 `Running` 状态，**此时未 ack**。
2. daemon 在任务 Running 期间崩溃 / 被强制重启。
3. daemon 重启后扫描持久化存储：`acksLate=true` 且状态为 `Running` 的任务被**自动重置为 `Pending`** 并重新触发。
4. 普通任务（`acksLate=false`）的 Running 状态保持，不会重投（视为已确认）。

> 崩溃恢复要求任务**幂等**——重复执行不产生副作用（如报表可重复生成、订单用状态机去重）。

## cron 表达式：5 字段与 6 字段（秒级）

xhjob 的 cron 基于 `cron 0.12`，**内部要求 6 个字段**（秒 分 时 日 月 周）。用户传 5 字段时，扩展自动在前面补 `0` 秒字段：

```rust
// src/scheduler/cron.rs
// cron 0.12 requires 6 fields (sec min hour day month weekday).
// If the user supplied 5 fields we prepend a `0` seconds field.
```

| 字段数 | 示例 | 含义 |
|------|------|------|
| 5 字段 | `0 9 * * *` | 每天 09:00:00（自动补秒=0） |
| 5 字段 | `0 9 * * 1-5` | 工作日 09:00:00 |
| 6 字段 | `*/30 * * * * *` | 每 30 秒触发一次（秒级精度） |
| 6 字段 | `0 0 9 * * *` | 每天 09:00:00（显式秒=0） |

```php
// 秒级 cron：每 30 秒健康检查
Xhjob::task()
    ->service('cron-svc')
    ->viaShell('curl -s https://api.example.com/health')
    ->cron('*/30 * * * * *')      // 6 字段，秒级
    ->withTimezone('Asia/Shanghai')
    ->maxExecutions(0)            // 无限
    ->dispatch();
```

## 多 cron 表达式（or_cron）

`orCron([...])` 追加额外 cron 表达式，任务在**任一**表达式匹配时触发（并集）：

```php
Xhjob::task()
    ->viaShell('php /app/jobs/cleanup.php')
    ->cron('0 2 * * *')                         // 主表达式：每天 02:00
    ->orCron(['0 14 * * *', '0 22 * * 6'])      // 额外：每天 14:00 与 周六 22:00
    ->withTimezone('Asia/Shanghai')
    ->dispatch();
```

## 注意事项

| 关注点 | 说明 |
|------|------|
| **cron 6 字段支持秒级** | 传 5 字段自动补 `0` 秒；需秒级精度时显式写 6 字段（如 `*/30 * * * * *`）。 |
| **时区必须设置** | `withTimezone` 未设置时，cron 求值用 daemon 进程的**系统本地时区**。容器 / 服务器时区与业务时区不一致时（如容器 UTC、业务 Asia/Shanghai），会触发时间错位。生产环境务必显式 `withTimezone('Asia/Shanghai')`。 |
| **persist 需 --all-features** | 持久化（SqliteStore + WAL）依赖 `persist` cargo feature。编译扩展时需 `cargo build --features persist`（或 `--all-features`）；运行时还需 `XHJOB_POOL_MODE` 之外设 `XHJOB_PERSIST=1`。未启用 feature 时 `persist(true)` 退化为 InMemoryStore，daemon 重启后任务丢失。 |
| **skipDates 按日历日期匹配** | `skipDates` 接收 Unix 时间戳数组，但匹配时只比较**任务时区的年月日**（不比较时分秒）。传当天任意时间戳均可，建议传当天 00:00:00。 |
| **workdaysOnly 与 cron 1-5** | `workdaysOnly()` 与 cron 周字段 `1-5` 效果相近，但 `workdaysOnly` 是在触发求值后的二次过滤，可与任意 cron 表达式组合（如 `0 9 * * *` + `workdaysOnly`）。 |
| **maxInstances 防重复** | 长任务（如 30 分钟报表）若上一轮未跑完又到下一轮触发点，`maxInstances(1)` 会阻止重复执行，配合 `coalesce` 合并漏触发。 |
| **持久化 + acksLate 需幂等** | 崩溃恢复会重投 Running 任务，业务必须幂等。 |

## 运维查询

```bash
# 查看所有 cron 注册任务
php -r 'print_r(xhjob_inspect("registered", "cron-svc", "/var/lib/xhjob"));'

# 查看任务下次触发时间
php -r 'print_r(xhjob_inspect("scheduled", "cron-svc", "/var/lib/xhjob"));'

# 查看任务状态与执行次数
php -r 'print_r(xhjob_state("TASK_ID", "cron-svc", "/var/lib/xhjob"));'

# 重新调度（改 cron 表达式）
php -r 'var_dump(xhjob_reschedule("TASK_ID", "0 10 * * *", "cron-svc", "/var/lib/xhjob"));'

# 暂停 / 恢复
php -r 'var_dump(xhjob_pause("TASK_ID", "cron-svc", "/var/lib/xhjob"));'
php -r 'var_dump(xhjob_resume("TASK_ID", "cron-svc", "/var/lib/xhjob"));'
```
