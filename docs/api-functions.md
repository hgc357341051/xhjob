---
title: PHP 函数参考
parent: API 参考
nav_order: 21
---

# PHP 函数参考

本页覆盖 xhjob 扩展（`src/lib.rs`）导出的全部 **27 个 `xhjob_*` 函数**，以及内置的 **`Xhjob` PHP 类**（Rust 侧链式 builder）。

{: .warning }
所有涉及 daemon 通信的函数都内置 **IPC 超时保护**（默认 5 秒，可通过环境变量 `XHJOB_IPC_TIMEOUT_SECS` 调整）。即使 daemon 接受连接后死锁 / 被 SIGSTOP / 崩溃，PHP-FPM worker 也不会被永久阻塞，从而避免 worker 池被逐个耗尽导致 502/504。

## 通用约定

- **服务名 `$name`**：所有函数的第一个可选参数为服务名，`null` 时回退到默认服务（`default`）。服务名会经过校验，非法名称会让函数返回失败值（`false` / 空数组 / `error:` 前缀字符串 / `null`）。
- **数据目录 `$data_dir`**：用于重定位 daemon 的 PID / sock / db / log 文件，空字符串与 `null` 等价（使用默认目录）。适用于备份 / 迁移 / 恢复场景。
- **`error:` 前缀**：返回字符串的函数（`xhjob_dispatch` / `xhjob_list` / `xhjob_events` / `xhjob_pull_events` / `xhjob_inspect` / `xhjob_chain` / `xhjob_group` / `xhjob_chord`）在失败时返回以 `error:` 开头的字符串，调用方应使用 `str_starts_with($r, 'error:')` 判断。

---

## 一、生命周期类

### xhjob_start

启动指定服务的 daemon 进程。若 daemon 已在运行则直接返回 `true`。

```php
xhjob_start(?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `?string` | `null` | 服务名，`null` 回退默认服务 |
| `$data_dir` | `?string` | `null` | 数据目录，空串 / `null` 用默认 |

**返回值**：`bool`。daemon 已运行或成功启动返回 `true`；启动超时 / 服务名校验失败 / spawn 失败返回 `false`。

**错误契约**：失败仅返回 `false`，不抛异常；详细原因写入 daemon 日志与 `tracing`。

**注意事项**：daemon 通过 `spawn_via_double_fork`（Unix）/ `spawn_via_create_process`（Windows）派生，重新 exec 一个 PHP 进程执行 `xhjob_run_daemon`。

```php
// 启动默认服务
xhjob_start();

