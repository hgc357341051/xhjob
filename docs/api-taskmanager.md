---
title: TaskManager API
parent: API 参考
nav_order: 23
---

# TaskManager API

`Xhjob\TaskManager` 是 xhjob PHP 扩展的任务管理门面，封装 `xhjob_dispatch` / `xhjob_state` / `xhjob_result` / `xhjob_list` 等原生函数，提供 `create` / `state` / `result` / `stop` / `restart` / `waitForState` 等高层 API。

{: .note }
本类位于命名空间 `Xhjob`，源文件 `Xhjob/TaskManager.php`。构造时绑定服务名与数据目录，后续所有调用自动透传这两个参数，无需每次重复传入。

## 设计要点

- **服务名 + 数据目录绑定**：构造时一次性绑定，所有方法复用。
- **错误归一化**：原生函数返回的 `error:` 前缀字符串 / 带 `error` 键的数组，在门面层统一转换为异常（`ServiceNotRunningException` / `TaskNotFoundException` / `InvalidTaskConfigException`）。
- **状态数组归一化**：`parseStateArray()` 兼容 `[[k, v], ...]` 键值对形式与 `['k' => 'v']` 关联数组形式，保证后续扩展升级兼容。

## 异常类型

| 异常类 | 触发场景 |
| --- | --- |
| `Xhjob\Exception\InvalidTaskConfigException` | 任务配置非法 / dispatch 返回 error |
| `Xhjob\Exception\ServiceNotRunningException` | daemon 不可达 / 查询失败 |
| `Xhjob\Exception\TaskNotFoundException` | 任务不存在（仅 `remove` 抛此异常） |

## 构造与基本信息

### __construct

```php
public function __construct(string $name = 'default', ?string $dataDir = null)
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `string` | `'default'` | 服务名 |
| `$dataDir` | `?string` | `null` | 数据目录，null 用默认 |

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');
```

### getName

获取服务名。

```php
public function getName(): string
```

```php
echo $mgr->getName(); // default
```

### getDataDir

获取数据目录。

```php
public function getDataDir(): ?string
```

```php
var_dump($mgr->getDataDir()); // string(15) "/var/lib/xhjob"
```

---

## 一、创建任务

### create

创建单个任务，等价于 `xhjob_dispatch`。

```php
public function create(TaskBuilder $b): string
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$b` | `TaskBuilder` | 任务构建器 |

**返回值**：`string`，`task_id`。

**异常**：daemon 返回 `error: ...` 时抛 `InvalidTaskConfigException`。

```php
use Xhjob\TaskBuilder;

$id = $mgr->create(
    TaskBuilder::shell('echo hi')->cron('0 * * * *')->withRetry(3, 2)
);
```

### createChain

创建任务链，等价于 `xhjob_chain`。

```php
public function createChain(array $builders): string
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$builders` | `array` | `TaskBuilder` 实例数组 |

**返回值**：`string`，`chain_id`。

**异常**：daemon 返回 `error: ...` 时抛 `InvalidTaskConfigException`。

```php
$chainId = $mgr->createChain([
    TaskBuilder::shell('echo "line1\nline2"'),
    TaskBuilder::shell('grep line2'),
]);
```

### createGroup

创建任务组，等价于 `xhjob_group`。

```php
public function createGroup(array $builders): string
```

**返回值**：`string`，`group_id`。

```php
$groupId = $mgr->createGroup([
    TaskBuilder::http('GET', 'https://a.test'),
    TaskBuilder::http('GET', 'https://b.test'),
]);
```

### createChord

创建 chord（header + body 回调），等价于 `xhjob_chord`。并行执行所有 header 任务，全部成功后执行 callback，callback 的 meta 携带所有 header 结果。任一 header 失败时 chord 转 `partial_failed` 终态，不派发 callback。参考 Celery chord。

```php
public function createChord(array $headerBuilders, TaskBuilder $callback): string
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$headerBuilders` | `array` | header `TaskBuilder` 实例数组 |
| `$callback` | `TaskBuilder` | 回调 `TaskBuilder`（body） |

**返回值**：`string`，`chord_id`。

```php
$chordId = $mgr->createChord(
    [TaskBuilder::shell('echo 1'), TaskBuilder::shell('echo 2')],
    TaskBuilder::shell('echo done')
);
```

### update

更新任务（先 remove 再以指定 id 重建）。

