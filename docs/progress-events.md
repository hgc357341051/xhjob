---
title: 进度上报与事件
parent: 核心能力
nav_order: 36
---

# 进度上报与事件

Xhjob 提供任务内进度上报、全局事件流拉取、单任务事件过滤，以及 daemon 运行时检查（inspect）四类可观测性能力。相关实现位于 `src/daemon_main.rs` 与 `src/store/mod.rs`。

---

## reportProgress（任务内进度上报）

`reportProgress($id, $percent, $meta)` 允许任务脚本在执行过程中主动上报进度，参考 Celery `update_state(state='PROGRESS', meta=...)`。

### 参数

- `id`：任务 ID。
- `percent`：进度百分比，范围 **0-100**。**超出该范围（如 101、-1）会被拒绝**，daemon 返回错误 `"percent must be 0-100"`。
- `meta`：可选的元数据字符串（通常为 JSON），随进度一起持久化。

### 代码演示

任务脚本内通过 PHP 扩展函数上报进度：

```php
<?php
// worker.php —— 任务脚本内部上报进度
// 假设通过 $argv[1] 接收任务 ID
$taskId = $argv[1];

// 处理到一半时上报 50%
xhjob_report_progress($taskId, 50, '{"step":"half"}');

// ... 继续处理 ...

// 完成时上报 100%
xhjob_report_progress($taskId, 100, '{"step":"done"}');
```

也可在业务代码中为某个任务上报：

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');
$mgr->reportProgress($taskId, 75, '{"step":"three-quarter"}');
```

### 注意事项

- 进度上报需任务 ID 已存在于 store 中，否则上报无效。
- 进度信息写入 store，可通过 `state` / `result` 查询接口读取。
- 上报方需通过 ownership 校验（仅任务 owner 可上报）。

---

## pullEvents（全局事件流）

`pullEvents($sinceTs, $eventType = null)` 拉取 daemon 的 **全局事件流**，返回 `sinceTs`（Unix 秒）之后的所有事件，可选按事件类型过滤。参考 APScheduler `EVENT_JOB_*`。

### 参数

- `sinceTs`：起始时间戳（Unix 秒），返回此时间之后的事件。传 `0` 表示拉取全部。
- `eventType`：可选，按事件类型字符串过滤（如 `'failed'`、`'succeeded'`）。为空字符串或 null 表示不过滤。

### 代码演示

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

// 拉取最近 5 分钟的所有事件
$events = $mgr->pullEvents(time() - 300);

// 仅拉取失败事件
$failedEvents = $mgr->pullEvents(0, 'failed');

foreach ($events as $ev) {
    echo $ev['task_id'] . ' ' . $ev['event_type'] . ' @ ' . date('c', $ev['ts']) . PHP_EOL;
}
```

---

## logs（单任务事件过滤）

`logs($id, $sinceTs = 0)` 拉取 **单个任务** 的事件日志，底层调用原生函数 `xhjob_events($sinceTs, $id, ...)`，是 `pullEvents` 的按 `task_id` 过滤版本，适合查看某个任务的完整生命周期事件。

### 参数

- `id`：目标任务 ID。
- `sinceTs`：起始时间戳（Unix 秒），默认 `0` 表示全部。

### 代码演示

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

// 查看 $taskId 最近 1 小时的所有事件
$taskEvents = $mgr->logs($taskId, time() - 3600);

foreach ($taskEvents as $ev) {
    echo $ev['event_type'] . ' @ ' . date('c', $ev['ts']) . PHP_EOL;
}
```

---

## EventType 枚举

所有事件都带有一个 `event_type` 字段，取值来自 `EventType` 枚举（定义于 `src/store/mod.rs`）。枚举值序列化为 **snake_case 字符串**，多词之间用 **下划线** 连接。

### 完整事件类型表

| 事件类型字符串 | EventType 枚举 | 含义 |
|---------------|----------------|------|
| `started` | `Started` | 任务开始执行 |
| `succeeded` | `Succeeded` | 任务执行成功 |
| `failed` | `Failed` | 任务执行失败 |
| `missed` | `Missed` | 触发被判定为 misfire（错过） |
| `cancelled` | `Cancelled` | 任务被取消 |
| `paused` | `Paused` | 任务被暂停 |
| `resumed` | `Resumed` | 任务被恢复 |
| `expired` | `Expired` | 任务过期（Pending 超 `expires` 秒）转终态 |
| `max_instances_reached` | `MaxInstancesReached` | 达到 `maxInstances` 上限，触发被丢弃 |
| `rate_limited` | `RateLimited` | 触发被 `rateLimit` 限流 |
| `interrupted` | `Interrupted` | 任务因优雅关停 / watchdog 被标记为中断 |
| `hung_detected` | `HungDetected` | watchdog 检测到任务假死并取消 |
| `lease_held` | `LeaseHeld` | 崩溃恢复时租约仍持有，跳过重派 |
| `unknown` | `Unknown` | 未知事件类型（兜底） |

> ⚠️ **拼写注意**：多词事件类型用 **下划线** 连接，例如 `lease_held`（不是 `leaseheld`）、`hung_detected`（不是 `hungdetected`）、`max_instances_reached`（不是 `maxinstancesreached`）、`rate_limited`（不是 `ratelimited`）。这是一个早期已修复的 serde 序列化 bug——修复前多词枚举会被错误地拼接为无分隔符的字符串，导致事件流过滤失效；修复后统一使用 snake_case 下划线拼写。在过滤 `pullEvents` 时务必使用带下划线的正确拼写。

---

## inspect（运行时检查）

`inspect($mode)` 对齐 Celery `inspect`，提供四种 daemon 运行时查询模式，返回 `{"data": ...}`。

### 四种模式

| 模式 | 含义 | 返回内容 |
|------|------|---------|
| `active` | 运行中任务 | 当前处于 `Running` 状态的任务摘要列表 |
| `registered` | 已注册任务 | cron / interval 周期任务摘要列表 |
| `scheduled` | 已调度任务 | 具有未来 `next_fire` 的任务摘要列表 |
| `stats` | 聚合统计（默认） | daemon worker 统计（累计执行数、配置上限、进程 RSS 等） |

### 代码演示

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

$active     = $mgr->inspect('active');      // 运行中任务
$registered = $mgr->inspect('registered');  // cron 注册任务
$scheduled  = $mgr->inspect('scheduled');   // 未来触发任务
$stats      = $mgr->inspect('stats');       // 聚合统计（默认）
```

### 注意事项

- 不传 `mode` 或传未知值时，默认走 `stats` 模式。
- `active` / `registered` / `scheduled` 返回的任务摘要会按租户（owner）过滤，只返回当前调用方可见的任务。
- `stats` 返回 daemon 累计任务执行数、配置的限制（如 worker 并发上限）以及当前进程 RSS（内存占用），用于容量评估与监控。