// 启动命名服务并指定数据目录
$ok = xhjob_start('cron-svc', '/var/lib/xhjob');
if (!$ok) {
    throw new RuntimeException('daemon 启动失败，请查看日志');
}
```

### xhjob_stop

停止指定服务的 daemon。

```php
xhjob_stop(?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：失败仅返回 `false`，原因记录到日志。

```php
xhjob_stop();                          // 停止默认服务
xhjob_stop('cron-svc', '/var/lib/xhjob'); // 停止命名服务
```

### xhjob_restart

重启指定服务的 daemon（等价于 stop + start）。

```php
xhjob_restart(?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`bool`。重启成功返回 `true`，失败返回 `false`。

**注意事项**：重启会重新加载 daemon。已持久化的 cron / interval 任务在重启后继续触发；运行中的任务根据 `acksLate` 设置决定是否重置为 Pending（崩溃恢复语义）。

```php
if (xhjob_restart()) {
    echo "daemon 已重启\n";
}
```

### xhjob_status

查询 daemon 运行状态。

```php
xhjob_status(?string $name = null, ?string $data_dir = null): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`array`，键值对形式。正常运行返回 `[["running","true"],["pid","123"]]`；未运行返回 `[["running","false"]]`；服务名非法返回 `[["running","false"],["error","<原因>"]]`。

**错误契约**：服务名非法时附带 `error` 键；daemon 未运行不算错误。

```php
$status = xhjob_status();
$pairs = [];
// 扩展返回 [[k, v], ...] 形式，便于转成关联数组
foreach ($status as $pair) {
    $pairs[$pair[0]] = $pair[1];
}
echo $pairs['running'] ? "running, pid={$pairs['pid']}" : "stopped";
```

### xhjob_run_daemon

{: .warning }
**隐藏入口**，由 daemon spawner 在重新 exec 的 PHP 子进程中调用，**永不返回**。普通用户代码不应直接调用。

```php
xhjob_run_daemon(?string $service_name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$service_name` | `?string` | `null` | 服务名，由 spawner 编码进 `-r` 代码串以穿越环境变量被擦除的场景 |
| `$data_dir` | `?string` | `null` | 数据目录，安装到 `service::current_data_dir()` |

**返回值**：`bool`。仅在服务名校验失败时返回 `false`，否则进入 daemon 主循环永不返回。

**错误契约**：服务名非法时返回 `false`；正常情况下函数不返回。

**注意事项**：在 Unix 上会重定向 std 流到日志文件（`dup2` 到 log 文件描述符），stdin 重定向到 `/dev/null`。当参数为 `null` 时回退到环境变量 `XHJOB_SERVICE_NAME` / `XHJOB_DATA_DIR`，最后回退到平台默认值。

```php
// 通常无需手动调用——spawner 会自动生成如下调用：
// php -r 'xhjob_run_daemon("cron-svc", "/var/lib/xhjob");'
```

---

## 二、任务派发与查询类

### xhjob_dispatch

将一个任务 JSON 派发到 daemon。

```php
xhjob_dispatch(string $task_json, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$task_json` | `string` | — | TaskBuilder JSON 字符串 |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 `task_id`；失败返回 `error: <原因>`。

**错误契约**（P0-1 fix）：失败返回值固定以 `error:` 前缀开头，调用方据此区分真实 `task_id` 与错误信息——若不加前缀，错误字符串会被误认为 task_id 造成「假成功」。

```php
$taskJson = json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo hello'],
    'cron'      => '0 * * * *',
], JSON_UNESCAPED_SLASHES);

$result = xhjob_dispatch($taskJson);
if (str_starts_with($result, 'error:')) {
    throw new RuntimeException('派发失败：' . substr($result, 6));
}
$taskId = $result;
echo "task_id = $taskId\n";
```

### xhjob_state

查询任务运行状态（精简 `StateInfo` 视图）。

```php
xhjob_state(string $id, ?string $name = null, ?string $data_dir = null): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`array`（`[[k, v], ...]` 键值对形式）。包含以下键（部分仅在对应字段非空时出现）：

| 键 | 说明 |
| --- | --- |
| `state` | 任务状态（pending/running/success/failed/cancelled/expired/interrupted） |
| `attempts` | 已尝试次数 |
| `created_at` | 创建时间戳 |
| `started_at` | 开始执行时间戳（可选） |
| `finished_at` | 完成时间戳（可选） |
| `last_error` | 最近一次错误信息（可选） |
| `execution_count` | cron 已执行次数 |
| `max_executions` | 最大执行次数（0=无限） |
| `paused` | 是否暂停 |
| `start_date` / `end_date` | 起止时间戳，`null` 表示未设置 |
| `meta` | 用户元数据，`null` 表示未设置 |
| `interval` / `run_at` | IntervalTrigger / DateTrigger 值，`null` 表示未设置 |
| `jitter` / `expires` | 抖动 / 过期秒数 |
| `retry_backoff` / `ignore_result` / `acks_late` | 布尔标志 |
| `soft_timeout` | 软超时秒数，`null` 表示未设置 |
| `misfire_grace_time` | 误触发宽限时间 |
| `tags` | 标签 JSON 数组字符串 |
| `rate_limit_count` / `rate_limit_window` | 速率限制 |
| `acks_on_failure` | 失败时是否确认 |
| `timezone` | 时区标识符 |
| `coalesce` | 是否合并误触发 |
| `progress` / `progress_meta` | 进度百分比 / 进度元数据 |
| `worker_pid` / `worker_starttime` | 执行 worker 的 PID / 启动时间（execution lease，可选） |

任务不存在或 daemon 不可达时返回 `[["state","UNKNOWN"],["error","task not found or daemon not running"]]`。

```php
$raw = xhjob_state($taskId);
$state = [];
foreach ($raw as $pair) {
    $state[$pair[0]] = $pair[1];
}
printf("state=%s attempts=%s\n", $state['state'], $state['attempts']);
```

### xhjob_result

查询任务执行结果。

```php
xhjob_result(string $id, ?string $name = null, ?string $data_dir = null): array
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`array`（键值对形式）。Shell 任务包含 `stdout` / `stderr` / `exit_code`；HTTP 任务包含 `body`（或二进制时的 `body_b64`）/ `status_code`。无结果记录时返回 `[["error","no result record for this task ..."]]`，调用方应配合 `xhjob_state()` 的 `last_error` 排查。

```php
$raw = xhjob_result($taskId);
$res = [];
foreach ($raw as $pair) {
    $res[$pair[0]] = $pair[1];
}
if (isset($res['error'])) {
    echo "无结果：{$res['error']}\n";
} else {
    echo "exit_code={$res['exit_code']}\n{$res['stdout']}";
}
```

### xhjob_get

按 ID 获取完整任务定义（与 `xhjob_state` 的精简视图不同，本函数返回全部持久化字段，包括 `retry_max` / `timeout` / `priority` / `cron` 等配置字段）。

```php
xhjob_get(string $id, ?string $name = null, ?string $data_dir = null): ?string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`?string`。成功返回完整 Task JSON 字符串；任务不存在或 daemon 不可达返回 `null`。

```php
$json = xhjob_get($taskId);
if ($json === null) {
    echo "任务不存在或 daemon 不可达\n";
} else {
    $task = json_decode($json, true);
    echo "cron={$task['cron']} retry_max={$task['retry_max']}\n";
}
```

### xhjob_list

列出服务下所有任务，可按状态 / 标签过滤。

```php
xhjob_list(?string $name = null, ?string $state_filter = null, ?string $tag = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$name` | `?string` | `null` | 服务名 |
| `$state_filter` | `?string` | `null` | 状态过滤（pending/running/success/...） |
| `$tag` | `?string` | `null` | 标签过滤，透传到 daemon 端做服务端过滤 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 `{"tasks":[...]}` 形式的 JSON 字符串；失败返回 `error: <原因>`。

```php
$json = xhjob_list(null, 'pending', 'billing');
if (str_starts_with($json, 'error:')) {
    throw new RuntimeException(substr($json, 6));
}
$tasks = json_decode($json, true)['tasks'];
foreach ($tasks as $t) {
    echo $t['id'] . "\n";
}
```

---

## 三、任务控制类

以下函数签名一致（`$id` 必填，`$name` / `$data_dir` 可选），均返回 `bool`，失败仅返回 `false`：

### xhjob_remove

从 store 中删除任务定义（不影响正在运行的实例）。参考 APScheduler `remove_job`。

```php
xhjob_remove(string $id, ?string $name = null, ?string $data_dir = null): bool
```

```php
if (!xhjob_remove($taskId)) {
    echo "删除失败（任务不存在或 daemon 不可达）\n";
}
```

### xhjob_pause

暂停 cron 任务，定义保留但 cron tick 不再触发。参考 APScheduler `pause_job`。

```php
xhjob_pause(string $id, ?string $name = null, ?string $data_dir = null): bool
```

```php
xhjob_pause($taskId); // 暂停后再 resume 才会恢复触发
```

### xhjob_resume

恢复已暂停的 cron 任务。参考 APScheduler `resume_job`。

```php
xhjob_resume(string $id, ?string $name = null, ?string $data_dir = null): bool
```

```php
xhjob_resume($taskId);
```

### xhjob_cancel

取消任务：Pending → Cancelled 终态；Running → 不重试、不再被 cron 触发。参考 Celery `revoke`。

```php
xhjob_cancel(string $id, ?string $name = null, ?string $data_dir = null): bool
```

```php
xhjob_cancel($taskId);
```

### xhjob_requeue

将终态任务（Cancelled / Failed / Expired）重新入队回 Pending 以便再次触发。重置 `attempts=0`，`next_fire=now`。参考 Celery `requeue`。

```php
xhjob_requeue(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**返回值**：`bool`。任务不在可重入队终态或不存在时返回 `false`。

```php
if (xhjob_requeue($taskId)) {
    echo "已重新入队\n";
}
```

### xhjob_reschedule

在线修改 cron 任务的 cron 表达式。保留任务状态、`execution_count`、`attempts`、`meta`，仅修改 `cron` 与 `next_fire`。参考 APScheduler `reschedule_job`。

```php
xhjob_reschedule(string $id, string $cron, ?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$cron` | `string` | — | 新的 5 字段 cron 表达式 |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`bool`。任务不存在 / 非 cron 任务（interval / runAt）/ 终态 / cron 非法时返回 `false`。

```php
// 把任务改成每天 8 点执行
xhjob_reschedule($taskId, '0 8 * * *');
```

### xhjob_modify

运行时局部更新任意任务字段。patch JSON 是一个对象，键映射到 Task 字段（cron / interval / priority / tags 等）。

```php
xhjob_modify(string $id, string $patch_json, ?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$patch_json` | `string` | — | patch 对象 JSON，如 `{"priority":10,"tags":["a"]}` |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`bool`。patch JSON 非法 / 任务不存在 / daemon 拒绝时返回 `false`。

```php
$patch = json_encode(['priority' => 10, 'tags' => ['urgent', 'billing']]);
xhjob_modify($taskId, $patch);
```

---

## 四、编排类

### xhjob_chain

创建任务链：顺序流水线，每个任务的 stdout 作为下一个任务的 stdin；任一步骤失败则整链转 `failed` 并跳过剩余步骤。参考 Celery `chain(t1, t2, t3)`。

```php
xhjob_chain(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$tasks_json` | `string` | — | TaskBuilder 配置对象数组 JSON，如 `[{"type":"shell","cmd":"echo a"}]` |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 `chain_id`；失败返回 `error: <原因>`（含 JSON 解析错误）。

```php
$tasks = json_encode([
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo "line1\nline2"']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'grep line2']],
]);
$result = xhjob_chain($tasks);
if (!str_starts_with($result, 'error:')) {
    echo "chain_id = $result\n";
}
```

### xhjob_chain_state

查询链状态，返回完整 `ChainRecord` JSON（`{chain_id, tasks, current_step, state, created_at, updated_at}`）。

```php
xhjob_chain_state(string $chain_id, ?string $name = null, ?string $data_dir = null): ?string
```

**返回值**：`?string`。链不存在或 daemon 不可达返回 `null`。

```php
$json = xhjob_chain_state($chainId);
if ($json !== null) {
    $rec = json_decode($json, true);
    echo "step={$rec['current_step']} state={$rec['state']}\n";
}
```

### xhjob_group

创建任务组：所有任务并行派发。最终组状态为 `success`（全部成功）/ `partial_failed`（部分失败）/ `failed`（全部失败）。参考 Celery `group(t1, t2, t3)`。

```php
xhjob_group(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
```

**返回值**：`string`。成功返回 `group_id`；失败返回 `error: <原因>`。

```php
$tasks = json_encode([
    ['task_type' => 'http', 'payload' => ['method' => 'GET', 'url' => 'https://a.test']],
    ['task_type' => 'http', 'payload' => ['method' => 'GET', 'url' => 'https://b.test']],
]);
$groupId = xhjob_group($tasks);
```

### xhjob_group_state

查询组状态，返回完整 `GroupRecord` JSON（`{group_id, tasks, state, created_at, updated_at}`）外加实时 `summary`（`{total, succeeded, failed, pending}`）。

```php
xhjob_group_state(string $group_id, ?string $name = null, ?string $data_dir = null): ?string
```

**返回值**：`?string`。组不存在或 daemon 不可达返回 `null`。

```php
$json = xhjob_group_state($groupId);
$rec = json_decode($json, true);
print_r($rec['summary']);
```

### xhjob_chord

创建 chord：header（并行任务）+ body（回调）。所有 header 任务并行执行；全部成功后派发 body，其 `meta` 设为携带每个 header 结果的 JSON 数组。任一 header 失败则 chord 转 `partial_failed` 且不派发 body。参考 Celery `chord(header, body)`。

```php
xhjob_chord(string $header_json, string $callback_json, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$header_json` | `string` | — | header 任务配置对象数组 JSON |
| `$callback_json` | `string` | — | 单个回调任务配置对象 JSON |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 `chord_id`；失败返回 `error: <原因>`。

```php
$header = json_encode([
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo 1']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo 2']],
]);
$callback = json_encode(['task_type' => 'shell', 'payload' => ['cmd' => 'echo done']]);
$chordId = xhjob_chord($header, $callback);
```

### xhjob_chord_state

查询 chord 状态，返回完整 `ChordRecord` JSON（`{id, header_task_ids, callback_json, callback_task_id, state, created_at, updated_at}`）。

```php
xhjob_chord_state(string $chord_id, ?string $name = null, ?string $data_dir = null): ?string
```

**返回值**：`?string`。chord 不存在或 daemon 不可达返回 `null`。

```php
$json = xhjob_chord_state($chordId);
$rec = json_decode($json, true);
echo "state={$rec['state']}\n";
```

---

## 五、事件与进度类

### xhjob_events

拉取自 `since_ts`（Unix 秒）以来的任务事件，可按 `task_id` 过滤。参考 APScheduler `EVENT_JOB_*`。

```php
xhjob_events(int $since_ts, ?string $task_id = null, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$since_ts` | `int` | — | 起始时间戳（Unix 秒） |
| `$task_id` | `?string` | `null` | 仅返回该任务的事件 |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 JSON 数组（元素为 `{task_id, event_type, payload, ts}`）；失败返回 `error: <原因>`。

```php
$since = time() - 3600; // 最近 1 小时
$json = xhjob_events($since, $taskId);
$events = json_decode($json, true);
foreach ($events as $e) {
    echo "{$e['ts']} {$e['event_type']}\n";
}
```

### xhjob_pull_events

拉取自 `since_ts` 以来的任务事件，可按 `event_type` 过滤（如 `started` / `succeeded` / `failed`）。

```php
xhjob_pull_events(int $since_ts, ?string $event_type = null, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$since_ts` | `int` | — | 起始时间戳（Unix 秒） |
| `$event_type` | `?string` | `null` | 事件类型过滤 |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`string`。成功返回 JSON 数组；失败返回 `error: <原因>`。

```php
// 只看最近 1 小时的失败事件
$json = xhjob_pull_events(time() - 3600, 'failed');
$failed = json_decode($json, true);
```

### xhjob_report_progress

上报任务进度（百分比 + 可选元数据）。参考 Celery `update_state(state='PROGRESS', meta=...)`。

```php
xhjob_report_progress(string $id, int $percent, ?string $meta_json = null, ?string $name = null, ?string $data_dir = null): bool
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$id` | `string` | — | 任务 ID |
| `$percent` | `int` | — | 进度百分比，**必须 0-100**，越界直接返回 `false` 且不联系 daemon |
| `$meta_json` | `?string` | `null` | 任意 JSON 元数据 |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**返回值**：`bool`。越界 / 服务名非法 / daemon 拒绝返回 `false`。

```php
$meta = json_encode(['step' => 'compiling', 'done' => 42]);
xhjob_report_progress($taskId, 75, $meta);
// 越界会被拒绝
var_dump(xhjob_report_progress($taskId, 150)); // false
```

### xhjob_inspect

聚合检查 daemon 状态。参考 Celery `inspect active / registered / scheduled / stats`。

```php
xhjob_inspect(string $mode, ?string $name = null, ?string $data_dir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$mode` | `string` | — | 查询模式（见下） |
| `$name` | `?string` | `null` | 服务名 |
| `$data_dir` | `?string` | `null` | 数据目录 |

**`$mode` 取值**：

| 模式 | 含义 | 返回 |
| --- | --- | --- |
| `active` | 当前运行中的任务 | JSON 数组 |
| `registered` | cron / interval 注册任务 | JSON 数组 |
| `scheduled` | 有未来 `next_fire` 的任务 | JSON 数组 |
| `stats` | 聚合 WorkerStats | JSON 对象 |

**返回值**：`string`。成功返回 JSON（数组 / 对象取决于模式）；失败返回 `error: <原因>`。

```php
$running = json_decode(xhjob_inspect('active'), true);
$stats   = json_decode(xhjob_inspect('stats'), true);
echo "running tasks: " . count($running) . "\n";
```

---

## 六、Xhjob PHP 类

`Xhjob` 是扩展内置的链式 builder 类，直接绑定到 Rust 侧的 `TaskBuilder`。所有配置方法返回 `&mut self`（PHP 中即 `$this`），支持链式调用，最后用 `dispatch()` 派发。

{: .note }
ext-php-rs 会把 Rust 的 `snake_case` 方法名自动转换为 PHP 的 `camelCase`（如 `data_dir` → `dataDir`，`with_headers` → `withHeaders`）。

### 创建实例

```php
Xhjob::task(): Xhjob
```

工厂方法，返回一个空的 `Xhjob` 实例。

```php
$xj = Xhjob::task();
```

### 任务类型与目标

#### viaShell

设置任务为 shell 类型。

```php
viaShell(string $cmd): $this
```

```php
Xhjob::task()->viaShell('echo hi');
```

#### viaHttp

设置任务为 HTTP 类型。

```php
viaHttp(string $method, string $url): $this
```

```php
Xhjob::task()->viaHttp('POST', 'https://api.test/users');
```

### 服务与目录

#### service

绑定到命名服务，后续 `dispatch()` 路由到该服务 daemon。

```php
service(string $name): $this
```

```php
Xhjob::task()->service('cron-svc');
```

#### dataDir

设置数据目录，`dispatch()` 据此解析 IPC socket 路径。

```php
dataDir(string $dir): $this
```

```php
Xhjob::task()->dataDir('/var/lib/xhjob');
```

### HTTP 专属

#### withHeaders

设置 HTTP 请求头（覆盖之前设置）。PHP 关联数组转换为 `Vec<(String, String)>`。

```php
withHeaders(array $headers): $this
```

```php
Xhjob::task()->viaHttp('GET', 'https://a.test')
    ->withHeaders(['Authorization' => 'Bearer xxx', 'Accept' => 'application/json']);
```

#### withBody

设置 HTTP 请求体。

```php
withBody(string $body): $this
```

```php
Xhjob::task()->viaHttp('POST', 'https://a.test')
    ->withBody(json_encode(['k' => 'v']));
```

#### withProxy

设置 HTTP / SOCKS5 代理。支持 `http://` / `https://` / `socks5://` / `socks5h://`，URL 可含 `user:pass@` 凭证。

```php
withProxy(string $proxy): $this
```

```php
Xhjob::task()->viaHttp('GET', 'https://a.test')
    ->withProxy('socks5h://user:pass@127.0.0.1:1080');
```

### Shell 专属

#### withEncoding

设置 shell 任务输出编码（大小写不敏感，转发给 `encoding_rs::Encoding::for_label`）。`auto` 在 Windows 触发 OEM 代码页检测（Unix 无副作用）。

```php
withEncoding(string $from): $this
```

```php
Xhjob::task()->viaShell('chcp 936 && echo 你好')
    ->withEncoding('GBK');
```

#### withTimezone

设置 IANA 时区（如 `Asia/Shanghai`），cron 在此时区下计算 `next_fire`。非法时区会导致 `dispatch()` 失败。

```php
withTimezone(string $tz): $this
```

```php
Xhjob::task()->viaShell('echo 9am')->cron('0 9 * * *')->withTimezone('America/New_York');
```

### 重试与超时

#### withRetry

设置重试策略。

```php
withRetry(int $max, int $delay): $this
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$max` | `int` | 最大重试次数 |
| `$delay` | `int` | 重试间隔秒数 |

```php
Xhjob::task()->viaShell('flaky-cmd')->withRetry(3, 5);
```

#### retryBackoff

启用指数退避重试：`min(retry_delay * 2^(attempts-1), retry_delay * 60)`。参考 Celery `retry_backoff`。

```php
retryBackoff(bool $on): $this
```

```php
Xhjob::task()->viaShell('flaky-cmd')->withRetry(5, 2)->retryBackoff(true);
```

#### timeout

设置单次执行超时（秒）。

```php
timeout(int $secs): $this
```

```php
Xhjob::task()->viaShell('long-job')->timeout(120);
```

#### softTimeout

设置软超时（优雅退出秒数）。当其严格小于 `timeout` 时，shell 执行器在 `soft_timeout` 秒后发 SIGTERM，若 `(timeout - soft_timeout)` 秒内未退出再发 SIGKILL。传 0 清除软超时。HTTP 任务忽略此字段。参考 Celery `soft_time_limit`。

```php
softTimeout(int $secs): $this
```

```php
Xhjob::task()->viaShell('job')->timeout(60)->softTimeout(50);
```

### 触发器

#### cron

设置 5 字段 cron 表达式。

```php
cron(string $expr): $this
```

```php
Xhjob::task()->viaShell('backup')->cron('0 2 * * *'); // 每天 2 点
```

#### orCron

设置额外的 cron 表达式列表，cron + orCron 任一匹配即触发（F-1）。

```php
orCron(array $exprs): $this
```

```php
Xhjob::task()->viaShell('report')
    ->cron('0 8 * * 1-5')            // 工作日早 8 点
    ->orCron(['0 18 * * 5', '0 9 1 * *']); // 或周五晚 6 点 / 月初早 9 点
```

#### skipDates

设置应跳过的日历日期（任务时区下的日期，F-2），参数为 Unix 时间戳数组。

```php
skipDates(array $dates): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 9 * * *')
    ->skipDates([strtotime('2026-01-01'), strtotime('2026-10-01')]);
```

#### workdaysOnly

启用后任务仅在周一至周五触发（F-3）。

```php
workdaysOnly(): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 9 * * *')->workdaysOnly();
```

#### every

设置 IntervalTrigger 周期（秒）。与 `cron` / `runAt` 互斥，若都设置则 cron / runAt 优先。参考 APScheduler `IntervalTrigger`。

```php
every(int $secs): $this
```

```php
Xhjob::task()->viaShell('poll')->every(60); // 每 60 秒
```

#### runAt

设置 DateTrigger 绝对时间戳，到点触发一次后立即转 Success 终态。优先级最高（覆盖 cron + interval）。参考 APScheduler `DateTrigger`。

```php
runAt(int $ts): $this
```

```php
Xhjob::task()->viaShell('one-shot')->runAt(time() + 300); // 5 分钟后
```

#### jitter

设置抖动（秒），随机偏移叠加到 cron / interval 的 `next_fire` 以避免惊群。runAt 任务忽略（精确一次性时间戳）。参考 APScheduler `jitter`。

```php
jitter(int $secs): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->jitter(30);
```

#### expires

设置任务级过期（秒）：Pending 超过该时长（自 `created_at` 起）转 `Expired` 终态。仅影响 Pending，不中断 Running。参考 APScheduler `expires`。

```php
expires(int $secs): $this
```

```php
Xhjob::task()->viaShell('job')->runAt(time() + 600)->expires(300);
```

### 并发控制

#### priority

设置优先级（数值越大越优先）。

```php
priority(int $p): $this
```

```php
Xhjob::task()->viaShell('job')->priority(10);
```

#### allowOverlap

是否允许同一任务重叠执行。

```php
allowOverlap(bool $allow): $this
```

```php
Xhjob::task()->viaShell('job')->cron('* * * * *')->allowOverlap(true);
```

#### maxInstances

设置最大并发实例数。

```php
maxInstances(int $n): $this
```

```php
Xhjob::task()->viaShell('job')->cron('* * * * *')->maxInstances(3);
```

#### coalesce

是否合并误触发：`true` 把错过的触发合并为一次（仍执行一次），`false` 直接跳过。

```php
coalesce(bool $c): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->coalesce(true);
```

#### persist

是否持久化任务。

```php
persist(bool $p): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->persist(true);
```

#### maxExecutions

设置 cron 任务最大执行次数（0 = 无限）。

```php
maxExecutions(int $n): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->maxExecutions(100);
```

#### misfireGraceTime

设置单任务误触发宽限时间（秒）。0 = 用全局默认（60s）。仅 cron 任务有效。参考 APScheduler `misfire_grace_time`。

```php
misfireGraceTime(int $secs): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->misfireGraceTime(120);
```

### 起止时间

#### startAt

设置起始时间戳，此前 cron 触发被跳过。

```php
startAt(int $ts): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->startAt(strtotime('2026-08-01'));
```

#### endAt

设置结束时间戳，此后任务转 Success 终态。

```php
endAt(int $ts): $this
```

```php
Xhjob::task()->viaShell('job')->cron('0 * * * *')->endAt(strtotime('2026-12-31'));
```

### 可靠性

#### ignoreResult

启用 fire-and-forget：daemon 跳过 `save_result`，`xhjob_result()` 返回 null。状态机仍正常运转。若同时设 `ignoreResult(true)` 和 `resultTtl(>0)`，记日志且 `ignoreResult` 优先。参考 Celery `ignore_result`。

```php
ignoreResult(bool $on): $this
```

```php
Xhjob::task()->viaShell('high-throughput')->cron('* * * * *')->ignoreResult(true);
```

#### acksLate

启用延迟确认：daemon 重启时 Running 的 `acksLate=true` 任务自动重置为 Pending（崩溃恢复语义）。参考 Celery `acks_late`。

```php
acksLate(bool $on): $this
```

```php
Xhjob::task()->viaShell('job')->acksLate(true);
```

#### acksOnFailure

失败时是否确认。`true`（默认）失败遵循 `retry_max`；`false` 失败无限重试直到成功或被取消 / 删除。参考 Celery `acks_on_failure`。

```php
acksOnFailure(bool $on): $this
```

```php
Xhjob::task()->viaShell('must-succeed')->withRetry(0, 1)->acksOnFailure(false);
```

#### resultTtl

设置结果 TTL（秒），0 = 永久保留。

```php
resultTtl(int $secs): $this
```

```php
Xhjob::task()->viaShell('job')->resultTtl(3600);
```

### 元数据与身份

#### id

设置显式任务 ID。**必须匹配 `^[A-Za-z0-9_-]{1,64}$`**（P0-2 fix：防止以 `error:` 开头的 id 破坏 dispatch 错误检测契约，防止含特殊字符的 id 引发下游解析 / SQLite 问题）。空串视为 None（自动生成）。非法 id 不报错，回退为自动生成并记 warn 日志。

```php
id(string $id): $this
```

```php
Xhjob::task()->viaShell('job')->id('nightly-backup-2026');
```

#### replaceExisting

启用后，配合 `id` 使用，dispatch 会整体替换同 id 的已存在任务。参考 APScheduler `replace_existing`。

```php
replaceExisting(bool $on): $this
```

```php
Xhjob::task()->viaShell('job')
    ->id('nightly-backup')
    ->replaceExisting(true)
    ->cron('0 2 * * *');
```

#### tag

追加单个标签，重复标签自动去重，空标签忽略。参考 APScheduler `tags`。

```php
tag(string $tag): $this
```

```php
Xhjob::task()->viaShell('job')->tag('billing')->tag('urgent');
```

#### rateLimit

设置速率限制：`window` 秒内最多 `count` 次触发，count=0 表示不限。参考 Celery `rate_limit`。

```php
rateLimit(int $count, int $window): $this
```

```php
Xhjob::task()->viaShell('job')->rateLimit(10, 60); // 每分钟最多 10 次
```

#### idempotent

声明 HTTP 任务为幂等（即使 POST/PUT/DELETE/PATCH 也允许重试）。默认 false 时非幂等方法在 5xx 不重试以防重复副作用；GET/HEAD/OPTIONS 始终可重试。参考 RFC 7231 §4.2.1-2。

```php
idempotent(bool $on): $this
```

```php
Xhjob::task()->viaHttp('POST', 'https://a.test/idempotent-endpoint')
    ->withRetry(3, 2)->idempotent(true);
```

#### withMeta

附加用户元数据（JSON 字符串）。

```php
withMeta(string $json): $this
```

```php
Xhjob::task()->viaShell('job')->withMeta(json_encode(['owner' => 'team-a', 'ticket' => 'JIRA-123']));
```

### 终端

#### dispatch

派发任务到 daemon。

```php
dispatch(): string
```

**返回值**：`string`。成功返回 `task_id`；失败返回 `error: <原因>`。

```php
$taskId = Xhjob::task()
    ->viaShell('echo hello')
    ->cron('0 * * * *')
    ->withRetry(3, 2)
    ->service('cron-svc')
    ->dataDir('/var/lib/xhjob')
    ->dispatch();
if (str_starts_with($taskId, 'error:')) {
    throw new RuntimeException('派发失败：' . substr($taskId, 6));
}
echo "task_id = $taskId\n";
```

---

## 完整链式示例

```php
// 一个完整的 cron shell 任务：每天 2 点备份，重试 3 次，记录进度元数据
$taskId = Xhjob::task()
    ->viaShell('/usr/local/bin/backup.sh')
    ->withEncoding('UTF-8')
    ->cron('0 2 * * *')
    ->withTimezone('Asia/Shanghai')
    ->withRetry(3, 10)
    ->retryBackoff(true)
    ->timeout(1800)
    ->softTimeout(1700)
    ->maxExecutions(0)
    ->persist(true)
    ->acksLate(true)
    ->resultTtl(86400)
    ->id('daily-backup')
    ->replaceExisting(true)
    ->tag('backup')
    ->tag('critical')
    ->withMeta(json_encode(['owner' => 'ops']))
    ->startAt(strtotime('today'))
    ->misfireGraceTime(300)
    ->dispatch();
```