```php
public function update(string $id, TaskBuilder $b): string
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 旧任务 ID |
| `$b` | `TaskBuilder` | 新任务构建器 |

**返回值**：`string`，新任务 ID（与传入 `$id` 相同）。

{: .note }
实现细节：先尝试 `remove($id)`（不存在时捕获 `TaskNotFoundException` 忽略），再以旧 id 重建（自动调用 `withId($id)->replaceExisting(true)` 以防 daemon 中残留同 id）。

**异常**：重建时 dispatch 失败抛 `InvalidTaskConfigException`。

```php
// 把已有任务改成新的 cron
$newId = $mgr->update($oldId, TaskBuilder::shell('new-cmd')->cron('0 3 * * *'));
// $newId === $oldId
```

---

## 二、查询任务

### get

获取任务详情（完整 Task JSON 解码后的关联数组）。

```php
public function get(string $id): ?array
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |

**返回值**：`?array`。任务不存在返回 `null`；成功返回完整配置关联数组。

**异常**：daemon 不可达（返回 `error: ...`）时抛 `ServiceNotRunningException`。

```php
$task = $mgr->get($id);
if ($task === null) {
    echo "任务不存在\n";
} else {
    echo "cron={$task['cron']} retry_max={$task['retry_max']}\n";
}
```

### list

列出任务，可按状态 / 标签过滤。

```php
public function list(?string $stateFilter = null, ?string $tag = null): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$stateFilter` | `?string` | `null` | 状态过滤（pending/running/success/...） |
| `$tag` | `?string` | `null` | 标签过滤 |

**返回值**：`array`，任务数组（`{"tasks":[...]}` 解码后的数组）。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$pending = $mgr->list('pending');
$billing = $mgr->list(null, 'billing');
foreach ($pending as $t) {
    echo $t['id'] . "\n";
}
```

### state

查询任务状态（关联数组）。

```php
public function state(string $id): array
```

**返回值**：`array`，包含 `state` / `attempts` / `created_at` / `started_at` / `finished_at` / `last_error` / `execution_count` / `max_executions` / `paused` / `progress` / `worker_pid` 等字段（详见 `xhjob_state` 字段表）。

**异常**：daemon 不可达或任务不存在（原生函数返回带 `error` 键的数组）时抛 `ServiceNotRunningException`。

```php
$s = $mgr->state($id);
printf("state=%s attempts=%s\n", $s['state'], $s['attempts']);
if ($s['state'] === 'failed') {
    echo "错误：{$s['last_error']}\n";
}
```

### result

查询任务执行结果。

```php
public function result(string $id): array
```

**返回值**：`array`。Shell 任务含 `stdout` / `stderr` / `exit_code`；HTTP 任务含 `body`（或二进制时的 `body_b64`）/ `status_code`。

**注意事项**：
- 当结果不存在时（`ignoreResult=true` 或任务未产出输出），返回带 `error` 键的数组——这是预期业务条件，**不抛异常**，调用方通过 `isset($r['error'])` 判断。
- 对 HTTP 二进制响应（非 UTF-8），结果含 `body_b64` 键，调用方可通过 `base64_decode($r['body_b64'])` 恢复原始字节。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$r = $mgr->result($id);
if (isset($r['error'])) {
    echo "无结果：{$r['error']}\n";
} elseif (isset($r['body_b64'])) {
    $bytes = base64_decode($r['body_b64']);
    echo "二进制响应，长度=" . strlen($bytes) . "\n";
} else {
    echo "exit_code={$r['exit_code']}\n{$r['stdout']}";
}
```

### logs

查询任务事件日志（按 `task_id` 过滤的事件）。

```php
public function logs(string $id, int $sinceTs = 0): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$sinceTs` | `int` | `0` | 起始时间戳（Unix 秒），0 = 全部 |

