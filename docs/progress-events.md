# 进度上报与事件

> 核心能力篇 · 任务内进度上报 · 单任务 / 全局事件流 · 14 种 EventType · inspect 四模式检视

Xhjob 提供完整的「进度可观测」链路：任务内 `reportProgress` 上报百分比，`events` / `pullEvents` 拉取事件流，`inspect` 检视 daemon 内部状态。本文档按 **签名 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构展开。

---

## 1. reportProgress — 任务内进度上报

### 签名

```php
// PHP 函数（任务脚本内调用）
function xhjob_report_progress(
    string $id,
    int $percent,
    ?string $meta_json = null,
    ?string $name = null,
    ?string $data_dir = null
): bool

// TaskManager（PHP 侧包装）
public function reportProgress(string $id, int $percent, ?string $meta = null): bool
```

### 行为说明

`reportProgress` 由**任务脚本自身**在执行过程中调用，把当前进度百分比写入 daemon：

- `$id` 是当前任务的 task_id（任务脚本可通过环境变量 `XHJOB_TASK_ID` 获取）；
- `$percent` 必须 **0-100**，越界（< 0 或 > 100）直接返回 `false`，不写入；
- `$meta_json` 是可选的 JSON 字符串，携带业务自定义元数据（如已处理行数、当前阶段名）；
- 成功写入返回 `true`。

### 注意事项

- 越界**不抛异常**，直接返回 `false`，调用方需检查返回值。
- 进度写入是**异步落盘**的（WAL），高频上报（如每行一次）会拖慢任务，建议按阶段或按批次上报。
- `$id` 必须是**当前正在执行**的任务 id，上报到其他任务 id 会被拒绝（返回 false）。

### 代码演示

```php
<?php
// /app/bin/import-csv.php —— 由 xhjob 派发的任务脚本
$taskId = getenv('XHJOB_TASK_ID') ?: '';
$file = $argv[1] ?? '/data/input.csv';

// 模拟：分批处理 CSV，每批上报进度
$totalLines = (int) shell_exec("wc -l < {$file}");
$batchSize = 1000;
$done = 0;
$handle = fopen($file, 'r');
while (($line = fgetcsv($handle)) !== false) {
    // ... 处理一行 ...
    $done++;
    if ($done % $batchSize === 0) {
        $percent = (int) floor($done / $totalLines * 100);
        $ok = xhjob_report_progress($taskId, $percent, json_encode([
            'done' => $done,
            'total' => $totalLines,
            'stage' => 'importing',
        ]));
        if (!$ok) {
            fwrite(STDERR, "reportProgress failed at {$done}/{$totalLines}\n");
        }
    }
}
fclose($handle);

// 收尾：100%
xhjob_report_progress($taskId, 100, json_encode(['stage' => 'done']));
echo "imported {$done} rows\n";
```

派发方轮询进度：

```php
<?php
$taskId = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('php /app/bin/import-csv.php /data/big.csv')
        ->timeout(600)
        ->persist(true)
        ->toJson()
);

while (true) {
    $state = xhjob_state($taskId);
    $progress = $state['progress'] ?? -1;     // 0-100，未上报为 -1
    echo "progress = {$progress}%, state = {$state['state']}\n";
    if (in_array($state['state'], ['success', 'failed', 'interrupted'], true)) break;
    usleep(1_000_000);
}
```

### 生产建议

- 进度上报间隔 ≥ 1 秒，避免高频写 SQLite 拖慢任务。
- meta 里带「当前阶段 + 已处理 / 总量」，前端可渲染「正在导入 12000/50000 行」。
- 越界返回 false 是静默的，任务脚本应记录失败日志，便于排查为何进度卡住。

---

## 2. events — 单任务事件过滤

### 签名

```php
// PHP 函数
function xhjob_events(
    int $since_ts,
    ?string $task_id = null,
    ?string $name = null,
    ?string $data_dir = null
): string
//   返回 JSON 数组 [{task_id, event_type, payload, ts}, ...]，失败返回 "error: ..."
```

### 行为说明

`events` 拉取自 `since_ts`（Unix 秒）以来的事件，可按 `task_id` 过滤：

- 返回 JSON 数组，每个元素含 `task_id` / `event_type` / `payload` / `ts`；
- `task_id = null` 时返回所有任务事件（等同全局流，但 `pullEvents` 更适合此场景）；
- `task_id` 非空时只返回该任务的事件，适合追踪单任务生命周期。

### 注意事项

