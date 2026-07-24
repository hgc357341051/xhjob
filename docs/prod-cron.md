# 定时任务

> 生产实战篇 · cron 表达式 · 时区 · 持久化 · 崩溃恢复 · 执行次数限制 · 漏触发合并 · 节假日 / 工作日

定时任务是后台调度中最常见的形态：每天凌晨跑报表、每 5 分钟同步数据、工作日 9 点发日报。Xhjob 把 cron 调度内建到 daemon，不需要外部 crontab，任务定义、状态、执行历史全部在 daemon 的 store 内可查可控。本文档覆盖生产级 cron 任务所需的全部配置项：时区、持久化、崩溃恢复、执行次数限制、漏触发合并、节假日 / 工作日过滤。

本文档按 **架构说明 → 完整可运行代码 → 注意事项 → 生产建议** 的结构展开。

---

## 架构说明

```
┌──────────────────────────────────────────────────────────────┐
│                      daemon (Rust)                           │
│                                                              │
│  ┌──────────────┐   每 tick 扫描    ┌──────────────────────┐ │
│  │  scheduler   │ ─────────────────► │  cron trigger 匹配   │ │
│  │  (cron 注册表)│                    │  · 5/6 字段表达式     │ │
│  └──────┬───────┘                    │  · withTimezone 时区  │ │
│         │                            │  · skipDates / workdays│ │
│         │ next_fire 到点              │  · coalesce 合并      │ │
│         ▼                            └──────────┬───────────┘ │
│  ┌──────────────┐                               │             │
│  │  max_pending │ ◄─────────────────────────────┘             │
│  │    队列      │                                             │
│  └──────┬───────┘                                             │
│         │                                                     │
│         ▼                                                     │
│  ┌──────────────┐    persist=true 时任务定义 + 状态 + 结果     │
│  │  pool + exec │    落 SQLite WAL，daemon 重启后自动恢复      │
│  └──────────────┘    acksLate=true 时 Running 任务重派         │
│                                                              │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │  store (SQLite WAL)                                      │ │
│  │  · tasks 表：cron / next_fire / max_executions / state   │ │
│  │  · events 表：started / succeeded / missed / ...         │ │
│  └──────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

cron 任务的生命周期：

1. **注册**：`TaskBuilder::shell(...)->cron('0 9 * * 1-5')->persist(true)->dispatch()` 把任务写入 daemon 的 cron 注册表，调度器计算第一次 `next_fire`。
2. **触发**：scheduler 每 tick（默认 1 秒）扫描所有注册的 cron 任务，匹配到点且未被 `skipDates` / `workdaysOnly` 过滤的任务，推入 `max_pending` 队列。
3. **执行**：pool 取出任务执行，结果与状态写入 store。
4. **循环**：执行完成后（或被 `maxInstances` 跳过后），调度器重算下一次 `next_fire`，直到达到 `maxExecutions` 或 `endAt` 边界。

> cron 任务与一次性 `runAt` 任务不同：cron 注册后**常驻**调度器，按周期反复触发，除非显式 `remove` 或达到 `maxExecutions`。

---

## 完整可运行代码

### 端到端示例：工作日数据汇总 + 节假日跳过 + 持久化 + 崩溃恢复

下面是一个完整的生产级 cron 任务：工作日凌晨 2 点跑数据汇总，跳过法定节假日，最多执行 1000 次，持久化 + 延迟确认（daemon 重启后自动重派未完成的任务）。

```php
<?php
// bin/register-cron.php  （部署时执行一次，注册 cron 任务到 daemon）
use Xhjob\TaskBuilder;

$name    = 'cron-svc';
$dataDir = '/var/lib/xhjob';

// 1. 确保 daemon 在运行
if ((xhjob_status($name, $dataDir)['running'] ?? 'false') !== 'true') {
    xhjob_start($name, $dataDir);
}

