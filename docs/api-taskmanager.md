# TaskManager API

`Xhjob\TaskManager` 是 ThinkPHP 8 扩展包提供的高层任务管理门面，封装 `xhjob_dispatch` / `xhjob_state` / `xhjob_result` / `xhjob_list` 等全局函数，提供 `create` / `state` / `result` / `stop` / `waitForState` 等面向对象 API。

> 源码：`releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php`，命名空间 `Xhjob`。

## 设计要点

- 构造时绑定 **服务名** 与 **数据目录**，后续所有方法自动透传这两个参数，无需每次重复传入。
- 把底层 `xhjob_*` 函数的 `error:` 前缀 / `error` 键 / `false` 等异构错误约定，统一收敛为 **异常**（`InvalidTaskConfigException` / `ServiceNotRunningException` / `TaskNotFoundException`）或 **可判定的返回值**（`null` / `false` / 带 `error` 键的数组）。
- 提供两个轮询等待方法 `waitForState` / `waitForResult`，封装常见的"派发后等结果"模式。

## 错误契约

| 方法类别 | 失败行为 |
| --- | --- |
| 创建类（create / createChain / createGroup / createChord / update） | 抛 `InvalidTaskConfigException`（底层 `error:` 前缀转换而来） |
| 查询类（get / list / state / result / logs / chainState / groupState / chordState / pullEvents / inspect） | daemon 不可达抛 `ServiceNotRunningException` |
| `result()` 无结果记录 | 返回带 `error` 键的数组（**业务条件，不抛异常**） |
| `get()` / `chainState()` / `groupState()` / `chordState()` 不存在 | 返回 `null` |
| `remove()` 任务不存在 | 抛 `TaskNotFoundException` |
| `stop` / `restart` / `pause` / `resume` / `reschedule` / `reportProgress` | 返回 `bool`，`false` 即失败，不抛异常 |
| `waitForState` / `waitForResult` | 临时错误（如 daemon 重启中）继续轮询，超时返回 `false` / `null` |

### 异常类型（4 个，命名空间 `Xhjob\Exception`）

| 异常 | 继承 | 抛出场景 |
| --- | --- | --- |
| `XhjobException` | `\Exception` | 基类，所有 Xhjob 异常的根 |
| `InvalidTaskConfigException` | `XhjobException` | 创建类方法：配置非法 / daemon 返回 `error:` |
| `ServiceNotRunningException` | `XhjobException` | 查询类方法：daemon 不可达 / 任务不存在（state 视为不可达） |
| `TaskNotFoundException` | `XhjobException` | `remove()` 失败；`update()` 内部捕获以容忍旧任务不存在 |

```php
<?php
use Xhjob\Exception\XhjobException;
use Xhjob\Exception\InvalidTaskConfigException;
use Xhjob\Exception\ServiceNotRunningException;
use Xhjob\Exception\TaskNotFoundException;

try {
    // ... 调用 TaskManager 方法
} catch (InvalidTaskConfigException $e) {
    // 创建类失败
} catch (ServiceNotRunningException $e) {
    // daemon 不可达
} catch (TaskNotFoundException $e) {
    // 任务不存在
} catch (XhjobException $e) {
    // 兜底
}
```

---

## 构造与 getter（3）

### `__construct`

```php
public function __construct(string $name = 'default', ?string $dataDir = null)
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$name` | `string` | 服务名，默认 `default` |
| `$dataDir` | `?string` | 数据目录，`null` = 平台默认 |

**返回值**：无（构造方法）。

**错误契约**：不抛异常（服务名合法性延迟到实际调用底层函数时校验）。

**注意事项**：绑定的 `$name` / `$dataDir` 会透传给后续所有方法。

**代码演示**

```php
<?php
use Xhjob\TaskManager;

$mgr = new TaskManager('cron-svc', '/var/lib/xhjob');
```

**生产建议**：ThinkPHP 中通过 ServiceProvider 注册单例 `xhjob.manager`，用 `xhjob_manager()` 获取；多服务场景各自 new 独立实例。

---

### `getName`

```php
public function getName(): string
```

**参数**：无。

**返回值**：`string`。构造时绑定的服务名。

**错误契约**：不抛异常。

**代码演示**：`echo $mgr->getName(); // cron-svc`

---

### `getDataDir`

```php
public function getDataDir(): ?string
```

**参数**：无。