- `since_ts` 是**秒级** Unix 时间戳，毫秒精度会被截断。
- 失败时返回字符串 `"error: ..."`（不是 JSON），调用方需先判断前缀。
- 事件**不会自动清理**，长期运行会累积，建议定期归档（见生产建议）。

### 代码演示

```php
<?php
// 追踪单个任务的全部事件
$taskId = $argv[1];
$since = time() - 3600;   // 最近 1 小时
$raw = xhjob_events($since, $taskId);

if (str_starts_with($raw, 'error:')) {
    fwrite(STDERR, "events error: {$raw}\n");
    exit(1);
}

$events = json_decode($raw, true);
foreach ($events as $ev) {
    echo sprintf(
        "[%s] %s payload=%s\n",
        date('Y-m-d H:i:s', $ev['ts']),
        $ev['event_type'],
        $ev['payload']
    );
}
// 典型输出：
// [2026-07-24 10:00:01] started payload={}
// [2026-07-24 10:00:15] lease_held payload={"worker_pid":12340}
// [2026-07-24 10:02:30] succeeded payload={"exit_code":0}
```

### 生产建议

- 单任务详情页用 `events(since, taskId)` 渲染时间线，比轮询 state 信息更丰富。
- `since_ts` 用上次拉取的最大 `ts + 1`，避免重复拉取同秒事件。

---

## 3. pullEvents — 全局事件流

### 签名

```php
// PHP 函数
function xhjob_pull_events(
    int $since_ts,
    ?string $event_type = null,
    ?string $name = null,
    ?string $data_dir = null
): string
//   返回 JSON 数组 [{task_id, event_type, payload, ts}, ...]，失败返回 "error: ..."

// TaskManager（返回数组）
public function pullEvents(int $sinceTs = 0, ?string $eventType = null): array
```

### 行为说明

`pullEvents` 是**全局**事件流，可按 `event_type` 过滤：

- 与 `events` 的区别：`events` 按 `task_id` 过滤，`pullEvents` 按 `event_type` 过滤；
- `event_type = null` 返回所有类型事件；
- 适合做监控大盘：拉所有 `failed` / `hung_detected` / `interrupted` 事件触发告警。

### 注意事项

- 事件名**带下划线**（`lease_held` / `hung_detected` / `max_instances_reached`），拼写必须严格匹配，否则过滤为空。
- 大流量场景 `since_ts` 范围过宽会返回海量事件，建议窗口 ≤ 5 分钟。
- `pullEvents` 不消费事件（不删除），多次拉取同一时间窗口会得到相同结果。

### 代码演示

```php
<?php
// 方式 A: PHP 函数 —— 拉最近 5 分钟所有失败相关事件
$since = time() - 300;
$raw = xhjob_pull_events($since, 'failed');
if (!str_starts_with($raw, 'error:')) {
    foreach (json_decode($raw, true) as $ev) {
        echo "[failed] task={$ev['task_id']} ts={$ev['ts']} payload={$ev['payload']}\n";
    }
}

// 方式 B: TaskManager —— 拉 hung_detected 事件（数组直接用）
$tm = new \Xhjob\TaskManager();
$hung = $tm->pullEvents(time() - 300, 'hung_detected');
foreach ($hung as $ev) {
    // 推送告警
    error_log("[ALERT] task {$ev['task_id']} hung detected: {$ev['payload']}");
}

// 拉全部类型（不过滤）
$all = $tm->pullEvents(time() - 60, null);
echo "last 60s events: " . count($all) . "\n";
```

### 生产建议

- 用 `pullEvents(since, 'failed'|'hung_detected'|'interrupted')` 实现监控告警，配合 PagerDuty / 飞书机器人推送。
- 长期事件归档：每 5 分钟 `pullEvents` 全量，写入 ClickHouse / ElasticSearch 供历史查询。
- 事件名拼写集中维护成常量类，避免散落字符串字面量。

---

## 4. EventType 枚举全表（14 种）

### 签名

```
EventType（Rust 枚举，序列化为 snake_case 字符串，手写实现避免 serde lowercase 误写）
```

### 行为说明

Xhjob 共 14 种事件类型，覆盖任务完整生命周期与异常场景。**多词值用下划线**（`lease_held` / `hung_detected` / `max_instances_reached`），严格对齐。