// 2. 法定节假日（Unix 时间戳 0 点），按任务时区 Asia/Shanghai 计算
$holidays = [
    strtotime('2026-01-01 00:00:00'),  // 元旦
    strtotime('2026-02-17 00:00:00'),  // 春节
    strtotime('2026-04-04 00:00:00'),  // 清明
    strtotime('2026-05-01 00:00:00'),  // 劳动节
    strtotime('2026-06-19 00:00:00'),  // 端午
    strtotime('2026-09-25 00:00:00'),  // 中秋
    strtotime('2026-10-01 00:00:00'),  // 国庆
];

// 3. 注册 cron 任务
$taskId = TaskBuilder::shell(
        'php /app/bin/daily-summary.php --date=$(date +%F) --idempotent'
    )
    ->withId('daily-summary')          // 稳定 ID，部署脚本可重复执行覆盖
    ->replaceExisting(true)            // 同 id 覆盖旧定义
    ->cron('0 2 * * 1-5')              // 工作日凌晨 2:00（5 字段）
    ->withTimezone('Asia/Shanghai')    // 显式时区，不依赖系统本地
    ->workdaysOnly()                   // 仅周一至周五（与 cron 的 1-5 冗余但更明确）
    ->skipDates($holidays)             // 跳过节假日
    ->persist(true)                    // 落 SQLite，daemon 重启不丢任务定义
    ->acksLate(true)                   // 崩溃恢复：Running 任务重启后重派
    ->maxExecutions(1000)              // 最多跑 1000 次后自动停止（约 4 年）
    ->coalesce(true)                   // 宕机期间多次 cron 只补一次
    ->misfireGraceTime(3600)           // 漏触发 1 小时内仍补跑，超时记 missed
    ->withRetry(3, 5)                  // 单次执行失败重试 3 次
    ->retryBackoff(true)               // 指数退避
    ->timeout(1800)                    // 单次硬超时 30 分钟
    ->softTimeout(1500)                // 软超时 25 分钟，给业务收尾
    ->maxInstances(1)                  // 防重叠：前一轮没跑完不派新一轮
    ->tag('report')->tag('daily')
    ->dispatch($name, $dataDir);

if (str_starts_with($taskId, 'error:')) {
    throw new RuntimeException('注册失败：' . substr($taskId, 6));
}
echo "cron 任务已注册: {$taskId}\n";

// 4. 验证：查 registered 模式确认任务进入 cron 注册表
$registered = xhjob_inspect('registered', $name, $dataDir);
if (!str_starts_with($registered, 'error:')) {
    $list = json_decode($registered, true) ?: [];
    foreach ($list as $t) {
        if (($t['id'] ?? '') === 'daily-summary') {
            printf("  next_fire=%s timezone=%s\n",
                $t['next_fire'] ?? '-',
                $t['timezone'] ?? '-'
            );
        }
    }
}
```

### 6 字段秒级 cron

Xhjob 支持 6 字段表达式（`秒 分 时 日 月 周`），用于秒级高频触发。当表达式按空格切分后字段数 ≥ 6 时按 6 字段解析；不足 6 字段时自动在前面补 `0` 秒字段。

```php
<?php
use Xhjob\TaskBuilder;

// 每 30 秒采一次心跳
TaskBuilder::shell('curl -sf http://127.0.0.1/healthz > /dev/null')
    ->cron('30 * * * * *')             // 6 字段：秒=30，每分钟第 30 秒
    ->withTimezone('Asia/Shanghai')
    ->maxInstances(1)                  // 防堆积
    ->ignoreResult(true)               // 心跳不关心结果
    ->dispatch();

// 每 10 秒
TaskBuilder::shell('php /app/bin/metrics-collect.php')
    ->cron('*/10 * * * * *')           // 6 字段：秒位 */10
    ->rateLimit(6, 60)                 // 兜底限流：60 秒内最多 6 次
    ->dispatch();
```

### persist + acksLate 崩溃恢复验证

```php
<?php
// bin/verify-recovery.php
// 演示：cron 任务执行中 daemon 被 SIGKILL → 重启后 Running 任务自动重派

$name = 'cron-svc';
$dataDir = '/var/lib/xhjob';