**返回值**：`?string`。构造时绑定的数据目录（未设为 `null`）。

**错误契约**：不抛异常。

**代码演示**：`$dir = $mgr->getDataDir();`

---

## 创建（5）

> 均可能抛 `InvalidTaskConfigException`（配置非法或 daemon 返回 `error:`）。

### `create`

```php
public function create(TaskBuilder $b): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$b` | `TaskBuilder` | 任务构建器（普通任务） |

**返回值**：`string`。task_id。

**错误契约**：底层 `xhjob_dispatch` 返回 `error:` 前缀时抛 `InvalidTaskConfigException('dispatch 失败：<error> (name=<service>)')`。

**注意事项**：内部调用 `$b->toJson()` 生成 JSON 后传给 `xhjob_dispatch`；等价于 `$b->dispatch($this->name, $this->dataDir)`，但把异常类型统一为 `InvalidTaskConfigException`（`TaskBuilder::dispatch` 也是抛此异常，行为一致）。

**代码演示**

```php
<?php
use Xhjob\TaskBuilder;

$id = $mgr->create(
    TaskBuilder::shell('echo hi')->withRetry(3, 2)
);
```

**生产建议**：业务层封装 `create` 调用并捕获异常，转换为业务错误码。

---

### `createChain`

```php
public function createChain(array $builders): string
```

**参数**：`$builders` — `TaskBuilder` 实例数组。

**返回值**：`string`。chain_id。

**错误契约**：底层 `xhjob_chain` 返回 `error:` 时抛 `InvalidTaskConfigException('chain 失败：<error> (name=<service>)')`。

**注意事项**：内部把各 builder `toArray()` 后 `json_encode` 成任务数组传给 `xhjob_chain`。

**代码演示**

```php
<?php
$chainId = $mgr->createChain([
    TaskBuilder::shell('step1.sh'),
    TaskBuilder::shell('step2.sh'),
]);
```

**生产建议**：chain 步骤不宜过多；任一步失败中断整链。

---

### `createGroup`

```php
public function createGroup(array $builders): string
```

**参数**：`$builders` — `TaskBuilder` 实例数组。

**返回值**：`string`。group_id。

**错误契约**：底层 `xhjob_group` 返回 `error:` 时抛 `InvalidTaskConfigException('group 失败：<error> ...')`。

**注意事项**：并行派发；终态 `success` / `partial_failed` / `failed`。

**代码演示**

```php
<?php
$groupId = $mgr->createGroup([
    TaskBuilder::http('GET', 'https://a'),
    TaskBuilder::http('GET', 'https://b'),
]);
```

**生产建议**：批量任务用 `rateLimit` 限流。

---

### `createChord`

```php
public function createChord(array $headerBuilders, TaskBuilder $callback): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$headerBuilders` | `array` | header `TaskBuilder` 数组（并行） |
| `$callback` | `TaskBuilder` | 回调 `TaskBuilder` |

**返回值**：`string`。chord_id。

**错误契约**：底层 `xhjob_chord` 返回 `error:` 时抛 `InvalidTaskConfigException('chord 失败：<error> ...')`。

**注意事项**：header 全部成功后才派发 callback（callback 的 `meta` 携带所有 header 结果）；任一 header 失败则 chord 转 `partial_failed`，不派发 callback。

**代码演示**

```php
<?php
$chordId = $mgr->createChord(
    [TaskBuilder::shell('shard-1.sh'), TaskBuilder::shell('shard-2.sh')],
    TaskBuilder::shell('merge.sh')
);
```

**生产建议**：Map-Reduce 场景的理想原语。

---

### `update`

```php
public function update(string $id, TaskBuilder $b): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 旧任务 ID |
| `$b` | `TaskBuilder` | 新任务构建器 |

**返回值**：`string`。新任务 ID（与传入 `$id` 相同，因内部强制 `withId($id)`）。

**错误契约**：`remove($id)` 抛 `TaskNotFoundException` 时被**静默捕获**（旧任务不存在视为正常）；后续 `create` 失败抛 `InvalidTaskConfigException`。

**注意事项**
- 实现：先 `remove($id)`（不存在静默吞 `TaskNotFoundException`），再 `$b->withId($id)->replaceExisting(true)` 重建。
- 启用 `replaceExisting(true)` 以防 daemon 中残留同 id 任务。

**代码演示**

```php
<?php
use Xhjob\TaskBuilder;