| EventType 值 | 触发场景 |
|---|---|
| `started` | 任务被 worker 取出开始执行（state: Pending → Running） |
| `succeeded` | 任务正常完成（exit_code = 0），state 置为 success |
| `failed` | 任务失败（非 0 退出 / 重试耗尽 / acksOnFailure 触发移除） |
| `missed` | cron 触发器错过执行窗口（超过 misfire_grace_time），跳过本次 |
| `cancelled` | 任务被 `xhjob_cancel` 显式取消 |
| `paused` | 任务 / 调度被 `xhjob_pause` 暂停 |
| `resumed` | 暂停后由 `xhjob_resume` 恢复 |
| `expired` | 任务超过 `expires` 时间未执行，被丢弃 |
| `max_instances_reached` | 达到 `maxInstances` 上限，新触发被拒绝合并 |
| `rate_limited` | 触发 `rateLimit` 滑动窗口限流，本次执行被推迟 |
| `interrupted` | 任务被强制中断（watchdog 杀死 / daemon 停止 / 外部信号） |
| `hung_detected` | watchdog 判定假死（runtime > timeout * factor），任务被取消 |
| `lease_held` | 崩溃恢复时 worker 仍存活，lease 持有，跳过重新入队防重复执行 |
| `unknown` | 兜底：未识别的事件类型（升级版本兼容旧客户端） |

### 注意事项

- **拼写严格区分下划线**：
  - ✅ `lease_held`（不是 `leaseheld`）
  - ✅ `hung_detected`（不是 `hungdetected`）
  - ✅ `max_instances_reached`（不是 `maxinstancesreached`）
- 早期版本曾用 serde `rename_all = "lowercase"`，会把多词枚举名压成全小写无下划线，导致前端过滤失败 → 现已改为**手写实现**，保证下划线。
- `unknown` 是兜底值，正常不应出现；若大量出现说明客户端版本与 daemon 不匹配。

### 代码演示

```php
<?php
// 把 14 种事件名集中维护成常量，避免散落字符串
final class XhjobEventType {
    public const STARTED               = 'started';
    public const SUCCEEDED             = 'succeeded';
    public const FAILED                = 'failed';
    public const MISSED                = 'missed';
    public const CANCELLED             = 'cancelled';
    public const PAUSED                = 'paused';
    public const RESUMED               = 'resumed';
    public const EXPIRED               = 'expired';
    public const MAX_INSTANCES_REACHED = 'max_instances_reached';
    public const RATE_LIMITED          = 'rate_limited';
    public const INTERRUPTED           = 'interrupted';
    public const HUNG_DETECTED         = 'hung_detected';
    public const LEASE_HELD            = 'lease_held';
    public const UNKNOWN               = 'unknown';

    public const ALL = [
        self::STARTED, self::SUCCEEDED, self::FAILED, self::MISSED,
        self::CANCELLED, self::PAUSED, self::RESUMED, self::EXPIRED,
        self::MAX_INSTANCES_REACHED, self::RATE_LIMITED, self::INTERRUPTED,
        self::HUNG_DETECTED, self::LEASE_HELD, self::UNKNOWN,
    ];
}

// 用常量拉事件，杜绝拼写错误
$tm = new \Xhjob\TaskManager();
$alerts = $tm->pullEvents(time() - 300, XhjobEventType::HUNG_DETECTED);
foreach ($alerts as $ev) {
    // 推送告警
}
```

### 生产建议

- 把告警事件分为三档：
  - **红色**（立即告警）：`failed` / `hung_detected` / `interrupted` / `expired`
  - **黄色**（聚合告警）：`missed` / `max_instances_reached` / `rate_limited`
  - **蓝色**（仅观测）：`started` / `succeeded` / `lease_held` / `paused` / `resumed`
- 事件名常量类随 SDK 发布，业务侧 import 使用，禁止硬编码字符串。

---

## 5. inspect — daemon 状态检视（四种 mode）

### 签名

```php
// PHP 函数
function xhjob_inspect(
    string $mode,            // active | registered | scheduled | stats（默认 stats）
    ?string $name = null,
    ?string $data_dir = null
): string
//   返回 JSON 字符串

// TaskManager
public function inspect(string $mode = 'stats'): array
```

### 行为说明

`inspect` 按 `mode` 检视 daemon 内部状态，四种模式：

| mode | 说明 |
|---|---|
| `active` | 当前活跃任务（Running 状态）列表，含 task_id / 入队时间 / 已运行时长 |
| `registered` | 已注册的任务定义（通过 `xhjob_register` 注册的命名任务） |
| `scheduled` | 调度队列（cron 触发器即将执行的下次时间表） |
| `stats` | 统计信息（默认）：各状态任务计数、存储后端、加密状态、watchdog 配置等 |

### 注意事项

