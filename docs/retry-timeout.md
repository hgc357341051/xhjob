# 重试与超时（Retry & Timeout）

> 核心能力篇 · withRetry / retryBackoff / acksOnFailure / acksLate / timeout / softTimeout / expires / misfireGraceTime

任务执行难免失败或卡住。Xhjob 的 `TaskBuilder` 提供一组链式方法，覆盖「失败重试 → 退避策略 → 确认语义 → 硬/软超时 → 过期丢弃」的完整弹性链路。所有方法均返回 `self`，可链式叠加。

本文档按 **签名 → 参数表 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构逐项展开，并在末尾给出全局注意事项。

> 示例基于 `Xhjob\TaskBuilder`（ThinkPHP 扩展包）/ Rust `Xhjob` 链式 Builder。事件类型（`EventType`）取自 `src/store/mod.rs`，多词值用下划线（如 `lease_held`、`hung_detected`）。

---

## 1. withRetry(int $max, int $delay = 1) — 基础重试

### 签名

```php
public function withRetry(int $max, int $delay = 1): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$max` | `int` | 是 | — | 最大重试次数（不含首次执行） |
| `$delay` | `int` | 否 | `1` | 每次重试前的固定间隔（秒） |

### 行为说明

`withRetry` 设置任务失败后的重试策略。任务执行失败后，若重试次数未达上限，则等待 `$delay` 秒后重新派发，直至成功或重试次数耗尽。

- **可重试的失败**：HTTP 任务仅 5xx 响应重试；shell 任务非零退出码重试；网络错误（无状态码）总是重试。
- **HTTP 方法安全**：`GET` / `HEAD` / `OPTIONS`（安全方法）5xx 总是重试；`POST` / `PUT` / `DELETE` / `PATCH`（非幂等）仅当显式 `idempotent(true)` 时才重试，防止重复副作用。
- 重试次数耗尽后任务转 `Failed` 终态，并记录 `failed` 事件。

### 注意事项

- `$max` 是「重试次数」而非「总执行次数」；`withRetry(3)` 最多执行 4 次（1 次首执行 + 3 次重试）。
- 非 idempotent 的 POST 类任务默认不重试，需要重试请显式 `idempotent(true)`。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 最多重试 3 次，每次间隔 2 秒
TaskBuilder::shell('php /app/bin/import.php')
    ->withRetry(3, 2)
    ->timeout(120)
    ->dispatch();

// HTTP POST 任务显式声明幂等后才重试
TaskBuilder::http('POST', 'https://api.example.com/sync')
    ->withRetry(2, 5)
    ->idempotent(true)
    ->timeout(30)
    ->dispatch();
```

### 生产建议

- 重试次数不宜过多（通常 2~5 次），避免雪崩；下游不可用时应快速失败并告警。
- 重试间隔应大于下游平均恢复时间；外部 API 建议用 `retryBackoff(true)` 指数退避。

---

## 2. retryBackoff(bool $on = true) — 指数退避

### 签名

```php
public function retryBackoff(bool $on = true): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$on` | `bool` | 否 | `true` | 是否启用指数退避 |

### 行为说明

`retryBackoff(true)` 启用指数退避：每次重试的间隔随尝试次数指数增长，避免在下游故障期高频重试加剧压力。退避公式为：

```
next_delay = min(delay * 2^attempts, delay * 60)
```

其中 `delay` 来自 `withRetry($max, $delay)`，`attempts` 为已失败次数（从 0 起算），并以上限 `delay * 60` 封顶。

#### 退避公式表（`delay = 1` 为例）

| attempts（已失败次数） | delay × 2^attempts | 封顶 delay × 60 | 实际间隔 |
|---|---|---|---|
| 0 | 1 × 2^0 = 1 | 60 | 1s |
| 1 | 1 × 2^1 = 2 | 60 | 2s |
| 2 | 1 × 2^2 = 4 | 60 | 4s |
| 3 | 1 × 2^3 = 8 | 60 | 8s |
| 4 | 1 × 2^4 = 16 | 60 | 16s |
| 5 | 1 × 2^5 = 32 | 60 | 32s |
| 6 | 1 × 2^6 = 64 | 60 | 60s（封顶） |
| 7+ | ≥ 128 | 60 | 60s（封顶） |

### 注意事项

- 退避公式以 `withRetry` 的 `$delay` 为底数；未调用 `withRetry` 时 `delay` 默认为 `1` 秒，退避增长缓慢，建议显式设置 `withRetry($max, $delay)`。
- 封顶值为 `delay * 60`，因此调大 `delay` 会同时抬高退避上限。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 调用第三方 API，指数退避（底数 5s，封顶 300s）
TaskBuilder::http('GET', 'https://api.example.com/orders')
    ->withRetry(6, 5)
    ->retryBackoff(true)
    ->timeout(15)
    ->dispatch();

// shell 任务同样适用
TaskBuilder::shell('php /app/bin/fetch-feed.php')
    ->withRetry(5, 2)
    ->retryBackoff(true)
    ->timeout(60)
    ->dispatch();
```

