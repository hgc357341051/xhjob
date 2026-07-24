---
title: 重试与超时
parent: 核心能力
nav_order: 32
---

# 重试与超时

Xhjob 提供完善的失败恢复与执行时长控制能力，覆盖三种失败场景：**任务逻辑失败重试**（`withRetry` / `retryBackoff` / `acksOnFailure`）、**硬超时强杀**（`timeout`）、**软超时优雅退出**（`softTimeout`），以及 **任务过期自动丢弃**（`expires`）。相关实现位于 `src/retry/mod.rs` 与 `src/executor/shell.rs`。

---

## withRetry（基础重试）

`withRetry(int $max, int $delay = 1)` 设定任务的重试策略：

- `max`：最大重试次数。达到上限后任务进入 `Failed` 终态。
- `delay`：每次重试前的固定等待秒数（默认 1 秒）。

只有特定错误类型才会触发重试：HTTP 任务的 5xx 响应、Shell 任务的非零退出码。参考 Celery retry。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 最多重试 3 次，每次间隔 2 秒
TaskBuilder::shell('fetch-data.sh')
    ->withRetry(3, 2)
    ->dispatch();
```

---

## retryBackoff（指数退避）

`retryBackoff(bool $on)` 开启指数退避，重试间隔随失败次数指数增长，避免在下游故障时雪崩式重试。参考 Celery `retry_backoff`。

### 计算公式

当 `retry_backoff=true` 时，第 N 次重试前的等待秒数为：

```
delay = min(retry_delay * 2^attempts, retry_delay * 60)
```

- `attempts` 是 **本次重试前** 已失败的次数（0-indexed）。
- `attempts=0` → `delay = retry_delay`（即 `delay` 本身）。
- `attempts=1` → `delay = 2 * retry_delay`。
- `attempts=2` → `delay = 4 * retry_delay`。
- 以此类推，**上限封顶为 `retry_delay * 60`**（避免无限增长）。

当 `retry_backoff=false`（默认）时，重试间隔固定为 `retry_delay`。

> 公式实现见 `src/retry/mod.rs` 的 `backoff_delay()` 函数。注意 `task.attempts` 在每次重试时通过 `saturating_add(1)` 递增，防止 `acks_on_failure=false` 累计到 `u32::MAX` 时回绕导致重试风暴。

### 代码演示

```php
// 指数退避：base delay=3s，封顶 3*60=180s
TaskBuilder::shell('call-third-api.sh')
    ->withRetry(5, 3)
    ->retryBackoff(true)
    ->dispatch();
```

---

## acksOnFailure（失败不 ack 无限重试）

`acksOnFailure(bool $on)` 控制任务失败后是否向队列 ack：

- `true`（默认）：失败即 ack，受 `retry_max` 限制，达到上限进入 `Failed` 终态。
- `false`：失败 **不 ack**，任务会被无限重试，**不受 `retry_max` 限制**（`effective_max_attempts` 被置为 `u32::MAX`）。

适用于"无论如何都必须成功"的关键任务，例如订单状态机推进。参考 Celery `acks_on_failure`。

### 代码演示

```php
// 关键任务：失败不 ack，无限重试直到成功
TaskBuilder::shell('process-order.sh')
    ->withRetry(3, 5)          // retry_max 在此被忽略
    ->acksOnFailure(false)
    ->dispatch();
```

---

## timeout（硬超时）

`timeout(int $secs)` 设定任务的 **硬超时**（秒）。超时后：

1. daemon 通过 `tokio::time::timeout` 包装任务 Future；
2. 超时触发后向子进程发送 **SIGKILL**；
3. reap 子进程，回收资源，任务标记为失败。

`timeout(0)` = 不设硬超时（仅依赖 watchdog 假死检测）。

### 代码演示

```php
// 硬超时 30 秒，超时直接 SIGKILL
TaskBuilder::shell('long-running.sh')
    ->timeout(30)
    ->dispatch();
```

---

## softTimeout（软超时，SIGTERM→SIGKILL 升级链）

`softTimeout(int $secs)` 设定 **软超时**（秒），参考 Celery `soft_time_limit`。它提供比硬超时更优雅的退出路径：

### 升级链

1. 子进程运行满 `soft_timeout` 秒 → 发送 **SIGTERM**（业务可捕获并做清理、落盘、flush）。
2. 自 SIGTERM 起再等待 `timeout - soft_timeout` 秒（宽限期）。
3. 若宽限期内子进程仍未退出 → 发送 **SIGKILL** 强杀。

### 约束

- **必须 `soft_timeout < timeout`**，否则视为配置错误。
- `softTimeout(0)` 等价于未设置（`None`），跳过 SIGTERM 路径。
- **HTTP 任务不支持软超时**（HTTP 客户端无法被优雅中断）：构建时会记录 warning 并将 `soft_timeout` 重置为 `None`。

### 代码演示

```php
// 软超时 25s（先 SIGTERM 优雅退出），硬超时 30s（5s 宽限后 SIGKILL）
TaskBuilder::shell('graceful-job.sh')
    ->softTimeout(25)
    ->timeout(30)
    ->dispatch();
```

---

## expires（任务过期自动丢弃）

`expires(int $secs)` 设定任务级别的过期时间（秒），参考 APScheduler `expires`：

- 若一个任务停留在 `Pending` 状态的时间 **超过 `expires` 秒**（自 `created_at` 起算），它会被自动转为 **`Expired` 终态** 并丢弃。
- 默认 `0` = 不过期。
- **只影响 Pending 任务**；已进入 `Running` 的任务不会被 expires 中断（Running 任务由 `timeout` / `softTimeout` / watchdog 管理）。

适用于"过期即无意义"的场景，例如限时秒杀的库存预热任务——如果迟迟没轮到执行，直接丢弃比执行一个过时的预热更有意义。

### 代码演示

```php
// 秒杀预热任务：若 120 秒内仍未执行则自动过期丢弃
TaskBuilder::shell('warmup-seckill.sh')
    ->expires(120)
    ->dispatch();
```
