# 触发器（Triggers）

> 核心能力篇 · cron / every / runAt / countdown / orCron / skipDates / workdaysOnly / jitter / misfireGraceTime / startAt / endAt

触发器决定一个任务「什么时候被派发」。Xhjob 的 `TaskBuilder` 提供了一组链式方法，覆盖定时（cron）、固定间隔（every）、一次性（runAt/countdown）、并集多表达式（orCron）、跳过日期（skipDates）、仅工作日（workdaysOnly）等场景。所有触发器方法均返回 `self`，可链式叠加。

本文档按 **签名 → 参数表 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构逐项展开，并在末尾给出触发器互斥规则表与全局注意事项。

> 所有示例基于 `Xhjob\TaskBuilder`（ThinkPHP 扩展包）/ Rust `Xhjob` 链式 Builder（PHP 侧 camelCase 调用）。`orCron` / `skipDates` / `workdaysOnly` 由 Rust 端 `or_cron` / `skip_dates` / `workdays_only` 经 snake→camel 自动转换暴露，命名一致。

---

## 1. cron(string $expr) — Cron 表达式触发

### 签名

```php
// TaskBuilder（self 链式）
public function cron(string $expr): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$expr` | `string` | 是 | cron 表达式。标准为 5 字段（分 时 日 月 周）；Xhjob 也支持 6 字段秒级（秒 分 时 日 月 周） |

### 行为说明

`cron` 设置一个按周期匹配的触发器。调度器在每次扫描时，按任务时区（`withTimezone`，未设则用系统本地时区）计算「当前时间之后的下一次匹配时刻」作为 `next_fire`，到点即派发任务，并重新计算下一次。

- **5 字段标准表达式**：字段顺序为 `分 时 日 月 周`，与 Linux `crontab` 一致。
- **6 字段秒级表达式**：字段顺序为 `秒 分 时 日 月 周`。当表达式按空格切分后字段数 ≥ 6 时，按 6 字段解析；不足 6 字段时自动在前面补 `0` 秒字段（即 5 字段等价于秒位恒为 0）。
- **多表达式并集**：通过 `orCron` 追加额外表达式，触发集合为 `cron` 与 `orCron` 的并集（取最近一次匹配）。

#### 5 字段示例表

| 表达式 | 含义 |
|---|---|
| `0 * * * *` | 每小时整点 |
| `*/5 * * * *` | 每 5 分钟 |
| `0 9 * * 1-5` | 工作日（周一至周五）每天 9:00 |
| `0 0 1 * *` | 每月 1 号 0:00 |
| `0 0,12 * * *` | 每天 0:00 与 12:00 |

#### 6 字段秒级示例

| 表达式 | 含义 |
|---|---|
| `30 * * * * *` | 每分钟的第 30 秒 |
| `0 */10 * * * *` | 每 10 秒（秒位 0，分位每 10 分钟——实际为每 10 分钟整；秒级高频请用 `*/10 * * * * *`） |
| `*/10 * * * * *` | 每 10 秒 |

### 注意事项

- 表达式非法时，派发会返回 `CronParse` 错误，`TaskBuilder::dispatch()` 将抛出 `InvalidTaskConfigException`。
- 5 字段与 6 字段可混用，但同一表达式不要同时写 5 个和 6 个字段；以空格切分后的字段数决定解析路径。
- 秒级表达式会带来高频触发，请配合 `maxInstances` / `rateLimit` 防止堆积。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 工作日每天 9:00 跑数据汇总
$taskId = TaskBuilder::shell('php /app/bin/report.php daily')
    ->cron('0 9 * * 1-5')
    ->withTimezone('Asia/Shanghai')
    ->withRetry(3, 2)
    ->timeout(300)
    ->dispatch();

// 6 字段秒级：每 30 秒采一次心跳
TaskBuilder::shell('curl -s http://127.0.0.1/healthz > /dev/null')
    ->cron('30 * * * * *')
    ->maxInstances(1)
    ->dispatch();