### 生产建议

- 对外部依赖（第三方 API、易抖动服务）强烈建议开启指数退避。
- 若下游故障恢复较慢，可适当增大 `$delay` 提高退避基线；但需配合 `expires` 避免无限重试占用资源。

---

## 3. acksOnFailure(bool $on = true) — 失败不 ack 无限重试

### 签名

```php
public function acksOnFailure(bool $on = true): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$on` | `bool` | 否 | `true` | `true`=失败即 ack（按 retry_max 终止）；`false`=失败不 ack，无限重试 |

### 行为说明

`acksOnFailure(false)` 改变失败确认语义（参考 Celery `acks_on_failure`）：任务失败后**不确认**，因此不受 `withRetry` 的 `max` 上限约束，会一直重试，**直到成功、被取消或被删除**。

- 默认 `acksOnFailure(true)`：失败按 `withRetry` 重试次数耗尽后转 `Failed` 终态。
- 设为 `false`：内部将重试上限视为无限大（`u32::MAX`），任务不会因失败次数耗尽而终止。

### 注意事项

- 仅用于「必须成功」的任务（如关键扣款、状态机推进）；普通任务不要用，否则下游长期故障时会无限重试堆积。
- 与 `expires` 配合可设置「失败重试的总寿命」：超过 `expires` 秒仍未成功则转 `Expired` 终态，避免永久重试。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 关键对账任务：必须成功，失败无限重试，但 1 小时内仍未成功则放弃
TaskBuilder::shell('php /app/bin/reconcile-critical.php')
    ->withRetry(0, 5)           // max 在 acksOnFailure(false) 下被忽略
    ->retryBackoff(true)
    ->acksOnFailure(false)
    ->expires(3600)             // 1 小时兜底
    ->timeout(120)
    ->dispatch();