// 注册一个 30 秒执行的长任务（每分钟触发一次）
$taskId = TaskBuilder::shell('echo start; sleep 30; echo done')
    ->cron('* * * * *')
    ->withTimezone('Asia/Shanghai')
    ->persist(true)
    ->acksLate(true)                   // 关键：崩溃后重派
    ->maxInstances(1)
    ->timeout(120)
    ->dispatch($name, $dataDir);

echo "registered: {$taskId}\n";
sleep(5);                              // 等任务进入 Running

$state = xhjob_state($taskId, $name, $dataDir);
echo "before crash: state={$state['state']}\n";

// 模拟 daemon 崩溃（生产中是 kill -9 / OOM）
xhjob_stop($name, $dataDir);
echo "daemon stopped (task persisted in SQLite)\n";

// 重启 daemon
xhjob_start($name, $dataDir);
echo "daemon restarted, running recovery:\n";
echo "  - stale pid cleanup (starttime 双校验)\n";
echo "  - reset_running_to_pending (acksLate=true 的 Running 任务)\n";
echo "  - lease check (worker 已死 → 重派)\n";

// 轮询确认任务恢复执行
$deadline = time() + 60;
while (time() < $deadline) {
    $s = xhjob_state($taskId, $name, $dataDir);
    echo "after restart: state={$s['state']}\n";
    if (in_array($s['state'], ['success', 'failed', 'interrupted'], true)) {
        break;
    }
    sleep(3);
}

// 查事件流：应有 succeeded，无重复执行（lease_held=0）
$events = json_decode(xhjob_pull_events(time() - 120, null, $name, $dataDir), true) ?: [];
foreach ($events as $ev) {
    if ($ev['task_id'] === $taskId) {
        printf("  [%s] %s\n", $ev['ts'], $ev['event_type']);
    }
}
```

### 查询与运维

```php
<?php
// bin/cron-ops.php  （运维脚本）
$name = 'cron-svc';
$dataDir = '/var/lib/xhjob';

// 查所有注册的 cron / interval 任务
$registered = json_decode(xhjob_inspect('registered', $name, $dataDir), true) ?: [];
printf("已注册定时任务: %d 个\n", count($registered));
foreach ($registered as $t) {
    printf("  id=%s cron=%s next=%s tz=%s exec=%s/%s\n",
        $t['id'] ?? '-',
        $t['cron'] ?? $t['interval'] ?? '-',
        $t['next_fire'] ?? '-',
        $t['timezone'] ?? 'system',
        $t['execution_count'] ?? '0',
        $t['max_executions'] ?? '∞'
    );
}

// 临时修改 cron 频率（不重建任务，保留 id 与历史）
xhjob_reschedule('daily-summary', '*/15 * * * *', $name, $dataDir);

