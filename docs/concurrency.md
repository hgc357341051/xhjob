# 并发控制（Concurrency Control）

> 核心能力篇 · maxInstances / allowOverlap / coalesce / rateLimit / priority / maxExecutions

触发器只决定「何时派发」，并发控制决定「能否同时跑、跑多少、谁先跑、何时停」。Xhjob 的 `TaskBuilder` 提供一组链式方法，覆盖最大实例数、重叠允许、漏触发合并、滑动窗口限流、优先级调度、最大执行次数等场景。所有方法均返回 `self`，可链式叠加。

本文档按 **签名 → 参数表 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构逐项展开，并在末尾给出常见组合表与全局注意事项。

> 示例基于 `Xhjob\TaskBuilder`（ThinkPHP 扩展包）/ Rust `Xhjob` 链式 Builder。并发判定逻辑位于 `src/scheduler/overlap.rs` 与 `src/scheduler/rate_limit.rs`。事件类型（`EventType`）取自 `src/store/mod.rs`，多词值用下划线（如 `max_instances_reached`、`rate_limited`）。

---

## 1. maxInstances(int $n) — 最大并发实例数

### 签名

```php
public function maxInstances(int $n): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$n` | `int` | 是 | `1` | 最大并发实例数；达到上限时本次触发被丢弃 |

### 行为说明

`maxInstances` 设置同一任务允许的最大并发执行实例数。当一次触发到达时，若该任务当前正在运行的实例数已达 `$n`，则**本次触发被丢弃**（SKIP_OVERLAP），并记录 `max_instances_reached` 事件，不会排队等待。

- 运行实例数由内存计数器（`on_start` / `on_finish` 维护）跟踪，能正确表达 N>1 的并发；daemon 重启时计数器重置为 0。
- 当 `maxInstances` 显式设为 N>1 时，它**优先于** `allowOverlap`，作为硬并发上限；若同时设了 `allowOverlap(true)`，会发出告警并以 `maxInstances` 为准。

### 注意事项

- 达到上限是「丢弃本次触发」而非「延后本次触发」——下一轮 cron 匹配会重新尝试派发，被丢弃的这次不会补执行。
- `maxInstances(1)` 是默认值，配合默认 `allowOverlap(false)` 即「严格串行」。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 数据库迁移任务：严格串行，绝不允许并发
TaskBuilder::shell('php /app/bin/migrate.php')
    ->cron('0 2 * * *')
    ->maxInstances(1)
    ->timeout(1800)
    ->dispatch();

// 允许最多 3 个并发实例（如多分片同时处理）
TaskBuilder::shell('php /app/bin/process-shard.php')
    ->every(60)
    ->maxInstances(3)
    ->timeout(300)
    ->dispatch();
```

### 生产建议

- 写入同一资源（DB 表、文件）的任务务必 `maxInstances(1)`，防止并发写冲突。
- 监听 `max_instances_reached` 事件：频繁触发意味着任务执行耗时超过了触发间隔，应考虑加并发或拉长间隔。

---

## 2. allowOverlap(bool $on = true) — 允许重叠执行

### 签名

```php
public function allowOverlap(bool $on = true): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$on` | `bool` | 否 | `true` | 是否允许同一任务并发执行 |

### 行为说明

`allowOverlap(true)` 允许同一任务的多次触发并发执行，覆盖 `maxInstances` 的默认阻止行为（即默认 `maxInstances(1) + allowOverlap(false)` 的「至多 1 个实例」语义）。

与 `maxInstances` 的关系（参考 APScheduler 语义）：

| 配置 | 行为 |
|---|---|
| `maxInstances(1)`（默认）+ `allowOverlap(false)`（默认） | 至多 1 个并发实例（严格不重叠） |
| `maxInstances(1)`（默认）+ `allowOverlap(true)` | 无限并发（向后兼容的「允许重叠」语义） |
| `maxInstances(N>1)` + `allowOverlap(任意)` | 硬上限 N（`maxInstances` 优先，同时设 `allowOverlap(true)` 会告警） |

### 注意事项

- `allowOverlap(true)` 单独使用（不设 `maxInstances`）是「无限并发」，仅适合极轻量、无资源竞争的任务；重任务请用 `maxInstances(N)` 设明确上限。
- 允许重叠要求业务脚本本身并发安全（无共享可变状态、或加了锁）。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 无状态心跳任务：允许重叠（上一轮没跑完也不影响下一轮）
TaskBuilder::shell('curl -s http://127.0.0.1/healthz')
    ->every(10)
    ->allowOverlap(true)
    ->timeout(8)
    ->dispatch();

// 明确上限比无限更安全：用 maxInstances 替代裸 allowOverlap
TaskBuilder::shell('php /app/bin/poll.php')
    ->every(10)
    ->maxInstances(5)        // 明确上限 5
    ->allowOverlap(true)     // 触发告警但以 maxInstances 为准
    ->timeout(8)
    ->dispatch();
```