```

### 生产建议

- 时区务必显式设置（`withTimezone('Asia/Shanghai')`），避免服务器迁移或容器时区漂移导致触发点偏移。
- 长耗时 cron 任务务必设 `timeout` 与 `maxInstances(1)`，防止上一轮未结束、下一轮又派发造成实例堆积。
- 高频秒级任务建议改用 `every` 更直观，且更易做限流。

---

## 2. every(int $secs) — 固定间隔触发

### 签名

```php
public function every(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 间隔秒数（IntervalTrigger） |

### 行为说明

`every` 设置一个固定间隔触发器（IntervalTrigger）。派发后 `next_fire` 被设为 `now + secs`，到点触发后再次累加 `secs`。

- **首次立即触发**：IntervalTrigger 的首次触发在派发后即生效（`next_fire` 初值为 `now + secs`，调度器首个扫描周期到点即派发）。
- 与 `cron` 同时设置时，`cron` 优先，`every` 被忽略并发出告警。

### 注意事项

- `$secs` 过大接近 `u64::MAX` 时，内部用 `saturating_add` 防止溢出回绕导致死循环触发。
- `every` 适合「轮询 / 心跳 / 拉取」类无明确时钟点的周期任务；有明确时钟点（如每天 0 点）请用 `cron`。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 每 60 秒拉取一次邮件
TaskBuilder::shell('php /app/bin/mail-pull.php')
    ->every(60)
    ->maxInstances(1)
    ->timeout(50)
    ->dispatch();

// 配合 jitter 防止多实例同时拉取造成惊群
TaskBuilder::shell('php /app/bin/sync.php')
    ->every(30)
    ->jitter(5)
    ->dispatch();
```

### 生产建议

- 间隔应略大于单次执行耗时，并设 `maxInstances(1)`，否则任务会越积越多。
- 多个同类型轮询任务叠加时，给每个任务加不同的 `jitter`，错峰执行。

---

## 3. runAt(int $ts) — 一次性绝对时间戳触发

### 签名

```php
public function runAt(int $ts): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$ts` | `int` | 是 | Unix 绝对时间戳（秒） |

### 行为说明

`runAt` 设置一个一次性触发器（DateTrigger）。`next_fire` 直接设为 `$ts`，到点触发**单次**后任务转入 `Success` 终态，不再继续调度。

- **优先级最高**：当 `runAt` 与 `cron` / `every` / `countdown` 同时设置时，`runAt` 优先，其余被忽略并发出告警；`jitter` 在 `runAt` 任务上也被忽略。

### 注意事项

- `$ts` 是绝对时间戳，与时区无关；但「你想要的那一刻」需要调用方自行换算成 Unix 时间戳。
- 单次任务触发后即终态，不要指望它再次触发；如需周期执行请用 `cron` / `every`。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 30 分钟后执行一次清理
TaskBuilder::shell('php /app/bin/cleanup-tmp.php')
    ->runAt(time() + 1800)
    ->timeout(120)
    ->dispatch();

// 指定绝对时刻：明天 02:00 (Asia/Shanghai) 执行一次对账
$tz = new DateTimeZone('Asia/Shanghai');
$ts = (new DateTime('tomorrow 02:00', $tz))->getTimestamp();
TaskBuilder::shell('php /app/bin/reconcile.php')
    ->runAt($ts)
    ->withRetry(2, 5)
    ->dispatch();
```

### 生产建议

- 用于「延迟一次性」任务（如订单超时关单、预约提醒），相比自建延迟队列更轻量。
- 若任务必须成功，配合 `acksOnFailure(false)` 让失败无限重试，直到成功或被取消。

---

## 4. countdown(int $secs) — 相对延迟触发

### 签名

```php
public function countdown(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 相对当前时间的延迟秒数 |

### 行为说明

`countdown` 设置一个相对延迟触发器，等价于 `runAt(time() + $secs)`（参考 Celery `apply_async(countdown=N)`）。

- **与 `runAt` 互斥**：同时设置 `runAt` 与 `countdown` 时，`runAt` 优先，`countdown` 被忽略。
- 触发后同样转入 `Success` 终态（一次性）。

### 注意事项

- `countdown` 是 `runAt` 的语法糖，二者不要同时设置；需要绝对时刻用 `runAt`，需要「N 秒后」用 `countdown`。
- 派发时刻到实际执行时刻之间，daemon 必须存活；若 daemon 在到点前重启且任务已 `persist(true)`，重启后会按 `next_fire` 继续调度，不会丢失。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 15 分钟后发一封提醒邮件
TaskBuilder::shell('php /app/bin/mail-remind.php --order=' . $orderId)
    ->countdown(900)
    ->dispatch();

// 同时设 runAt 与 countdown：runAt 生效，countdown 被忽略
TaskBuilder::shell('echo hi')
    ->runAt(time() + 3600)
    ->countdown(60)   // 被忽略，仅 runAt 生效
    ->dispatch();
```

### 生产建议

- 适合「下单后 N 分钟未支付则关单」等业务延迟场景；把业务 ID 通过命令参数或 `withMeta` 透传。
- 延迟任务建议开启 `persist(true)`，防止 daemon 重启丢失。

---

## 5. orCron(array $exprs) — 多 Cron 表达式并集触发

### 签名

```php
public function orCron(array $exprs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$exprs` | `array<string>` | 是 | 附加 cron 表达式列表，替换此前设置过的列表；空数组清空 |

### 行为说明

`orCron` 追加额外的 cron 表达式。任务的触发集合为 `cron`（若设）与 `orCron`（非空项）的**并集**：调度器对每个表达式分别求下一次触发，取其中最近的一次作为 `next_fire`，到点派发后重新求并集。

- 当 `cron` 未设而 `orCron` 非空时，`orCron` 自身即构成触发集合。
- 任一表达式非法都会导致派发失败（`CronParse` 错误）。

### 注意事项

- `orCron` 是「替换」语义：再次调用会用新列表覆盖旧列表，而非追加。
- 求并集时取最小 `next_fire`，因此不同表达式的触发点会被合并到一条调度线上，不会并发派发同一任务。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 工作日早 9 点 + 每月 1 号 0 点：两次都要跑月报预热
TaskBuilder::shell('php /app/bin/report-warmup.php')
    ->cron('0 9 * * 1-5')
    ->orCron(['0 0 1 * *'])
    ->withTimezone('Asia/Shanghai')
    ->dispatch();

// 仅用 orCron（不设 cron）：每天 0 点与 12 点
TaskBuilder::shell('php /app/bin/heartbeat.php')
    ->orCron(['0 0 * * *', '0 12 * * *'])
    ->dispatch();
```

### 生产建议

- 适合「多种周期都要执行同一逻辑」的场景，避免为同一脚本派发多个任务造成配置分散。
- 配合 `coalesce(true)` 可把短时间内的多次匹配合并为一次执行。

---

## 6. skipDates(array $dates) — 跳过指定日期

### 签名

```php
public function skipDates(array $dates): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$dates` | `array<int>` | 是 | Unix 时间戳数组；按其「日历日期」（任务时区下）判定跳过 |

### 行为说明

`skipDates` 设置一个跳过日期列表（典型用于节假日）。调度器在判定是否触发时，会把 `next_fire` 的日历日期（任务时区下）与列表中的日历日期比对，命中则跳过本次触发并继续向后求下一次匹配。

- 跳过的是「整日」，而非精确到时间戳那一秒。
- 通常与 `cron` / `orCron` 组合使用；对 `runAt`/`countdown` 一次性任务意义不大。

### 注意事项

- 列表中的时间戳仅取其日历日期部分，建议统一用各节假日当天 0 点的时间戳以避免歧义。
- 列表较长时无性能问题（按日比对），但请定期清理过期日期。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 2026 年法定节假日（示例，时间戳为当天 0 点 Asia/Shanghai）
$holidays = [
    (new DateTime('2026-01-01', new DateTimeZone('Asia/Shanghai')))->getTimestamp(),
    (new DateTime('2026-02-17', new DateTimeZone('Asia/Shanghai')))->getTimestamp(),
    (new DateTime('2026-05-01', new DateTimeZone('Asia/Shanghai')))->getTimestamp(),
];

// 工作日每天 9 点跑，但跳过节假日
TaskBuilder::shell('php /app/bin/morning-report.php')
    ->cron('0 9 * * 1-5')
    ->skipDates($holidays)
    ->withTimezone('Asia/Shanghai')
    ->dispatch();
```

### 生产建议

- 节假日表建议从外部配置 / 接口加载，避免每年手工改代码。
- 与 `workdaysOnly` 二选一即可：`workdaysOnly` 只过滤周末，`skipDates` 可过滤任意日期，两者可叠加（先工作日，再跳节假日）。

---

## 7. workdaysOnly() — 仅工作日触发

### 签名

```php
public function workdaysOnly(): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| 无 | — | — | 无参方法，调用即启用 |

### 行为说明

`workdaysOnly` 启用「仅工作日」过滤：仅周一至周五触发，周六、周日跳过。命中周末时跳过本次触发并继续向后求下一次匹配。

- 与 `cron` / `orCron` / `every` 均可组合。
- 等价于在 cron 周位写 `1-5`，但 `every` 间隔任务无法用周位表达，此时用 `workdaysOnly` 更方便。

### 注意事项

- 「工作日」此处指周一至周五，**不含**法定节假日调休；如需跳过节假日，请叠加 `skipDates`。
- 调用一次即开启，无关闭参数；如需关闭请重新构建 builder。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 每 10 分钟轮询一次，但仅工作日
TaskBuilder::shell('php /app/bin/poll.php')
    ->every(600)
    ->workdaysOnly()
    ->maxInstances(1)
    ->dispatch();

// every + workdaysOnly + skipDates 三重过滤
TaskBuilder::shell('php /app/bin/daily-check.php')
    ->every(3600)
    ->workdaysOnly()
    ->skipDates($holidays)
    ->dispatch();
```

### 生产建议

- 适合「工作时间内才需要跑」的监控 / 同步任务，可显著降低周末无效负载。
- 跨时区团队请注意：周末判定基于任务时区，非服务器本地时区（若显式设了 `withTimezone`）。

---

## 8. jitter(int $secs) — 随机抖动防惊群

### 签名

```php
public function jitter(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 抖动秒数；`next_fire` 会被加上 `[0, $secs]` 区间的随机偏移 |

### 行为说明

`jitter` 给 `next_fire` 叠加一个 `[0, jitter]` 的随机秒数，使大量同周期任务的触发点被随机打散，避免整点惊群（如 1000 个任务都在 `0 * * * *` 同时触发，瞬间打满下游）。

- 对 `cron` 与 `every` 任务生效。
- **对 `runAt` 一次性任务被忽略**（并发出告警），因为一次性任务的精确时刻不应被随机化。
- 内部用 `saturating_add` 防止「未来时间戳 + 大抖动」溢出回绕。

### 注意事项

- 抖动是「向后」偏移（只会延后，不会提前），因此不会导致任务早于 cron 匹配点触发。
- 抖动过大可能让任务跨过下一个匹配点，请将 `jitter` 控制在相邻两次触发间隔的一定比例内（如 ≤ 间隔的 30%）。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 100 个实例每整点跑备份，用 jitter 错峰 0~300 秒
for ($i = 0; $i < 100; $i++) {
    TaskBuilder::shell("php /app/bin/backup.php --shard={$i}")
        ->cron('0 * * * *')
        ->jitter(300)
        ->maxInstances(1)
        ->dispatch();
}
```

### 生产建议

- 多租户 / 多分片场景下，给每个分片不同的 `jitter`，保护下游数据库 / 第三方 API。
- 抖动应小于 `misfireGraceTime`，否则可能被误判为漏触发。

---

## 9. misfireGraceTime(int $secs) — 误触发宽限

### 签名

```php
public function misfireGraceTime(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 宽限秒数；`0` 表示使用全局默认 60s |

### 行为说明

`misfireGraceTime` 设置「误触发宽限」：当一次触发的实际派发时刻比原计划 `next_fire` 晚了超过此值（如 daemon 重启或调度延迟），则判定为漏触发，记录 `Missed` 事件而非真正执行。

- 传 `0` 时使用全局默认值 `60` 秒。
- 与 `coalesce(true)` 配合可把多次漏触发合并为一次补执行。

### 注意事项

- 宽限过小会导致正常的调度抖动被误判为漏触发；过大则会让迟到很久的触发仍被执行，可能造成数据回放。
- 漏触发产生的 `Missed` 事件可通过 `xhjob_events()` / `pullEvents` 拉取观测。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 每分钟跑，允许迟到最多 10 秒，超过则记 Missed
TaskBuilder::shell('php /app/bin/minute-rollup.php')
    ->cron('* * * * *')
    ->misfireGraceTime(10)
    ->coalesce(true)
    ->dispatch();
```

### 生产建议

- 关键分钟级任务建议把 `misfireGraceTime` 设为略大于单次执行耗时，避免执行慢导致下一轮被误判漏触发。
- 漏触发监控应接入告警：`Missed` 事件频繁出现通常意味着 daemon 负载过高或被频繁重启。

---

## 10. startAt(int $ts) / endAt(int $ts) — 生效区间

### 签名

```php
public function startAt(int $ts): self
public function endAt(int $ts): self
```

### 参数表

| 方法 | 参数 | 类型 | 说明 |
|---|---|---|---|
| `startAt` | `$ts` | `int` | 起始时间戳（`start_date`）；此前的 cron 触发被跳过 |
| `endAt` | `$ts` | `int` | 结束时间戳（`end_date`）；此后的任务转入 `Success` 终态 |

### 行为说明

`startAt` / `endAt` 为任务划定一个生效时间窗口：

- **`startAt`**：在 `start_date` 之前，即使 cron 匹配也会被跳过（不派发、不计 Missed）。
- **`endAt`**：超过 `end_date` 后，任务直接转入 `Success` 终态，停止后续调度。

二者均可单独使用，也可组合成「某时间段内周期执行」。

### 注意事项

- `startAt` / `endAt` 是绝对时间戳，与时区无关；「你想要的那一刻」需调用方自行换算。
- `endAt` 触发的终态是 `Success`（正常结束），而非 `Expired`（后者是 Pending 超时丢弃，见重试超时篇）。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 促销活动期间（2026-11-01 ~ 2026-11-11）每天 0 点跑一次榜单刷新
$tz  = new DateTimeZone('Asia/Shanghai');
$beg = (new DateTime('2026-11-01 00:00', $tz))->getTimestamp();
$end = (new DateTime('2026-11-11 23:59', $tz))->getTimestamp();

TaskBuilder::shell('php /app/bin/refresh-rank.php')
    ->cron('0 0 * * *')
    ->startAt($beg)
    ->endAt($end)
    ->withTimezone('Asia/Shanghai')
    ->dispatch();
```

### 生产建议

- 用于有明确起止时间的活动 / 灰度任务，避免到点后人工取消。
- `endAt` 设得略晚于实际结束时刻，给最后一轮执行留出余量。

---

## 11. withTimezone(string $tz) — 触发时区

### 签名

```php
public function withTimezone(string $tz): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$tz` | `string` | 是 | IANA 时区标识符，如 `Asia/Shanghai`、`America/New_York` |

### 行为说明

`withTimezone` 设置 cron 求值所用的时区。调度器在计算「下一次匹配」时，把当前 Unix 时间戳按此时区解释为本地时间，再与 cron 字段匹配。

- 未设置时使用系统本地时区（`chrono::Local`）。
- 非法时区字符串会导致派发失败（`Config("invalid timezone: ...")`）。

### 注意事项

- `every` / `runAt` / `countdown` 基于绝对秒数，与时区无关；`withTimezone` 主要影响 `cron` / `orCron` 的匹配。
- 容器中系统时区常为 UTC，**强烈建议**显式设置 `withTimezone`，避免「在本地能跑、上容器就偏 8 小时」。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 美东时间每天 9:00 跑
TaskBuilder::shell('php /app/bin/us-report.php')
    ->cron('0 9 * * *')
    ->withTimezone('America/New_York')
    ->dispatch();
```

### 生产建议

- 跨时区业务为每个任务显式指定时区，不要依赖服务器本地时区。
- 夏令时切换日 cron 触发点会偏移，需提前评估对业务的影响。

---

## 触发器互斥规则表

Xhjob 在派发时按 **`runAt` > `cron`（含 `orCron`）> `every`** 的优先级计算初始 `next_fire`，同时设置多个会发出告警并按优先级取一个：

| 同时设置的方法 | 生效项 | 被忽略项 | 行为 |
|---|---|---|---|
| `runAt` + `cron` | `runAt` | `cron` | 告警；按一次性时间戳触发，触发后转 `Success` |
| `runAt` + `every` | `runAt` | `every` | 告警；同上 |
| `runAt` + `countdown` | `runAt` | `countdown` | `runAt` 优先 |
| `runAt` + `jitter` | `runAt` | `jitter` | 告警；一次性任务不抖动 |
| `cron` + `every` | `cron` | `every` | 告警；按 cron 周期触发 |
| `cron` + `orCron` | 并集 | 无 | 取两者最近一次匹配 |
| `countdown` + `cron` | `cron` | `countdown` | `countdown` 仅在无 `runAt`/`cron` 时作为一次性触发 |

> 一次性触发器（`runAt` / `countdown`）触发后转 `Success` 终态；周期触发器（`cron` / `every`）触发后继续计算下一次，直至 `endAt` 到达或 `maxExecutions` 达限。

---

## 全局注意事项

1. **时区默认值**：未调用 `withTimezone` 时，cron 求值使用系统本地时区。容器环境系统时区常为 UTC，**生产环境务必显式设置 `withTimezone`**。
2. **6 字段秒级支持**：Xhjob 的 cron 解析支持 5 字段（分时日月周）与 6 字段（秒分时日月周）；不足 6 字段时自动补 `0` 秒字段。秒级高频任务请配合 `maxInstances` / `rateLimit` 防堆积。
3. **触发器与执行隔离**：触发器只决定「何时派发」，不决定「是否并发执行」；并发控制由 `maxInstances` / `allowOverlap` / `coalesce` / `rateLimit` 负责，详见 [并发控制](concurrency.md)。
4. **持久化与触发器**：周期 / 一次性任务建议开启 `persist(true)`，daemon 重启后会按 `next_fire` 继续调度，不会丢失；崩溃恢复细节详见 [持久化与崩溃恢复](persistence-recovery.md)。
5. **错误传播**：非法 cron 表达式 / 非法时区会让 `TaskBuilder::dispatch()` 抛 `InvalidTaskConfigException`，应在业务层捕获并记录。