- `mode` 不区分大小写匹配，但建议传小写；非法 mode 返回 `error: unknown mode`。
- `active` / `scheduled` 在任务量大时返回数据较大，建议分页或限定时间窗口。
- `stats` 是最轻量的，适合做健康检查（容器 liveness probe）。

### 代码演示

```php
<?php
// 方式 A: PHP 函数
$stats = json_decode(xhjob_inspect('stats'), true);
echo "store_backend = {$stats['store_backend']}\n";
echo "encryption_enabled = " . ($stats['encryption_enabled'] ? 'true' : 'false') . "\n";
echo "watchdog_interval = {$stats['watchdog_interval']}\n";
echo "active_count = {$stats['states']['running']}\n";

// 方式 B: TaskManager（数组）
$tm = new \Xhjob\TaskManager();
$active = $tm->inspect('active');
foreach ($active['tasks'] as $t) {
    echo "active: {$t['task_id']} running={$t['runtime_secs']}s\n";
}

$registered = $tm->inspect('registered');
foreach ($registered['tasks'] as $t) {
    echo "registered: {$t['name']} timeout={$t['timeout']}\n";
}

$scheduled = $tm->inspect('scheduled');
foreach ($scheduled['items'] as $s) {
    echo "scheduled: {$s['task_name']} next_run=" . date('c', $s['next_run_ts']) . "\n";
}
```

### 输出示例（stats mode）

```json
{
  "store_backend": "sqlite",
  "encryption_enabled": true,
  "watchdog_interval": 5,
  "watchdog_factor": 2,
  "pool_mode": "async",
  "pool_capacity": 1024,
  "states": {
    "pending": 12,
    "running": 3,
    "success": 15823,
    "failed": 47,
    "interrupted": 2,
    "expired": 0,
    "cancelled": 5
  },
  "uptime_secs": 86420
}
```

### 生产建议

- 容器 liveness probe 调 `inspect('stats')`，断言 `store_backend == "sqlite"` 且 `encryption_enabled == true`。
- 大盘用 `inspect('stats')` 的 `states` 渲染各状态任务数趋势图。
- `inspect('active')` 用于排查「为什么任务卡住」：长 runtime 的任务往往是假死候选。

---

## 综合生产建议

### 1. 用 pullEvents 实现监控告警

```php
<?php
// monitor.php —— cron 每分钟跑一次
$tm = new \Xhjob\TaskManager();
$since = time() - 60;

// 红色告警：失败 / 假死 / 中断
$red = array_merge(
    $tm->pullEvents($since, 'failed'),
    $tm->pullEvents($since, 'hung_detected'),
    $tm->pullEvents($since, 'interrupted')
);
if (count($red) > 0) {
    send_alert('red', "{$red[0]['event_type']} on task {$red[0]['task_id']}");
}

// 黄色告警：限流 / 实例上限 / 错过
$yellow = array_merge(
    $tm->pullEvents($since, 'rate_limited'),
    $tm->pullEvents($since, 'max_instances_reached'),
    $tm->pullEvents($since, 'missed')
);
if (count($yellow) > 10) {
    send_alert('yellow', "throttling: " . count($yellow) . " events in last minute");
}
```

### 2. 用 reportProgress 实现前端进度条轮询

```php
<?php
// API 端点：GET /api/task/{id}/progress
$taskId = $route['id'];
$state = xhjob_state($taskId);

header('Content-Type: application/json');
echo json_encode([
    'task_id'  => $taskId,
    'state'    => $state['state'],
    'progress' => $state['progress'] ?? -1,   // 0-100，-1 表示未上报
    'meta'     => $state['progress_meta'] ?? null,
]);
```

前端轮询（伪代码，仅示意交互）：

```javascript
// 前端每秒轮询进度端点
async function pollProgress(taskId) {
  while (true) {
    const r = await fetch(`/api/task/${taskId}/progress`).then(r => r.json());
    updateProgressBar(r.progress);     // 0-100
    updateStage(r.meta?.stage);        // 'importing' / 'done'
    if (['success', 'failed', 'interrupted', 'expired'].includes(r.state)) break;
    await new Promise(r => setTimeout(r, 1000));
  }
}
```

### 3. 可观测性三件套

| 维度 | 工具 | 用途 |
|---|---|---|
| 进度 | `reportProgress` + 前端轮询 | 用户可见的「还剩多少」 |
| 事件 | `pullEvents` + 告警系统 | 运维可见的「发生了什么」 |
| 状态 | `inspect('stats')` + 大盘 | 全局可见的「整体健康度」 |

三者配合，构成 Xhjob 任务的完整可观测性闭环：用户看进度、运维看事件、老板看大盘。