### 生产建议

- 优先用 `maxInstances(N)` 表达明确并发上限，而非裸 `allowOverlap(true)` 的无限并发。
- 允许重叠时务必配合 `rateLimit`，防止短时间触发风暴打满下游。

---

## 3. coalesce(bool $on = true) — 合并漏触发

### 签名

```php
public function coalesce(bool $on = true): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$on` | `bool` | 否 | `true` | 是否合并多次漏触发病为一次执行 |

### 行为说明

`coalesce(true)`（默认）启用漏触发合并：当任务因故错过了多次触发（如 daemon 重启、被 `maxInstances` 丢弃），恢复调度时只补执行**一次**，而非把错过的每一次都补一遍。

- `coalesce(true)`：合并 N 次漏触发为 1 次执行（默认）。
- `coalesce(false)`：改用 `misfireGraceTime`（默认 60s）逐次判定，仅补执行「迟到未超过宽限」的漏触发，超限的记 `missed` 事件。

### 注意事项

- `coalesce` 默认就是 `true`，多数场景无需改动；只有需要「逐次补执行 + 宽限判定」时才设 `false`。
- 合并语义对「状态同步类」任务友好（多次错过同步一次即可），对「逐次计费 / 逐次通知」类任务不适合（会漏处理）。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 每 5 分钟同步一次状态：错过多次只补一次（默认 coalesce=true）
TaskBuilder::shell('php /app/bin/sync-state.php')
    ->cron('*/5 * * * *')
    ->coalesce(true)
    ->maxInstances(1)
    ->dispatch();

// 严格逐次：关掉合并，靠 misfireGraceTime 判定每次
TaskBuilder::shell('php /app/bin/per-minute-billing.php')
    ->cron('* * * * *')
    ->coalesce(false)
    ->misfireGraceTime(30)
    ->dispatch();
```

### 生产建议

- 状态同步 / 缓存刷新类任务保持默认 `coalesce(true)`，避免恢复时雪崩式补执行。
- 计费 / 通知类任务按需设 `coalesce(false)` + 合理的 `misfireGraceTime`，并接入 `missed` 事件告警。

---

## 4. rateLimit(int $count, int $window) — 滑动窗口限流

### 签名

```php
public function rateLimit(int $count, int $window): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$count` | `int` | 是 | 窗口内允许的最多触发次数 |
| `$window` | `int` | 是 | 窗口大小（秒） |

### 行为说明

`rateLimit` 设置按任务维度的滑动窗口限流（参考 Celery `rate_limit(count, window)`）。算法维护该任务最近的触发时间戳列表，新触发到达时，若过去 `$window` 秒内的触发数已达 `$count`，则**拒绝本次触发**，记录 `rate_limited` 事件，并把 `next_fire` 推后 `$window` 秒后重新评估。

- `rate_limit_count == 0` 或 `rate_limit_window == 0`（默认）表示不限流，直接放行。
- 限流状态在内存中维护，**daemon 重启后窗口清空**（与 Celery 语义一致，限流是 worker 本地概念，非持久化状态）。

### 注意事项

- 限流是「丢弃并推后」，被限流的触发不会排队，而是延后到下一窗口重新评估。
- 限流与 `maxInstances` 互补：`maxInstances` 限制「同时跑几个」，`rateLimit` 限制「一段时间内跑几次」。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 调用第三方 API：60 秒内最多 10 次
TaskBuilder::http('GET', 'https://api.example.com/data')
    ->cron('*/5 * * * *')     // 每 5 分钟一次（理论 12 次/小时）
    ->rateLimit(10, 60)       // 但限流 60s 内最多 10 次
    ->timeout(15)
    ->dispatch();

// 高频任务限流：1 秒内最多 5 次
TaskBuilder::shell('php /app/bin/fast-poll.php')
    ->every(1)
    ->rateLimit(5, 1)
    ->maxInstances(3)
    ->dispatch();
```