```

### 生产建议

- 必须配合 `expires` 或外部告警，否则下游死掉时任务会永久重试。
- 适合幂等任务；非幂等副作用任务慎用，避免重试导致重复扣款 / 重复发信。

---

## 4. timeout(int $secs) — 硬超时（SIGKILL）

### 签名

```php
public function timeout(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$secs` | `int` | 是 | `30` | 硬超时秒数；到点发 SIGKILL 强杀 |

### 行为说明

`timeout` 设置单次执行的硬超时。任务执行超过 `$secs` 秒未完成时，daemon 直接发 `SIGKILL` 终止子进程，任务转 `Failed`，并记录 `failed` 事件。

- 硬超时是不可中断的「强杀」：子进程没有机会做清理，资源（临时文件、连接）可能残留。
- shell 与 http 任务均适用：shell 任务杀子进程；http 任务取消底层请求 future。
- `timeout(0)` 在假死检测（watchdog）中会被跳过——没有 timeout 基线无法判定假死。

### 注意事项

- `timeout` 是兜底防线，应略大于任务正常执行的 P99 耗时，避免误杀慢任务。
- 强杀可能产生孤儿进程或临时文件残留，建议业务脚本自身做幂等清理。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 视频转码，硬超时 30 分钟
TaskBuilder::shell('ffmpeg -i input.mp4 output.mp4')
    ->timeout(1800)
    ->withRetry(1, 10)
    ->dispatch();

// HTTP 请求硬超时 15 秒
TaskBuilder::http('GET', 'https://slow-api.example.com/data')
    ->timeout(15)
    ->withRetry(2, 3)
    ->dispatch();
```

### 生产建议

- 每个任务都应设 `timeout`，禁止裸跑（默认 30s 对长任务会误杀，对短任务又过长）。
- 需要优雅停止请用 `softTimeout`（见下节），把 `timeout` 留作 SIGKILL 兜底。

---

## 5. softTimeout(int $secs) — 软超时（SIGTERM → grace → SIGKILL）

### 签名

```php
public function softTimeout(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 软超时秒数；到点先发 SIGTERM |

### 行为说明

`softTimeout` 设置软超时（参考 Celery `soft_time_limit`）。shell 执行器在 `soft_timeout` 秒到达时先发 `SIGTERM`，给子进程一个优雅退出的机会；若子进程在 `timeout - soft_timeout` 秒的宽限期内仍未退出，则升级为 `SIGKILL` 强杀。

升级链：`SIGTERM（soft_timeout）` → grace（`timeout - soft_timeout`）→ `SIGKILL（timeout）`。

- 仅对 shell 任务生效；HTTP 客户端无法通过 SIGTERM 优雅中断。
- **必须严格小于 `timeout`**，否则 SIGTERM 阶段永远没有触发机会，soft_timeout 会被忽略。
- 当 `soft_timeout >= timeout` 时，soft_timeout 被重置为 `None` 并发出告警。

### 注意事项

- ⚠️ **HTTP 任务设置 `softTimeout` 会抛 `InvalidTaskConfigException`**：HTTP 客户端无法被 SIGTERM 优雅中断，因此 HTTP 任务不允许配置 soft_timeout。
- `softTimeout` 必须严格小于 `timeout`，否则不生效（被忽略并告警）。
- 业务脚本应捕获 `SIGTERM` 做清理（关闭连接、刷盘、写状态），才能从软超时中受益。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// shell 任务：25 秒软超时优雅停止，30 秒硬超时兜底
TaskBuilder::shell('php /app/bin/long-job.php')
    ->softTimeout(25)
    ->timeout(30)
    ->withRetry(1, 5)
    ->dispatch();
```

业务脚本捕获 SIGTERM 示例：

```php
<?php
// /app/bin/long-job.php
pcntl_async_signals(true);
pcntl_signal(SIGTERM, function () {
    // 优雅清理：刷盘、关闭连接、写退出状态
    fwrite(STDERR, "收到 SIGTERM，开始清理...\n");
    cleanup_and_exit();
});
```

### 生产建议

- 凡是「可中断、需清理」的长任务（数据处理、批导出）都应设 `softTimeout`，并把 `timeout` 设为 `softTimeout + grace`（grace 通常 5~10 秒）。
- HTTP 任务不要设 `softTimeout`，靠 `timeout` 硬超时即可。

---

## 6. expires(int $secs) — Pending 过期丢弃

### 签名

```php
public function expires(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | Pending 状态最大存活秒数；超过则转 `Expired` 终态 |

### 行为说明

`expires` 设置任务在 `Pending` 状态下的最大存活时间。任务派发后若迟迟未被调度执行（如 daemon 过载、限流堆积），且 Pending 时长超过 `$secs` 秒，则转入 `Expired` 终态并记录 `expired` 事件，不再执行。

- 主要用于丢弃「过时即无意义」的任务（如实时性要求高的提醒、行情推送）。
- 与 `acksOnFailure(false)` 的无限重试配合，可作为「重试总寿命」兜底。

### 注意事项

- `expires` 计的是 Pending 存活时长，不是执行时长；执行中的 `Running` 任务不受 `expires` 影响（执行超时由 `timeout` 管）。
- `expires = 0`（默认）表示不启用过期丢弃。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 行情推送：5 分钟内未发出就丢弃（过时数据无意义）
TaskBuilder::shell('php /app/bin/push-quote.php --symbol=AAPL')
    ->expires(300)
    ->timeout(10)
    ->dispatch();

// 关键任务无限重试 + 1 小时过期兜底
TaskBuilder::shell('php /app/bin/critical.php')
    ->acksOnFailure(false)
    ->retryBackoff(true)
    ->expires(3600)
    ->timeout(120)
    ->dispatch();
```

### 生产建议

- 实时类任务务必设 `expires`，避免堆积后批量回放造成数据错乱。
- `expires` 应大于任务正常排队 + 执行耗时，否则正常任务也会被误丢弃。

---

## 7. acksLate(bool $on = true) — 延迟确认与崩溃恢复

### 签名

```php
public function acksLate(bool $on = true): self
```

### 参数表

| 参数 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `$on` | `bool` | 否 | `true` | 是否启用延迟确认 |

### 行为说明

`acksLate(true)` 启用延迟确认（参考 Celery `acks_late`）：任务在「执行完成并记录结果」后才确认；daemon 在执行期间崩溃重启时，处于 `Running` 状态的任务会被重置为 `Pending` 并重新派发，避免「崩溃丢任务」。

- 与 `persist(true)` 配合：持久化保证任务定义与状态落盘，`acksLate` 保证 Running 任务可被重新派发，二者一起构成崩溃恢复能力。
- daemon 重启时，`reset_running_to_pending` 会把 Running 任务重置为 Pending（无论 `acksLate` 是否开启，P0 修复后所有 Running 任务都会被重置以防孤儿）。

### 注意事项

- `acksLate` 会让「执行中崩溃」的任务被重复执行一次，因此业务脚本必须是幂等的（或能容忍重复执行）。
- 崩溃恢复的完整机制（PID 复用防护 `lease_held`、Running 重置、SIGKILL 自愈）详见 [持久化与崩溃恢复](persistence-recovery.md)。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 重要任务：持久化 + 延迟确认，崩溃后自动重派
TaskBuilder::shell('php /app/bin/important-job.php')
    ->persist(true)
    ->acksLate(true)
    ->withRetry(2, 5)
    ->timeout(120)
    ->dispatch();
```

### 生产建议

- 关键任务（不能丢、可幂等）建议 `persist(true) + acksLate(true)` 一起开。
- 非幂等任务慎用 `acksLate`，否则崩溃后重试可能产生重复副作用。

---

## 8. misfireGraceTime(int $secs) — 误触发宽限

### 签名

```php
public function misfireGraceTime(int $secs): self
```

### 参数表

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `$secs` | `int` | 是 | 宽限秒数；`0`=用全局默认 60s |

### 行为说明

`misfireGraceTime` 设置误触发宽限：当一次触发的实际派发时刻比原计划 `next_fire` 晚了超过此值（如 daemon 重启或调度延迟），则判定为漏触发，记录 `Missed` 事件而非真正执行。

- 传 `0` 时使用全局默认 `60` 秒。
- 与 `coalesce(true)` 配合可把多次漏触发合并为一次补执行（详见 [并发控制](concurrency.md)）。

### 注意事项

- 宽限过小会把正常抖动误判为漏触发；过大则让迟到很久的触发仍被执行，可能造成数据回放。
- 此处与触发器篇的 `misfireGraceTime` 是同一方法，既影响触发判定也影响重试补触发语义。

### 代码演示

```php
<?php
use Xhjob\TaskBuilder;

// 每分钟跑，允许迟到最多 10 秒，超过则记 Missed 而非执行
TaskBuilder::shell('php /app/bin/minute-rollup.php')
    ->cron('* * * * *')
    ->misfireGraceTime(10)
    ->coalesce(true)
    ->timeout(50)
    ->dispatch();
```

### 生产建议

- 分钟级关键任务把 `misfireGraceTime` 设为略大于单次执行耗时，避免执行慢导致下一轮被误判漏触发。
- 接入 `Missed` 事件告警：频繁 Missed 通常意味着 daemon 负载过高或被频繁重启。

---

## 全局注意事项

1. **HTTP 任务与 softTimeout**：HTTP 客户端无法被 SIGTERM 优雅中断，HTTP 任务设置 `softTimeout` 会抛 `InvalidTaskConfigException`；HTTP 任务只靠 `timeout` 硬超时。
2. **softTimeout 必须严格小于 timeout**：`soft_timeout >= timeout` 时 SIGTERM 阶段无触发机会，soft_timeout 会被忽略并告警。
3. **重试与幂等**：非幂等 HTTP 方法（POST/PUT/DELETE/PATCH）默认不重试，需显式 `idempotent(true)`；`acksOnFailure(false)` 的无限重试同样要求任务幂等。
4. **假死检测（Watchdog）**：daemon 后台 watchdog 周期扫描 Running 任务，运行时长超过 `timeout * factor` 判定假死，取消任务并记 `hung_detected` 事件。配置：
   - `XHJOB_WATCHDOG_INTERVAL`（默认 5 秒，`0`=禁用）
   - `XHJOB_WATCHDOG_FACTOR`（默认 2，即 `timeout * 2` 才判假死）
   - `timeout == 0` 的任务无基线，watchdog 跳过——因此每个任务都应设 `timeout`。
5. **崩溃恢复细节**：`acksLate` + `persist` 的完整恢复流程（Running 重置、`lease_held` PID 复用防护、SIGKILL 自愈）详见 [持久化与崩溃恢复](persistence-recovery.md)。

---

## 相关事件类型（EventType）

重试与超时相关的事件类型（多词值用下划线，与 `src/store/mod.rs` 严格对齐）：

| EventType | 触发场景 |
|---|---|
| `started` | 任务开始执行 |
| `succeeded` | 任务执行成功 |
| `failed` | 任务执行失败（含重试耗尽 / 硬超时 SIGKILL） |
| `missed` | 触发被判定为漏触发（超过 misfireGraceTime） |
| `interrupted` | 任务被优雅关停中断（drain 截止时仍在执行） |
| `hung_detected` | watchdog 检测到假死并取消任务 |
| `expired` | Pending 超过 expires 转终态 |
| `lease_held` | 崩溃恢复时孤儿子进程仍在运行，跳过重派以防重复执行 |
| `cancelled` | 任务被显式取消 |

> 注意拼写：`lease_held`（非 `leaseheld`）、`hung_detected`（非 `hungdetected`）。完整 14 种 EventType 见 [进度上报与事件](progress-events.md)。