// 把现有 cron 任务改成新表达式
$newId = $mgr->update('nightly-cleanup', TaskBuilder::shell('cleanup.sh')->cron('0 4 * * *'));
// $newId === 'nightly-cleanup'
```

**生产建议**：动态修改任务配置用 `update`（保留 id）；仅改 cron 用更轻量的 `reschedule`。

---

## 查询（5）

> daemon 不可达时抛 `ServiceNotRunningException`。

### `get`

```php
public function get(string $id): ?array
```

**参数**：`$id` — 任务 ID。

**返回值**：`?array`。任务存在时返回完整任务定义关联数组（`json_decode` 结果）；不存在返回 `null`。

**错误契约**：底层 `xhjob_get` 返回 `null` / 空串 → 返回 `null`；返回 `error:` 前缀 → 抛 `ServiceNotRunningException('get 失败：<error> (id=..., name=...)')`。

**注意事项**：返回的是任务定义（配置快照），不是执行结果——执行结果用 `result()`。

**代码演示**

```php
<?php
$def = $mgr->get($id);
if ($def === null) {
    echo "任务不存在\n";
}
```

**生产建议**：用于任务配置审计；判断是否在运行用 `state()`。

---

### `list`

```php
public function list(?string $stateFilter = null, ?string $tag = null): array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$stateFilter` | `?string` | 状态过滤，如 `pending` / `running` / `success` / `failed` |
| `$tag` | `?string` | 标签过滤 |

**返回值**：`array`。任务数组（`json_decode` 结果，非法时返回 `[]`）。

**错误契约**：底层 `xhjob_list` 返回 `error:` 前缀 → 抛 `ServiceNotRunningException('list 失败：<error> (name=...)')`。

**注意事项**：`$stateFilter` 与 `$tag` 可同时使用（AND 语义）。

**代码演示**

```php
<?php
$failed = $mgr->list('failed');
foreach ($failed as $t) {
    echo $t['id'] . "\n";
}
```

**生产建议**：监控面板按 `failed` 拉取告警；避免全量拉取大响应。

---

### `state`

```php
public function state(string $id): array
```

**参数**：`$id` — 任务 ID。

**返回值**：`array`。含 `state` / `attempts` / `created_at` 等 30+ 字段（详见 [PHP 函数参考 / xhjob_state](api-functions.md#xhjob_state)）。

**错误契约**：底层 `xhjob_state` 返回带 `error` 键的数组（daemon 不可达 / 任务不存在）→ 抛 `ServiceNotRunningException('state 失败：<error> (id=..., name=...)')`。**不再静默吞错**。

**注意事项**：所有值为字符串，数值字段需 `(int)` 转换；`tags` 是 JSON 字符串需 `json_decode`。

**代码演示**

```php
<?php
try {
    $s = $mgr->state($id);
    printf("state=%s attempts=%d\n", $s['state'], (int)$s['attempts']);
} catch (ServiceNotRunningException $e) {
    // daemon 不可达
}
```

**生产建议**：轮询时关注 `state` 是否进入终态（`success` / `failed` / `cancelled` / `expired` / `interrupted`）。

---

### `result`

```php
public function result(string $id): array
```

**参数**：`$id` — 任务 ID。

**返回值**：`array`。含 `body` / `body_b64` / `status_code` / `stdout` / `stderr` / `exit_code`（按任务类型与产出情况出现）。

**错误契约**
- daemon 不可达（底层返回 `error:` 字符串）→ 抛 `ServiceNotRunningException('result 失败：<error> (id=..., name=...)')`。
- **无结果记录**（`ignoreResult=true` / 任务未产出输出）→ 返回带 `error` 键的数组，**这是预期的业务条件，不抛异常**；调用方用 `isset($r['error'])` 判断。

**注意事项**
- HTTP 二进制响应含 `body_b64`（base64），用 `base64_decode` 恢复。
- 取结果前建议先 `state()` 确认已进入终态。

**代码演示**

```php
<?php
$r = $mgr->result($id);
if (isset($r['error'])) {
    echo "无结果：{$r['error']}\n";  // 业务条件，非异常
    return;
}
echo $r['stdout'] ?? $r['body'] ?? '';
```

**生产建议**：区分"无结果"（业务条件）与"daemon 不可达"（异常）；前者用 `isset($r['error'])`，后者用 try/catch。

---

### `logs`

```php
public function logs(string $id, int $sinceTs = 0): array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$sinceTs` | `int` | 起始时间戳（Unix 秒），默认 0 = 全部 |

**返回值**：`array`。事件数组（`json_decode` 结果，非法时返回 `[]`）。

**错误契约**：底层 `xhjob_events` 返回 `error:` 前缀 → 抛 `ServiceNotRunningException('logs 失败：<error> (id=..., name=...)')`。

**注意事项**：内部调用 `xhjob_events($sinceTs, $id, ...)`，按 `task_id` 过滤事件。

**代码演示**

```php
<?php
$events = $mgr->logs($id, time() - 3600);
foreach ($events as $ev) {
    printf("[%d] %s\n", $ev['ts'], $ev['event_type']);
}
```

**生产建议**：排障时拉取任务全生命周期事件；`sinceTs` 做增量拉取。

---

## 控制（6）

### `stop`

```php
public function stop(string $id): bool
```

**参数**：`$id` — 任务 ID。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：不抛异常；`false` 即失败。

**注意事项**：内部调用 `xhjob_cancel`——Pending 转 `cancelled` 终态，Running 任务收到取消信号转 `cancelled`。

**代码演示**

```php
<?php
if (!$mgr->stop($id)) {
    error_log('取消失败：任务可能已终态');
}
```

**生产建议**：业务取消用 `stop`（保留记录），定期清理用 `remove`。

---

### `restart`

```php
public function restart(string $id): bool
```

**参数**：`$id` — 任务 ID。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：不抛异常。

**注意事项**：内部调用 `xhjob_requeue`——仅对终态任务（`cancelled` / `failed` / `expired`）有效，重置 `attempts=0`、`next_fire=now`，回到 `pending`。运行中 / pending 任务返回 `false`。

**代码演示**

```php
<?php
if ($mgr->restart($id)) {
    echo "已重新入队\n";
}
```

**生产建议**：修复后强制重试失败任务；与 `acksOnFailure(false)` 的无限重试任务配合人工干预。

---

### `pause`

```php
public function pause(string $id): bool
```

**参数**：`$id`。

**返回值**：`bool`。

**错误契约**：不抛异常。

**注意事项**：暂停后调度器跳过触发；运行中实例不中断。

**代码演示**：`$mgr->pause($id);` 维护窗口结束后 `$mgr->resume($id);`

**生产建议**：维护窗口批量暂停 cron 任务。

---

### `resume`

```php
public function resume(string $id): bool
```

**参数**：`$id`。

**返回值**：`bool`。

**错误契约**：不抛异常。

**注意事项**：恢复被 `pause` 暂停的任务。

**代码演示**：见 `pause`。

**生产建议**：恢复后用 `state` 确认 `paused=false` 与 `next_fire` 已重算。

---

### `remove`

```php
public function remove(string $id): bool
```

**参数**：`$id`。

**返回值**：`bool`。成功返回 `true`。

**错误契约**：底层 `xhjob_remove` 返回 `false` → 抛 `TaskNotFoundException('任务不存在或删除失败 (id=..., name=...)')`。

**注意事项**：删除任务定义，不影响运行中实例；与 `stop`（cancel）的区别是 remove 彻底删除记录。

**代码演示**

```php
<?php
use Xhjob\Exception\TaskNotFoundException;