### 生产建议

- 对接有配额的第三方 API 务必设 `rateLimit`，避免触发限流被封禁。
- 监听 `rate_limited` 事件：频繁触发说明上游配额不足或触发频率过高，需调整 `count`/`window` 或降低触发频率。

---

## 5. priority(int $p) — 优先级调度

### 签名

```php
public function priority(int $p): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$p` | `int` | 是 | `0` | 优先级；**数值越大越优先调度** |

### 行为说明

`priority` 设置任务调度优先级。当多个 `Pending` 任务同时等待派发时，调度器按优先级从高到低选取——**数值越大越优先**派发。同优先级的任务按 FIFO 排序。

- 默认优先级为 `0`；可设负数（如 `-10`）表示低优先级，正数（如 `10`）表示高优先级。
- 优先级仅在「资源竞争」时生效：若池子空闲，所有任务都会立即派发，优先级无差别。

### 注意事项

- 优先级是「调度顺序」的提示，不是「抢占」——已在执行的任务不会被更高优先级任务打断。
- 大量高优先级任务可能让低优先级任务长期饥饿，应配合 `maxInstances` / 资源规划避免饿死。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 紧急对账：高优先级
TaskBuilder::shell('php /app/bin/reconcile-urgent.php')
    ->countdown(60)
    ->priority(10)
    ->timeout(300)
    ->dispatch();

// 离线报表：低优先级，让位于业务任务
TaskBuilder::shell('php /app/bin/report-offline.php')
    ->cron('0 1 * * *')
    ->priority(-5)
    ->timeout(3600)
    ->dispatch();
```

### 生产建议

- 把「用户感知、实时性要求高」的任务设高优先级，把「离线、批处理」任务设低优先级。
- 优先级档位不宜过多（通常 -10 ~ 10），避免调度器比较开销与配置混乱。

---

## 6. maxExecutions(int $n) — 最大执行次数

### 签名

```php
public function maxExecutions(int $n): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$n` | `int` | 是 | `0` | 最大执行次数；`0`=无限 |

### 行为说明

`maxExecutions` 设置 cron / every 周期任务的累计最大执行次数。任务每成功执行一次计数加一，**达到上限后转入 `Success` 终态**，停止后续调度。

- `maxExecutions(0)`（默认）表示无限执行，符合常规周期任务语义。
- 主要用于「跑 N 次就停」的场景（如灰度逐步放量、有限轮次的迁移任务）。

### 注意事项

- 计的是「执行次数」而非「触发次数」：被 `maxInstances` / `rateLimit` 丢弃的触发不计入。
- 终态为 `Success`（正常完成），即使中间某次执行失败过——只要总成功次数达限即终止。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 灰度迁移：每 10 分钟跑一次，总共跑 6 次后自动停止
TaskBuilder::shell('php /app/bin/migrate-batch.php')
    ->every(600)
    ->maxExecutions(6)
    ->maxInstances(1)
    ->timeout(300)
    ->dispatch();

// 常规周期任务：无限执行（默认）
TaskBuilder::shell('php /app/bin/daily-cleanup.php')
    ->cron('0 3 * * *')
    ->maxExecutions(0)        // 显式声明无限（即默认）
    ->dispatch();
```

### 生产建议