// 维护窗口：暂停所有 report 标签的任务
$list = json_decode(xhjob_list($name, null, 'report', $dataDir), true)['tasks'] ?? [];
foreach ($list as $t) {
    xhjob_pause($t['id'], $name, $dataDir);
}
// 维护结束后
foreach ($list as $t) {
    xhjob_resume($t['id'], $name, $dataDir);
}
```

---

## 注意事项

- **时区必须显式设置**：`withTimezone('Asia/Shanghai')` 缺省时调度器用**系统本地时区**（`/etc/localtime` / `TZ`）。容器环境时区经常漂移（基础镜像默认 UTC），不设时区会导致 cron 触发点偏移 8 小时。生产环境**每个 cron 任务都必须显式设时区**。
- **cron 6 字段支持秒级**：5 字段（`分 时 日 月 周`）与 6 字段（`秒 分 时 日 月 周`）都支持。按空格切分后字段数 ≥ 6 走 6 字段解析，不足 6 字段自动在前面补 `0` 秒位。秒级高频任务务必配 `maxInstances(1)` + `rateLimit` 防堆积。
- **`persist(true)` 需 `--all-features` 编译**：持久化是编译时 feature。用 `cargo build --release --all-features`（或 `--features persist`）编译扩展与 daemon。否则 `persist(true)` 被接受但**静默回退到 InMemoryStore**，daemon 一死任务定义全丢，`acksLate` 也失效。部署后用 `xhjob_inspect('stats')` 校验 `store_backend` 是否为 `sqlite`。
- **`coalesce(true)` 合并漏触发**：daemon 宕机期间多次 cron 到点（如停机 1 小时，每分钟的任务漏了 60 次），开启 coalesce 只补跑**一次**，不补 60 次。关闭则每次漏触发都补跑（可能打爆队列）。默认 `coalesce=true`。
- **`misfireGraceTime` 与 `coalesce` 配合**：`misfireGraceTime`（秒）定义漏触发的容忍窗口，0 = 用全局默认 60s。超过宽限期的漏触发记 `missed` 事件不再补跑。`coalesce=true` 时窗口内多次漏触发合并为一次。
- **`maxExecutions` 限制总执行次数**：达到上限后任务转 `success` 终态不再触发。`0` = 无限。适合「只跑 N 次就停」的限时活动任务。注意是**成功执行次数**累计，`max_instances_reached` 跳过的不计入。
- **`acksLate(true)` 必须幂等**：daemon SIGKILL 后重启会把 Running 任务重置为 Pending 重派，可能产生**重复执行**。业务脚本必须幂等（UPSERT / 去重键），否则会重复发报表、重复扣款。详见 [持久化与崩溃恢复](persistence-recovery.md)。
- **`skipDates` 时间戳按任务时区**：`skipDates` 接收 Unix 时间戳数组，按任务 `withTimezone` 的时区解释为「当天」。传 `2026-01-01 00:00:00 Asia/Shanghai` 的时间戳会跳过整个 1 月 1 日。务必用 `strtotime` 在目标时区下计算。
- **`workdaysOnly()` 仅周一至周五**：等价于 cron 周位 `1-5`。如果 cron 表达式已写 `1-5`，再加 `workdaysOnly()` 是冗余的（双重过滤），不会冲突。中国大陆的调休（周末上班）无法用 `workdaysOnly` 表达，需用 `skipDates` 的反向逻辑（或自行在工作日里排除节假日、在周末里手动派发）。

---

## 生产建议

- **部署脚本幂等注册**：cron 任务用 `withId('稳定业务名')` + `replaceExisting(true)`，部署脚本可重复执行，每次覆盖为最新定义，不会产生重复任务。不要用自动生成的 UUID 注册 cron，否则每次部署都会多一个。
- **daemon 由 systemd 托管**：cron 调度依赖 daemon 常驻。systemd unit 配 `Restart=on-failure` + `RestartSec=2s`，让 daemon 崩溃后 2 秒内自愈。`persist(true)` + `acksLate(true)` 保证恢复后任务定义与在途任务都不丢。
- **监控 `missed` 事件**：`xhjob_pull_events(time() - 3600, 'missed')` 拉取最近 1 小时的漏触发事件。频繁 missed 说明 daemon 频繁宕机或 `misfireGraceTime` 太短，应排查稳定性。
- **长任务防重叠**：耗时可能超过 cron 周期的任务（如 5 分钟跑一次但可能跑 8 分钟），必须设 `maxInstances(1)`。前一轮未完成时新一轮被跳过并记 `max_instances_reached` 事件，监控此事件频率可判断任务是否长期超时。
- **节假日表外置**：`skipDates` 不要硬编码在部署脚本里。维护一份节假日 JSON（如 `/etc/xhjob/holidays.json`），部署脚本读取后注入，每年初更新一次。
- **cron 与 `every` 的选择**：固定间隔用 `every($secs)` 更直观（如 `every(300)` = 每 5 分钟）；复杂时间点用 `cron`（如 `0 9 * * 1-5`）。两者同时设置时 `cron` 优先。秒级高频用 `every` 比 6 字段 cron 更易读。
- **定期清理终态任务**：cron 任务长期累积会撑大 SQLite 表。对已 `success` / `cancelled` 且超过 `resultTtl` 的任务，定期 `xhjob_remove` 清理。可注册一个专门的清理 cron（如每天凌晨 4 点跑 `bin/cleanup.php`）。
- **时区一致性**：所有 cron 任务统一用一个时区（通常 `Asia/Shanghai`），避免跨时区团队混乱。跨地域部署时按地域拆分 `service_name`，每个服务用各自时区。