try {
    $mgr->remove($id);
} catch (TaskNotFoundException $e) {
    // 任务已不存在，可忽略
}
```

**生产建议**：定时清理已完成任务避免 SQLite 表膨胀；`update` 内部依赖此方法并容忍不存在。

---

### `reschedule`

```php
public function reschedule(string $id, string $cron): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$cron` | `string` | 新的 5 字段 cron 表达式 |

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：不抛异常；`false` 即失败（cron 非法 / 任务不存在 / 非 cron 任务）。

**注意事项**：仅修改 cron 并重算 `next_fire`，不改其他配置。

**代码演示**

```php
<?php
$mgr->reschedule($id, '*/15 * * * *');
```

**生产建议**：动态调频用 `reschedule`，比 `update`（remove + 重建）更轻量且保留历史。

---

## 编排状态（3）

> daemon 不可达时抛 `ServiceNotRunningException`。

### `chainState`

```php
public function chainState(string $chainId): ?array
```

**参数**：`$chainId`。

**返回值**：`?array`。chain 存在时返回 `ChainRecord` 关联数组（`chain_id` / `tasks` / `current_step` / `state` / `created_at` / `updated_at`）；不存在返回 `null`。

**错误契约**：底层返回 `null` / 空串 → 返回 `null`；返回 `error:` 前缀 → 抛 `ServiceNotRunningException('chainState 失败：<error> (chainId=..., name=...)')`。

**代码演示**

```php
<?php
$rec = $mgr->chainState($chainId);
if ($rec !== null) {
    echo "step={$rec['current_step']} state={$rec['state']}\n";
}
```

**生产建议**：`failed` 时按 `current_step` 定位失败步骤。

---

### `groupState`

```php
public function groupState(string $groupId): ?array
```

**参数**：`$groupId`。

**返回值**：`?array`。group 存在时返回 `GroupRecord` 关联数组；不存在返回 `null`。

**错误契约**：底层返回 `error:` 前缀 → 抛 `ServiceNotRunningException('groupState 失败：<error> (groupId=..., name=...)')`。

**代码演示**

```php
<?php
$rec = $mgr->groupState($groupId);
echo $rec['state'] ?? 'unknown';
```

**生产建议**：`partial_failed` 时遍历子任务 state 定位失败项。

---

### `chordState`

```php
public function chordState(string $chordId): ?array
```

**参数**：`$chordId`。

**返回值**：`?array`。chord 存在时返回 `ChordRecord` 关联数组（含 `id` / `header_task_ids` / `callback_json` / `callback_task_id` / `state` / `created_at` / `updated_at`）；不存在返回 `null`。

**错误契约**：底层返回 `error:` 前缀 → 抛 `ServiceNotRunningException('chordState 失败：<error> (chordId=..., name=...)')`。

**代码演示**

```php
<?php
$rec = $mgr->chordState($chordId);
echo $rec['state'] ?? 'unknown';
```

**生产建议**：`partial_failed` 时检查 `header_task_ids` 中各 header 任务的 `last_error`。

---

## 进度 / 事件（3）

### `reportProgress`

```php
public function reportProgress(string $id, int $percent, ?string $meta = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$percent` | `int` | 进度百分比，**必须 0-100** |
| `$meta` | `?string` | 任意 JSON 元数据 |

**返回值**：`bool`。成功返回 `true`；`$percent` 越界（非 0-100）底层直接返回 `false`（不联系 daemon）。

**错误契约**：不抛异常；`false` 即失败（含越界 / 服务名非法 / IPC 失败）。

**注意事项**：参考 Celery `update_state(state='PROGRESS', meta=...)`；进度值可通过 `state()` 的 `progress` / `progress_meta` 字段读回。

**代码演示**

```php
<?php
$mgr->reportProgress($id, 50, json_encode(['chunk' => 5]));
```

**生产建议**：进度上报按 5%-10% 步进，避免 IPC 压力；越界值被静默丢弃，调用方需自行校验。

---

### `pullEvents`

```php
public function pullEvents(int $sinceTs = 0, ?string $eventType = null): array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$sinceTs` | `int` | 起始时间戳（Unix 秒），默认 0 = 全部 |
| `$eventType` | `?string` | 事件类型过滤（`started` / `succeeded` / `failed` / ...） |

**返回值**：`array`。事件数组（`json_decode` 结果，非法时返回 `[]`）。

**错误契约**：底层 `xhjob_pull_events` 返回 `error:` 前缀 → 抛 `ServiceNotRunningException('pullEvents 失败：<error> (name=...)')`。

**注意事项**：按 `event_type` 过滤全局事件流（与 `logs` 按 `task_id` 过滤不同）。

**代码演示**

```php
<?php
$fails = $mgr->pullEvents(time() - 600, 'failed');
foreach ($fails as $ev) {
    // 推送告警
}
```

**生产建议**：告警网关按 `eventType=failed` 定时拉取，配合 `sinceTs` 增量消费。

---

### `inspect`

```php
public function inspect(string $mode = 'stats'): array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$mode` | `string` | 查询模式：`active` / `registered` / `scheduled` / `stats`（默认） |

**返回值**：`array`。`active`/`registered`/`scheduled` 返回数组，`stats` 返回对象（`json_decode` 结果，非法时返回 `[]`）。

**错误契约**：底层 `xhjob_inspect` 返回 `error:` 前缀 → 抛 `ServiceNotRunningException('inspect 失败：<error> (mode=..., name=...)')`。

**注意事项**：`active` 当前运行任务；`registered` cron/interval 任务；`scheduled` 有未来 `next_fire` 的任务；`stats` 聚合 `WorkerStats`。

**代码演示**

```php
<?php
$stats = $mgr->inspect('stats');
print_r($stats);
```

**生产建议**：监控面板用 `stats` 看全局负载，`active` 看实时并发，`scheduled` 预判未来调度压力。

---

## 轮询等待（2）

### `waitForState`

```php
public function waitForState(string $id, string $expectedState, int $timeoutSec = 30): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$expectedState` | `string` | 期望状态（如 `success` / `running`） |
| `$timeoutSec` | `int` | 超时秒数，默认 30 |

**返回值**：`bool`。在超时前进入期望状态返回 `true`；超时或进入非期望终态返回 `false`。

**错误契约**：不抛异常。轮询期间 `state()` 抛异常（如 daemon 重启中）会被捕获，`usleep(500000)` 后继续轮询。

**注意事项**
- 终态集合：`failed` / `cancelled` / `expired` / `interrupted` / `success`。**进入终态且非期望时提前返回 `false`**，不再继续轮询。
- 重试间隔 500ms（`usleep(500000)`，临时错误时）与 300ms（正常轮询时）。

**代码演示**

```php
<?php
if ($mgr->waitForState($id, 'success', 60)) {
    echo "任务成功\n";
} else {
    echo "任务未在 60s 内成功（失败 / 取消 / 超时）\n";
}
```

**生产建议**：派发后用 `waitForState` 同步等结果；超时阈值按业务 SLA 设置，避免长时阻塞。

---

### `waitForResult`

```php
public function waitForResult(string $id, int $timeoutSec = 30): ?array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$timeoutSec` | `int` | 超时秒数，默认 30 |

**返回值**：`?array`。任务进入终态后返回 `result()` 结果数组；超时返回 `null`。

**错误契约**：不抛异常。轮询期间 `state()` 抛异常会被捕获继续轮询；终态后调 `result()` 若抛 `ServiceNotRunningException` 会向上传播（终态判定与结果读取间的瞬态窗口）。

**注意事项**
- 终态集合：`success` / `failed` / `cancelled` / `expired` / `interrupted`。
- 进入终态后调用 `result()`；若结果不存在（`ignoreResult=true`）则返回带 `error` 键的数组（非 `null`）。

**代码演示**

```php
<?php
$r = $mgr->waitForResult($id, 120);
if ($r === null) {
    echo "超时未完成\n";
} elseif (isset($r['error'])) {
    echo "无结果：{$r['error']}\n";
} else {
    echo "exit_code=" . ($r['exit_code'] ?? '-') . "\n";
    echo $r['stdout'] ?? '';
}
```

**生产建议**：同步业务流程用 `waitForResult`；异步通知场景不要阻塞，改用 `pullEvents` 事件驱动。

---

## 完整示例

```php
<?php
use Xhjob\TaskManager;
use Xhjob\TaskBuilder;
use Xhjob\Exception\InvalidTaskConfigException;
use Xhjob\Exception\ServiceNotRunningException;

$mgr = new TaskManager('cron-svc', '/var/lib/xhjob');

// 1. 创建并等待结果
try {
    $id = $mgr->create(
        TaskBuilder::shell('process-order.sh')
            ->withRetry(3, 5)
            ->timeout(120)
            ->withMeta(json_encode(['order_id' => 42]))
    );

    $result = $mgr->waitForResult($id, 180);
    if ($result === null) {
        throw new RuntimeException("订单处理超时 (id=$id)");
    }
    if (($result['exit_code'] ?? 1) !== 0) {
        throw new RuntimeException("订单处理失败: " . ($result['stderr'] ?? ''));
    }
} catch (InvalidTaskConfigException $e) {
    throw new RuntimeException('派发失败：' . $e->getMessage());
}

// 2. 批量失败任务重试
try {
    $failed = $mgr->list('failed');
    foreach ($failed as $t) {
        $mgr->restart($t['id']);
    }
} catch (ServiceNotRunningException $e) {
    error_log('daemon 不可达：' . $e->getMessage());
}

// 3. 更新现有任务的 cron
$mgr->update('nightly-report', TaskBuilder::shell('report.sh')->cron('0 4 * * *'));

// 4. 监控 daemon 并发
$stats = $mgr->inspect('stats');
$active = $mgr->inspect('active');
echo "active=" . count($active) . "\n";
```