**返回值**：`array`，事件数组（元素为 `{task_id, event_type, payload, ts}`）。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$events = $mgr->logs($id, time() - 3600); // 最近 1 小时
foreach ($events as $e) {
    echo "{$e['ts']} {$e['event_type']}\n";
}
```

---

## 三、控制任务

以下方法均返回 `bool`，失败仅返回 `false`（不抛异常）。

### stop

停止（取消）任务，等价于 `xhjob_cancel`。

```php
public function stop(string $id): bool
```

```php
if ($mgr->stop($id)) {
    echo "已取消\n";
}
```

### restart

重启（重新入队）任务，等价于 `xhjob_requeue`。

```php
public function restart(string $id): bool
```

```php
$mgr->restart($id); // 将终态任务重新入队回 Pending
```

### pause

暂停任务，等价于 `xhjob_pause`。

```php
public function pause(string $id): bool
```

```php
$mgr->pause($id);
```

### resume

恢复任务，等价于 `xhjob_resume`。

```php
public function resume(string $id): bool
```

```php
$mgr->resume($id);
```

### remove

删除任务，等价于 `xhjob_remove`。

```php
public function remove(string $id): bool
```

**返回值**：`bool`，成功返回 `true`。

**异常**：任务不存在或删除失败时抛 `TaskNotFoundException`。

```php
try {
    $mgr->remove($id);
} catch (TaskNotFoundException $e) {
    echo "任务已不存在\n";
}
```

### reschedule

重新调度任务（修改 cron 表达式），等价于 `xhjob_reschedule`。

```php
public function reschedule(string $id, string $cron): bool
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$cron` | `string` | 新的 cron 表达式 |

```php
$mgr->reschedule($id, '0 8 * * *'); // 改成每天 8 点
```

---

## 四、编排状态

### chainState

查询任务链状态。

```php
public function chainState(string $chainId): ?array
```

**返回值**：`?array`。链不存在返回 `null`；成功返回 `ChainRecord` 关联数组（`{chain_id, tasks, current_step, state, created_at, updated_at}`）。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$rec = $mgr->chainState($chainId);
if ($rec !== null) {
    echo "step={$rec['current_step']} state={$rec['state']}\n";
}
```

### groupState

查询任务组状态。

```php
public function groupState(string $groupId): ?array
```

**返回值**：`?array`。组不存在返回 `null`；成功返回 `GroupRecord` 关联数组（`{group_id, tasks, state, created_at, updated_at}`）外加实时 `summary`（`{total, succeeded, failed, pending}`）。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$rec = $mgr->groupState($groupId);
if ($rec !== null) {
    print_r($rec['summary']);
}
```

### chordState

查询 chord 状态。

```php
public function chordState(string $chordId): ?array
```

**返回值**：`?array`。chord 不存在返回 `null`；成功返回 `ChordRecord` 关联数组（`{id, header_task_ids, callback_json, callback_task_id, state, created_at, updated_at}`）。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$rec = $mgr->chordState($chordId);
if ($rec !== null) {
    echo "state={$rec['state']}\n";
}
```

---

## 五、进度上报与事件拉取

### reportProgress

上报任务进度。参考 Celery `update_state(state='PROGRESS', meta=...)`。

```php
public function reportProgress(string $id, int $percent, ?string $meta = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$percent` | `int` | — | 进度百分比 **0-100**，越界返回 `false` |
| `$meta` | `?string` | `null` | 任意 JSON 元数据 |

**返回值**：`bool`。

```php
$mgr->reportProgress($id, 50, json_encode(['step' => 'halfway']));
$mgr->reportProgress($id, 100);                  // 完成
var_dump($mgr->reportProgress($id, 150));        // false（越界）
```

### pullEvents

拉取任务事件，可按 `event_type` 过滤。

```php
public function pullEvents(int $sinceTs = 0, ?string $eventType = null): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$sinceTs` | `int` | `0` | 起始时间戳（Unix 秒），0 = 全部 |
| `$eventType` | `?string` | `null` | 事件类型过滤（started/succeeded/failed/...） |