- 用于有明确总量上限的迁移 / 灰度任务，到限自动停止，避免人工取消。
- 关键周期任务保持 `maxExecutions(0)`，切勿误设为小值导致任务意外停止。

---

## 常见组合表

不同业务场景下的推荐组合：

| 场景 | 推荐配置 | 说明 |
|---|---|---|
| **严格串行**（DB 迁移、写同一文件） | `maxInstances(1)` + `allowOverlap(false)` | 至多 1 个实例，绝不并发；默认即此行为 |
| **允许并发但限流**（调第三方 API） | `maxInstances(5)` + `rateLimit(10, 60)` | 最多 5 个并发，且 60s 内最多触发 10 次 |
| **无限并发**（无状态心跳） | `allowOverlap(true)`（不设 maxInstances） | 上一轮没跑完也不阻塞下一轮；仅适合轻量任务 |
| **多分片并行**（分片处理） | `maxInstances(N)` + `allowOverlap(true)` | 显式 N 个并发（`maxInstances` 优先，告警可忽略） |
| **高优先级实时任务** | `priority(10)` + `maxInstances(1)` + `timeout(短)` | 优先调度、串行、快速失败 |
| **低优先级离线任务** | `priority(-5)` + `maxInstances(1)` + `timeout(长)` | 让位于业务，长耗时但不抢占 |
| **漏触发合并**（状态同步） | `coalesce(true)` + `maxInstances(1)` | 错过多次只补一次，避免恢复雪崩 |
| **逐次补执行**（计费/通知） | `coalesce(false)` + `misfireGraceTime(30)` | 逐次判定，超宽限记 `missed` |
| **跑 N 次即停**（灰度迁移） | `maxExecutions(N)` + `maxInstances(1)` | 累计 N 次后转 `Success` 终态 |
| **高频限流**（轮询保护） | `every(1)` + `rateLimit(5, 1)` + `maxInstances(3)` | 每秒最多 5 次、最多 3 个并发 |

---

## 全局注意事项

1. **并发判定在 daemon 内存中**：`maxInstances` 的运行计数与 `rateLimit` 的窗口状态都在 daemon 进程内存维护，daemon 重启会重置（计数归零、窗口清空）。这与持久化任务定义不冲突——任务定义落盘，但并发瞬时状态不落盘。
2. **maxInstances 优先于 allowOverlap**：`maxInstances(N>1)` 显式设置时作为硬上限，优先于 `allowOverlap`；同时设 `allowOverlap(true)` 会告警但以 `maxInstances` 为准。
3. **丢弃 vs 排队**：`maxInstances` 达上限与 `rateLimit` 超限都是「丢弃本次触发」（前者不补，后者推后到下一窗口重新评估），均不会无限堆积待执行任务。
4. **事件可观测**：并发控制产生的关键事件为 `max_instances_reached` 与 `rate_limited`（多词值用下划线，与 `src/store/mod.rs` 严格对齐），可通过 `xhjob_events()` / `pullEvents` 拉取监控。
5. **与触发器协作**：并发控制方法只决定「能否 / 何时执行」，触发时刻由 [触发器](triggers.md) 决定，执行弹性由 [重试与超时](retry-timeout.md) 决定，三者正交组合。

---

## 相关事件类型（EventType）

并发控制相关的事件类型（多词值用下划线，与 `src/store/mod.rs` 严格对齐）：

| EventType | 触发场景 |
|---|---|
| `started` | 任务实例开始执行（`on_start` 计数 +1） |
| `succeeded` | 任务实例执行成功（`on_finish` 计数 -1） |
| `max_instances_reached` | 触发到达但运行实例数已达 `maxInstances` 上限，本次触发被丢弃 |
| `rate_limited` | 触发到达但滑动窗口内触发数已达 `rateLimit` 上限，本次触发被拒绝并推后 |
| `missed` | `coalesce(false)` 模式下漏触发超过 `misfireGraceTime`，记漏触发而非执行 |

> 注意拼写：`max_instances_reached`（非 `maxinstancesreached`）、`rate_limited`（非 `ratelimited`）。完整 14 种 EventType 见 [进度上报与事件](progress-events.md)。
