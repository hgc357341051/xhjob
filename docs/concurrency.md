---
title: 并发控制
parent: 核心能力
nav_order: 33
---

# 并发控制

Xhjob 提供一组并发控制选项，用于约束同一任务的并发执行数、合并错过的触发、限流以及调整调度优先级。相关实现位于 `src/scheduler/overlap.rs`、`src/scheduler/rate_limit.rs` 与 `src/scheduler/queue.rs`。

---

## maxInstances（最大并发实例数）

`maxInstances(int $n)` 限制同一任务 **同时运行** 的实例数。当已有 `n` 个实例在 Running 时，新的触发会被跳过，并记录一条 `max_instances_reached` 事件。

- 默认值为 **1**（同一任务不并发）。
- `n` 会被归一化为 `max(n, 1)`，即最小为 1。
- 超出限制的触发 **不会排队等待**，而是直接丢弃该次触发（参考 APScheduler `max_instances`）。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 同一任务最多 3 个实例并发运行
TaskBuilder::shell('worker.sh')
    ->cron('*/1 * * * *')
    ->maxInstances(3)
    ->dispatch();
```

### 注意事项

- `maxInstances` 与 `allowOverlap` 互为反义：默认 `maxInstances=1` + `allowOverlap=false` 即"串行不重叠"。
- 若你的任务执行耗时可能超过触发周期，请根据需要调大 `maxInstances` 或显式 `allowOverlap(true)`，否则会频繁触发 `max_instances_reached`。

---

## allowOverlap（允许重叠执行）

`allowOverlap(bool $allow)` 控制同一任务的新触发是否允许在上一实例尚未完成时启动。

- `false`（默认）：不允许重叠，等价于"上一个跑完才能跑下一个"。
- `true`：允许重叠，新触发立即派发（受 `maxInstances` 上限制约）。

### 代码演示

```php
// 允许重叠执行：即使上一次还没跑完，新周期到点就派发
TaskBuilder::shell('stream-process.sh')
    ->cron('*/1 * * * *')
    ->allowOverlap(true)
    ->maxInstances(5)
    ->dispatch();
```

### 注意事项

- 开启 `allowOverlap` 时通常需要配合 `maxInstances` 设定上限，否则周期任务会无限堆积实例。
- 重叠执行要求任务本身是 **幂等或可重入** 的，否则可能产生数据竞争。

---

## coalesce（合并错过的触发）

`coalesce(bool $c)` 控制当任务错过多个触发点时（如 daemon 宕机恢复后发现错过了 N 个周期），是否将它们 **合并为一次** 执行。

- `true`（默认）：合并为一次，只执行一次。
- `false`：错过的触发直接 **跳过**（不执行），`next_fire` 滚动到下一个周期。

与 `misfireGraceTime` 配合：只有 `now - next_fire > grace_time` 的触发才算 misfire，才会走 coalesce 逻辑。

### 代码演示

```php
// 关闭合并：宕机恢复后错过的周期一律跳过，只跑下一个周期
TaskBuilder::shell('hourly-report.sh')
    ->cron('0 * * * *')
    ->coalesce(false)
    ->misfireGraceTime(300)
    ->dispatch();
```

### 注意事项

- 对于"补跑"敏感的任务（如账单生成）建议保持 `coalesce=true`，避免漏跑；对于"最新即可"的任务（如缓存刷新）可设为 `false`。

---

## rateLimit（滑动窗口限流）

`rateLimit(int $count, int $window)` 设定滑动窗口限流：在 `window` 秒的滑动窗口内，最多派发 `count` 次。超出时任务被暂时阻塞，记录 `rate_limited` 事件，并将 `next_fire` 推进到窗口结束后再尝试。

- 滑动窗口计数器在 cron/interval 触发路径与 dispatch 派发路径之间共享。
- 周期任务（cron/interval）被限流时，`next_fire` 会被推进 `window` 秒，避免 `scan_once` 重复 enqueue。
- 限流桶在任务进入终态时清理；周期任务回到 Pending 重新触发时桶被保留，使滑动窗口跨触发周期生效。参考 Celery `rate_limit`。

### 代码演示

```php
// 每 60 秒窗口内最多派发 10 次（约每 6 秒一次）
TaskBuilder::shell('call-api.sh')
    ->every(1)
    ->rateLimit(10, 60)
    ->dispatch();
```

### 注意事项

- `rateLimit` 面向 **单任务** 的频率控制；若需全局并发上限，请在 daemon 启动参数或 worker 配置层调整。
- 滑动窗口是近似实现，极端高并发下可能有微小偏差。

---

## priority（调度优先级）

`priority(int $p)` 设定任务调度优先级。**数值越大越优先** 被派发。

当队列中同时有多个 Pending 任务等待 worker 时，调度器按 `priority` 降序选取先派发；优先级相同时按 `created_at` 升序（先到先服务）。

### 代码演示

```php
// 高优先级订单任务优先于普通任务派发
TaskBuilder::shell('process-vip-order.sh')
    ->priority(10)
    ->dispatch();

// 普通任务
TaskBuilder::shell('process-normal-order.sh')
    ->priority(1)
    ->dispatch();
```

### 注意事项

- 优先级仅在 **任务排队等待 worker** 时生效；一旦任务进入 Running，优先级不再影响其执行。
- 默认优先级为 `0`。

---

## maxExecutions（最大执行次数）

`maxExecutions(int $n)` 限制周期任务（cron/interval）的 **累计执行次数**。达到上限后任务进入 `Success` 终态，不再被触发。

- `0` = **无限**（默认，永不停止）。
- 每次成功执行后 `execution_count` 递增；当 `execution_count >= max_executions` 时转入终态。
- 仅对周期任务（cron/interval）有意义；一次性任务（runAt/countdown）本就只执行一次。

### 代码演示

```php
// 试用任务：总共只跑 100 次后自动停止
TaskBuilder::shell('trial-job.sh')
    ->cron('*/5 * * * *')
    ->maxExecutions(100)
    ->dispatch();
```

### 注意事项

- `maxExecutions` 计数的是 **成功执行** 次数；失败重试不会重复累加该计数。
- 计数持久化在 store 中，daemon 重启后不丢失（需开启 `persist`）。