**返回值**：`array`，事件数组。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
// 只看最近 1 小时的失败事件
$failed = $mgr->pullEvents(time() - 3600, 'failed');
foreach ($failed as $e) {
    echo "{$e['task_id']} failed at {$e['ts']}\n";
}
```

### inspect

聚合检查 daemon 状态。参考 Celery `inspect active / registered / scheduled / stats`。

```php
public function inspect(string $mode = 'stats'): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$mode` | `string` | `'stats'` | 查询模式：active / registered / scheduled / stats |

**`$mode` 取值**：

| 模式 | 含义 |
| --- | --- |
| `active` | 当前运行中的任务 |
| `registered` | cron / interval 注册任务 |
| `scheduled` | 有未来 `next_fire` 的任务 |
| `stats` | 聚合 WorkerStats（默认） |

**返回值**：`array`。

**异常**：daemon 不可达时抛 `ServiceNotRunningException`。

```php
$running = $mgr->inspect('active');
$stats   = $mgr->inspect('stats');          // 默认模式
echo "running tasks: " . count($running) . "\n";
```

---

## 六、轮询等待

### waitForState

轮询等待任务进入期望状态。

```php
public function waitForState(string $id, string $expectedState, int $timeoutSec = 30): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$expectedState` | `string` | — | 期望状态（pending/running/success/...） |
| `$timeoutSec` | `int` | `30` | 超时秒数 |

**返回值**：`bool`。在超时前进入期望状态返回 `true`；超时或进入非期望终态返回 `false`。

**注意事项**：
- 轮询间隔 300ms；`state()` 临时错误（如 daemon 重启中）时退避 500ms 继续轮询。
- **终态**：`success` / `failed` / `cancelled` / `expired` / `interrupted`。一旦进入终态且非期望，立即返回 `false`，不再等待。

```php
if ($mgr->waitForState($id, 'success', 60)) {
    echo "任务成功完成\n";
} else {
    echo "超时或进入非成功终态\n";
}
```

### waitForResult

轮询等待任务结果。仅在任务进入终态后返回结果数组，超时返回 `null`。

```php
public function waitForResult(string $id, int $timeoutSec = 30): ?array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$timeoutSec` | `int` | `30` | 超时秒数 |

**返回值**：`?array`。进入终态（`success` / `failed` / `cancelled` / `expired` / `interrupted`）后返回 `result()` 结果；超时返回 `null`。

```php
$r = $mgr->waitForResult($id, 120);
if ($r === null) {
    echo "等待结果超时\n";
} else {
    echo "exit_code={$r['exit_code']}\n{$r['stdout']}";
}
```

---

## 完整示例

### 创建并等待结果

```php
use Xhjob\TaskManager;
use Xhjob\TaskBuilder;

$mgr = new TaskManager('default', '/var/lib/xhjob');

// 创建一次性 shell 任务
$id = $mgr->create(
    TaskBuilder::shell('echo hello && sleep 2')
        ->timeout(30)
        ->withRetry(1, 2)
);

// 等待成功（最多 30 秒）
if ($mgr->waitForState($id, 'success', 30)) {
    $r = $mgr->result($id);
    echo "stdout: {$r['stdout']}";
}
```

### 进度上报 + 轮询

```php
// 业务侧任务执行中上报进度（任务 ID 通常由派发方注入）
$mgr->reportProgress($id, 25, json_encode(['phase' => 'init']));
$mgr->reportProgress($id, 50, json_encode(['phase' => 'processing']));
$mgr->reportProgress($id, 75, json_encode(['phase' => 'finalizing']));
$mgr->reportProgress($id, 100);

// 另一进程轮询进度
while (!$mgr->waitForState($id, 'success', 5)) {
    $s = $mgr->state($id);
    echo "progress={$s['progress']}%\n";
    if ($s['state'] === 'failed') {
        break;
    }
}
```

### 任务组并行 + 状态汇总

```php
$groupId = $mgr->createGroup([
    TaskBuilder::http('GET', 'https://a.test')->timeout(10),
    TaskBuilder::http('GET', 'https://b.test')->timeout(10),
    TaskBuilder::http('GET', 'https://c.test')->timeout(10),
]);

// 轮询组状态直到完成
while (true) {
    $rec = $mgr->groupState($groupId);
    $sum = $rec['summary'];
    echo "total={$sum['total']} done={$sum['succeeded']} failed={$sum['failed']}\n";
    if ($sum['pending'] == 0) {
        break;
    }
    usleep(500000);
}
echo "group state: {$rec['state']}\n";
```

### Chord 聚合回调

```php
$chordId = $mgr->createChord(
    [
        TaskBuilder::http('GET', 'https://a.test/1'),
        TaskBuilder::http('GET', 'https://a.test/2'),
    ],
    TaskBuilder::shell('process-aggregate.sh')  // callback.meta 携带 header 结果
);

// 等待 chord 完成
$mgr->waitForState($chordId, 'success', 120);
$rec = $mgr->chordState($chordId);
echo "callback_task_id={$rec['callback_task_id']}\n";
```

### 在线修改任务

```php
// 修改 cron + 优先级
$mgr->update($id, TaskBuilder::shell('new-cmd')->cron('0 4 * * *')->priority(10));
// 或仅改 cron
$mgr->reschedule($id, '0 6 * * *');
```
