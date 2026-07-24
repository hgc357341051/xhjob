# PHP 函数参考

Xhjob 扩展由 Rust + [ext-php-rs](https://github.com/davidcole1340/ext-php-rs) 实现，向 PHP 暴露 **27 个全局函数**（`xhjob_*`，snake_case）与 **`Xhjob` 链式构建类**（camelCase）。本文档严格对齐 `src/lib.rs` 中的真实实现。

所有函数的前两个公共参数是 **服务名 `$name`**（`null` = 默认 `default`）与 **数据目录 `$data_dir`**（`null` = 平台默认），用于支持多服务与目录迁移场景。下文不再逐条重复说明，仅在注意事项中点出差异。

---

## 错误契约总览

Xhjob 的全局函数按返回类型采用 **四种** 统一的错误约定，调用方据此判断成功 / 失败：

| 返回类型 | 失败表示 | 检测方式 |
| --- | --- | --- |
| `string`（dispatch / list / events / pull_events / inspect / chain / group / chord） | 以 `error:` 前缀返回 | `str_starts_with($r, 'error:')` |
| `?string`（get / chain_state / group_state / chord_state） | 返回 `null` | `$r === null` |
| `array`（state / result / status） | 把 `error` 作为一个键值对放入数组 | `isset($r['error'])` |
| `bool`（生命周期 / 控制类） | 返回 `false` | `$r === false` |

> ⚠️ 永远不要把 `error: xxx` 字符串当作 task_id 使用。`str_starts_with($r, 'error:')` 是 string 类返回值的唯一正确判错方式（兼容 PHP 7.x 时用 `strncmp($r, 'error:', 6) === 0`）。

---

## 生命周期（5 个）

### `xhjob_start`

```php
xhjob_start(?string $name = null, ?string $data_dir = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$name` | `?string` | 服务名，`null` 走 `default`；须匹配 `^[A-Za-z0-9_-]+$` |
| `$data_dir` | `?string` | 数据目录，`null` 走平台默认 |

**返回值**：`bool`。daemon 已在运行返回 `true`；启动失败或服务名非法返回 `false`。

**错误契约**：返回 `false` 即失败。服务名校验失败、PID 文件写入失败、re-exec 子进程拉起失败均归为 `false`（详细原因写入 daemon 日志与 `tracing`）。

**注意事项**
- 该函数以 spawn + re-exec 方式拉起独立守护进程，调用方（通常是 FPM / CLI 短生命周期进程）会立即返回，不会阻塞。
- 若 daemon 已在运行（PID 存活且非僵尸），直接返回 `true`，不会重复启动。
- 服务名非法（含空格 / 路径分隔符等）会在 `tracing::error!` 记录后返回 `false`。

**代码演示**

```php
<?php
if (!xhjob_start('cron-svc', '/var/lib/xhjob')) {
    error_log('xhjob daemon 启动失败，请检查日志');
    return;
}
// 启动成功，可立即派发任务
$id = xhjob_dispatch(json_encode([
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo hello'],
]), 'cron-svc', '/var/lib/xhjob');
```

**生产建议**：在 FPM 入口（如 ThinkPHP 中间件）首次派发前惰性调用一次 `xhjob_start()`；不要在每次请求都强制重启。结合 `xhjob_status()` 做健康检查更稳。

---

### `xhjob_stop`

```php
xhjob_stop(?string $name = null, ?string $data_dir = null): bool
```

**参数**：同 `xhjob_start`。

**返回值**：`bool`。成功发送停止信号返回 `true`；服务名非法或 daemon 未运行返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 通过读取 PID 文件并向 daemon 进程发送终止信号实现；daemon 收到后会排空当前任务后退出。
- 若 daemon 已不在运行，返回 `false`（无残留可停）。

**代码演示**

```php
<?php
xhjob_stop('cron-svc', '/var/lib/xhjob');
// 等待退出
while (xhjob_status('cron-svc', '/var/lib/xhjob')['running'] === 'true') {
    usleep(200_000);
}
```

**生产建议**：停止操作配合状态轮询确认退出，避免在滚动发布时立即 `xhjob_start` 导致 PID 文件竞争。

---

### `xhjob_restart`

```php
xhjob_restart(?string $name = null, ?string $data_dir = null): bool
```

**参数**：同 `xhjob_start`。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 等价于 `stop` + `start` 的原子组合，daemon 重启后会从 SQLite 恢复任务定义；标记了 `acksLate=true` 的 Running 任务会被重置为 Pending 重新派发（崩溃恢复语义）。

**代码演示**

```php
<?php
// 发布新版本二进制后热重启
if (xhjob_restart('cron-svc')) {
    echo "重启完成\n";
}
```

**生产建议**：发版热重启优先用 `restart` 而非 `stop`+`start`，以减少任务调度空窗。

---

### `xhjob_status`

```php
xhjob_status(?string $name = null, ?string $data_dir = null): array
```

**参数**：同 `xhjob_start`。

**返回值**：键值对数组，恒含 `running`（`"true"`/`"false"` 字符串）；daemon 在运行时额外含 `pid`；服务名非法时含 `error`。

**错误契约**：服务名非法时返回 `["running" => "false", "error" => "<原因>"]`，用 `isset($r['error'])` 检测。

**注意事项**
- 返回值是字符串键值对数组（`Vec<(String, String)>` 在 PHP 侧表现为关联数组），`running` / `pid` 均为字符串。
- 不会抛异常，适合做探活。

**代码演示**

```php
<?php
$s = xhjob_status('cron-svc', '/var/lib/xhjob');
if (isset($s['error'])) {
    throw new RuntimeException("状态查询失败：{$s['error']}");
}
if ($s['running'] === 'true') {
    printf("daemon 运行中，pid=%s\n", $s['pid'] ?? '-');
}
```

**生产建议**：接入 K8s liveness/readiness 探针时，`running === 'true'` 即视为健康。

---

### `xhjob_run_daemon`

```php
xhjob_run_daemon(?string $service_name = null, ?string $data_dir = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$service_name` | `?string` | 服务名，会经 `service::validate` 校验后通过 `service::set_current()` 安装 |
| `$data_dir` | `?string` | 数据目录，经 `set_current_data_dir()` 安装 |

**返回值**：`bool`。**正常情况下永不返回**（进入守护循环）；仅当服务名非法时返回 `false`。

**错误契约**：服务名非法返回 `false`（写入 `tracing::error!`）。

**注意事项**
- 这是 **隐藏入口**，仅供 PHP 二次 re-exec 时在当前进程内运行守护循环使用——`xhjob_start` 会 spawn 一个 PHP 子进程并通过 `-r` 代码片段调用本函数。**业务代码不应直接调用**。
- 调用后会重定向 std 流到日志文件（Unix 下 `reopen_std_streams_for_daemon`），随后进入 `daemon_main()` 永不返回。

**代码演示**

```php
<?php
// 仅作示意：xhjob_start 内部生成的 re-exec 代码等价于：
// xhjob_run_daemon('cron-svc', '/var/lib/xhjob');
// 业务层请勿直接调用
```

**生产建议**：禁止在 Web 请求中直接调用本函数；如需自定义 spawn 路径，复用 `xhjob_start` 即可。

---

## 派发与查询（5 个）

### `xhjob_dispatch`

```php
xhjob_dispatch(string $task_json, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$task_json` | `string` | TaskBuilder JSON 字符串（任务定义） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 task_id；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。失败原因包括：JSON 解析失败、服务名非法、IPC 不可达、daemon 返回 `ok=false`。

**注意事项**
- `$task_json` 必须是合法 JSON 对象，结构与 Rust `TaskBuilder` 一致（`task_type` / `payload` / `cron` / `interval` / `run_at` 等）。
- 成功返回的 task_id 形如 UUID；若任务指定了 `id` 且 `replace_existing=true`，则会覆盖同 id 任务。

**代码演示**

```php
<?php
$task = [
    'task_type' => 'shell',
    'payload'   => ['cmd' => 'echo hello'],
    'cron'      => '0 * * * *',
    'retry_max' => 3,
    'retry_delay' => 2,
];
$r = xhjob_dispatch(json_encode($task), 'default');
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('派发失败：' . substr($r, 6));
}
$taskId = $r;
```

**生产建议**：不要手拼 JSON，优先用 `TaskBuilder`（见 [TaskBuilder API](api-taskbuilder.md)）或 `Xhjob` 类构建；手拼易触发 serde 反序列化失败。

---

### `xhjob_state`

```php
xhjob_state(string $id, ?string $name = null, ?string $data_dir = null): array
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：键值对数组，含 `state` / `attempts` / `created_at` 等 **30+ 字段**：`state`、`attempts`、`created_at`、`started_at`（可选）、`finished_at`（可选）、`last_error`（可选）、`execution_count`、`max_executions`、`paused`、`start_date`、`end_date`、`meta`、`interval`、`run_at`、`jitter`、`expires`、`retry_backoff`、`ignore_result`、`acks_late`、`soft_timeout`、`misfire_grace_time`、`tags`（JSON 字符串）、`rate_limit_count`、`rate_limit_window`、`acks_on_failure`、`timezone`、`coalesce`、`progress`、`progress_meta`、`worker_pid`、`worker_starttime`。

**错误契约**：任务不存在或 daemon 不可达时返回 `["state" => "UNKNOWN", "error" => "<原因>"]`，用 `isset($r['error'])` 或 `$r['state'] === 'UNKNOWN'` 检测。

**注意事项**
- 所有值均为字符串（`Vec<(String, String)>`）；数值字段需自行 `(int)` 转换。
- `tags` 字段是 JSON 数组字符串，需 `json_decode($r['tags'], true)` 取回数组。
- 可选字段（`started_at` 等）仅在对应值存在时才出现在数组中。

**代码演示**

```php
<?php
$s = xhjob_state($taskId, 'default');
if (isset($s['error'])) {
    echo "查询失败：{$s['error']}\n";
    return;
}
printf("state=%s attempts=%d\n", $s['state'], (int)$s['attempts']);
if (($s['state'] ?? '') === 'failed' && isset($s['last_error'])) {
    echo "上次错误：{$s['last_error']}\n";
}
```

**生产建议**：轮询时优先关注 `state` 是否进入终态（`success` / `failed` / `cancelled` / `expired` / `interrupted`），避免无限轮询。

---

### `xhjob_result`

```php
xhjob_result(string $id, ?string $name = null, ?string $data_dir = null): array
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：键值对数组，含 `body` / `body_b64` / `status_code` / `stdout` / `stderr` / `exit_code`（均为可选，按任务类型与产出情况出现）。

**错误契约**：无结果记录时返回 `["error" => "no result record for this task ..."]`，用 `isset($r['error'])` 检测。这是预期的业务条件（如 `ignoreResult=true` 或任务在产出前失败），不是 daemon 故障。

**注意事项**
- shell 任务产出 `stdout` / `stderr` / `exit_code`；http 任务产出 `body`（或 `body_b64`） / `status_code`。
- 当 HTTP 响应为非 UTF-8 二进制时，结果含 `body_b64`（base64 编码），调用方用 `base64_decode($r['body_b64'])` 恢复原始字节。
- 结果受 `result_ttl` 控制（`0` = 永久保留）；`ignoreResult=true` 的任务不会产出结果。

**代码演示**

```php
<?php
$r = xhjob_result($taskId, 'default');
if (isset($r['error'])) {
    echo "无结果：{$r['error']}\n";
    return;
}
if (isset($r['body_b64'])) {
    $bytes = base64_decode($r['body_b64']);
} else {
    $text = $r['stdout'] ?? $r['body'] ?? '';
}
echo "exit_code=" . ($r['exit_code'] ?? $r['status_code'] ?? '-') . "\n";
```

**生产建议**：取结果前先用 `xhjob_state()` 确认已进入终态；终态前结果可能尚未写入。

---

### `xhjob_get`

```php
xhjob_get(string $id, ?string $name = null, ?string $data_dir = null): ?string
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`?string`。任务存在时返回完整任务定义 JSON 字符串；不存在时返回 `null`。

**错误契约**：任务不存在返回 `null`。与 `xhjob_state`（返回 trimmed `StateInfo` 视图）不同，`xhjob_get` 返回 **全部持久化字段**（含配置字段）。

**注意事项**
- 返回的是任务定义（创建时的配置快照），不是执行结果——执行结果用 `xhjob_result`。
- IPC 错误时也会返回 `null`，无法与"任务不存在"区分；如需精确区分，先调 `xhjob_status` 确认 daemon 在线。

**代码演示**

```php
<?php
$json = xhjob_get($taskId, 'default');
if ($json === null) {
    echo "任务不存在或 daemon 不可达\n";
    return;
}
$def = json_decode($json, true);
print_r($def['payload'] ?? []);
```

**生产建议**：用于任务配置审计 / 迁移；不要用它判断任务是否在运行（用 `xhjob_state`）。

---

### `xhjob_list`

```php
xhjob_list(?string $name = null, ?string $state_filter = null, ?string $tag = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$name` | `?string` | 服务名 |
| `$state_filter` | `?string` | 状态过滤，如 `pending` / `running` / `success` / `failed` |
| `$tag` | `?string` | 标签过滤（透传到 daemon 端 `tag_filter`） |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 JSON `{"tasks":[...]}`；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- 注意返回的是 **JSON 字符串**，需 `json_decode($r, true)` 取回关联数组。
- `$state_filter` 与 `$tag` 可同时使用（AND 语义）。
- 不传任何过滤时返回该服务下全部任务（可能较大，生产环境建议加分页或状态过滤）。

**代码演示**

```php
<?php
$r = xhjob_list('default', 'failed', null);
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('list 失败：' . substr($r, 6));
}
$tasks = json_decode($r, true)['tasks'] ?? [];
foreach ($tasks as $t) {
    echo $t['id'] . "\n";
}
```

**生产建议**：监控面板按 `state_filter=failed` 拉取失败任务告警；避免全量拉取造成大响应。

---

## 控制（7 个）

### `xhjob_remove`

```php
xhjob_remove(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`bool`。删除成功返回 `true`；任务不存在、服务名非法或 IPC 失败返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 从存储中删除任务定义，**不影响**已在运行的实例（运行中的实例会执行完毕）。
- 服务名非法会写入 `tracing::error!` 后返回 `false`。

**代码演示**

```php
<?php
if (!xhjob_remove($taskId, 'default')) {
    echo "删除失败（任务可能不存在）\n";
}
```

**生产建议**：定时清理已完成任务，避免 SQLite 表无限增长。

---

### `xhjob_pause`

```php
xhjob_pause(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 暂停后任务的 `paused` 字段置为 `true`，调度器跳过其触发；运行中的实例不会被中断。
- 用 `xhjob_resume` 恢复。

**代码演示**

```php
<?php
xhjob_pause($taskId, 'default');
// 维护窗口结束后
xhjob_resume($taskId, 'default');
```

**生产建议**：维护窗口期间批量暂停 cron 任务，避免维护中的副作用调用。

---

### `xhjob_resume`

```php
xhjob_resume(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**：恢复被 `xhjob_pause` 暂停的任务；`paused` 置回 `false`，下次触发正常调度。

**代码演示**：见 `xhjob_pause`。

**生产建议**：恢复后用 `xhjob_state` 确认 `paused=false` 与 `next_fire` 已重算。

---

### `xhjob_cancel`

```php
xhjob_cancel(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 取消任务：Pending 任务转 `cancelled` 终态不再触发；Running 任务会收到取消信号并转 `cancelled`。
- 与 `xhjob_remove` 的区别：cancel 保留任务记录与状态历史，remove 彻底删除。

**代码演示**

```php
<?php
// 用户取消订单关联的延时任务
if (!xhjob_cancel($orderId . '-timeout', 'default')) {
    error_log('取消失败：任务可能已终态');
}
```

**生产建议**：业务取消优先用 `cancel`（保留审计记录），定期清理再用 `remove`。

---

### `xhjob_requeue`

```php
xhjob_requeue(string $id, ?string $name = null, ?string $data_dir = null): bool
```

**参数**：`$id` / `$name` / `$data_dir`。

**返回值**：`bool`。成功重新入队返回 `true`；任务不在可重入队终态（`cancelled` / `failed` / `expired`）或不存在返回 `false`。

**错误契约**：`false` 即失败。

**注意事项**
- 重置 `attempts` 为 0，`next_fire` 设为当前时间，任务回到 `pending`。
- 仅对终态任务有效；运行中或 pending 任务调用会返回 `false`。

**代码演示**

```php
<?php
// 修复后重试一个失败任务
if (xhjob_requeue($taskId, 'default')) {
    echo "已重新入队\n";
}
```

**生产建议**：配合 `acksOnFailure(false)` 的无限重试任务，requeue 用于人工干预后的强制重试。

---

### `xhjob_reschedule`

```php
xhjob_reschedule(string $id, string $cron, ?string $name = null, ?string $data_dir = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$cron` | `string` | 新的 5 字段 cron 表达式 |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败（cron 表达式非法 / 任务不存在 / IPC 错误）。

**注意事项**
- 仅修改 cron 表达式并重算 `next_fire`，不改变其他配置。
- 对非 cron 任务（interval / runAt）调用会返回 `false`。

**代码演示**

```php
<?php
// 从每小时改为每 15 分钟
xhjob_reschedule($taskId, '*/15 * * * *', 'default');
```

**生产建议**：动态调整频率时用 reschedule，比 remove + 重建更轻量且保留任务 id 与历史。

---

### `xhjob_modify`

```php
xhjob_modify(string $id, string $patch_json, ?string $name = null, ?string $data_dir = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$patch_json` | `string` | 补丁 JSON（键值对，覆盖对应字段） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`bool`。成功返回 `true`，失败返回 `false`。

**错误契约**：`false` 即失败。补丁 JSON 解析失败会写入 `tracing::error!("invalid patch json")` 后返回 `false`。

**注意事项**
- `$patch_json` 必须是合法 JSON 对象；非法 JSON 直接返回 `false`。
- 适合细粒度字段更新（如改 `priority` / `max_instances`），比 `reschedule`（仅改 cron）更通用。

**代码演示**

```php
<?php
$patch = json_encode(['priority' => 10, 'max_instances' => 3]);
if (!xhjob_modify($taskId, $patch, 'default')) {
    throw new RuntimeException('修改失败');
}
```

**生产建议**：优先用 `modify` 做增量字段更新，避免 remove + 重建丢失运行历史。

---

## 编排（6 个）

### `xhjob_chain`

```php
xhjob_chain(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$tasks_json` | `string` | TaskBuilder 配置对象的 JSON 数组 |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 chain_id；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败（JSON 非法 / IPC 错误 / daemon 返回 `ok=false`）。

**注意事项**
- 顺序管道：每个任务的 stdout 作为下一个任务的 stdin；任一步失败则 chain 转 `failed`，跳过剩余步骤。
- `$tasks_json` 是 JSON 数组，如 `[{"task_type":"shell","payload":{"cmd":"echo a"}}, ...]`。

**代码演示**

```php
<?php
$tasks = [
    ['task_type' => 'shell', 'payload' => ['cmd' => 'echo hello-world']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'grep hello']],
];
$r = xhjob_chain(json_encode($tasks), 'default');
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('chain 失败：' . substr($r, 6));
}
$chainId = $r;
```

**生产建议**：用 `TaskBuilder::chain([...])` 构建而非手拼 JSON，保证字段结构正确。

---

### `xhjob_chain_state`

```php
xhjob_chain_state(string $chain_id, ?string $name = null, ?string $data_dir = null): ?string
```

**参数**：`$chain_id` / `$name` / `$data_dir`。

**返回值**：`?string`。chain 存在时返回完整 `ChainRecord` JSON（`chain_id` / `tasks` / `current_step` / `state` / `created_at` / `updated_at`）；不存在或 daemon 不可达返回 `null`。

**错误契约**：`null` 即不存在 / 不可达。

**注意事项**：返回 JSON 字符串，需 `json_decode` 取回数组。

**代码演示**

```php
<?php
$json = xhjob_chain_state($chainId, 'default');
if ($json === null) {
    echo "chain 不存在\n";
    return;
}
$rec = json_decode($json, true);
echo "step={$rec['current_step']} state={$rec['state']}\n";
```

**生产建议**：监控 chain 的 `current_step` 与 `state`，`failed` 时定位失败步骤。

---

### `xhjob_group`

```php
xhjob_group(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
```

**参数**：同 `xhjob_chain`。

**返回值**：`string`。成功为 group_id；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- 并行批量：所有子任务并发派发。终态为 `success`（全部成功）/ `partial_failed`（部分失败）/ `failed`（全部失败）。

**代码演示**

```php
<?php
$tasks = [
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s http://a']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s http://b']],
];
$r = xhjob_group(json_encode($tasks), 'default');
$groupId = str_starts_with($r, 'error:') ? null : $r;
```

**生产建议**：批量通知 / 报表生成适合 group；注意 `rateLimit` 防止瞬时打爆下游。

---

### `xhjob_group_state`

```php
xhjob_group_state(string $group_id, ?string $name = null, ?string $data_dir = null): ?string
```

**参数**：`$group_id` / `$name` / `$data_dir`。

**返回值**：`?string`。group 存在时返回 `GroupRecord` JSON；不存在 / 不可达返回 `null`。

**错误契约**：`null` 即不存在 / 不可达。

**注意事项**：返回 JSON 字符串，需 `json_decode`。

**代码演示**

```php
<?php
$json = xhjob_group_state($groupId, 'default');
$rec = $json === null ? null : json_decode($json, true);
echo $rec['state'] ?? 'unknown';
```

**生产建议**：`partial_failed` 时遍历子任务 state 定位失败项。

---

### `xhjob_chord`

```php
xhjob_chord(string $header_json, string $callback_json, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$header_json` | `string` | header 任务配置的 JSON 数组（并行执行） |
| `$callback_json` | `string` | 回调任务配置的 JSON 对象（全部 header 成功后执行） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 chord_id；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- 参考 Celery chord：header 全部成功后才派发 callback，callback 的 `meta` 携带所有 header 结果。
- 任一 header 失败时 chord 转 `partial_failed` 终态，**不派发 callback**。

**代码演示**

```php
<?php
$headers = [
    ['task_type' => 'shell', 'payload' => ['cmd' => 'shard-1.sh']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'shard-2.sh']],
];
$callback = ['task_type' => 'shell', 'payload' => ['cmd' => 'merge.sh']];
$r = xhjob_chord(json_encode($headers), json_encode($callback), 'default');
$chordId = str_starts_with($r, 'error:') ? null : $r;
```

**生产建议**：Map-Reduce 场景的理想原语；header 用 group 并行计算，callback 汇总。

---

### `xhjob_chord_state`

```php
xhjob_chord_state(string $chord_id, ?string $name = null, ?string $data_dir = null): ?string
```

**参数**：`$chord_id` / `$name` / `$data_dir`。

**返回值**：`?string`。chord 存在时返回 `ChordRecord` JSON（含 `id` / `header_task_ids` / `callback_json` / `callback_task_id` / `state` / `created_at` / `updated_at`）；不存在 / 不可达返回 `null`。

**错误契约**：`null` 即不存在 / 不可达。

**注意事项**：返回 JSON 字符串，需 `json_decode`。

**代码演示**

```php
<?php
$json = xhjob_chord_state($chordId, 'default');
$rec = $json === null ? null : json_decode($json, true);
echo $rec['state'] ?? 'unknown';
```

**生产建议**：`partial_failed` 时检查 `header_task_ids` 中各 header 任务的 `last_error`。

---

## 事件与进度（4 个）

### `xhjob_events`

```php
xhjob_events(int $since_ts, ?string $task_id = null, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$since_ts` | `int` | 起始 Unix 时间戳（秒） |
| `$task_id` | `?string` | 任务 ID 过滤（`null` = 全部任务） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 JSON 数组 `[{task_id, event_type, payload, ts}, ...]`；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- 返回 JSON 字符串，需 `json_decode($r, true)` 取回数组。
- `$task_id` 过滤仅返回该任务的事件。

**代码演示**

```php
<?php
$r = xhjob_events(time() - 3600, $taskId, 'default');
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('events 失败：' . substr($r, 6));
}
foreach (json_decode($r, true) as $ev) {
    printf("[%d] %s: %s\n", $ev['ts'], $ev['event_type'], $ev['task_id']);
}
```

**生产建议**：审计 / 调试单个任务生命周期用 `events`；全局事件流用 `pull_events`。

---

### `xhjob_pull_events`

```php
xhjob_pull_events(int $since_ts, ?string $event_type = null, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$since_ts` | `int` | 起始 Unix 时间戳（秒） |
| `$event_type` | `?string` | 事件类型过滤（`started` / `succeeded` / `failed` / ...） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 JSON 数组 `[{task_id, event_type, payload, ts}, ...]`；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- 与 `xhjob_events` 的区别：`events` 按 `task_id` 过滤，`pull_events` 按 `event_type` 过滤。
- 适合按事件类型订阅（如只拉取 `failed` 事件做告警）。

**代码演示**

```php
<?php
$r = xhjob_pull_events(time() - 600, 'failed', 'default');
if (!str_starts_with($r, 'error:')) {
    $fails = json_decode($r, true);
    // 推送告警...
}
```

**生产建议**：告警网关按 `event_type=failed` 定时拉取，配合 `since_ts` 做增量消费。

---

### `xhjob_report_progress`

```php
xhjob_report_progress(string $id, int $percent, ?string $meta_json = null, ?string $name = null, ?string $data_dir = null): bool
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$id` | `string` | 任务 ID |
| `$percent` | `int` | 进度百分比，**必须 0-100** |
| `$meta_json` | `?string` | 任意 JSON 元数据 |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`bool`。上报成功返回 `true`；`$percent` 越界（非 0-100）**直接返回 `false`**（不联系 daemon）；服务名非法 / IPC 失败返回 `false`。

**错误契约**：`false` 即失败。越界检查在联系 daemon 之前完成。

**注意事项**
- 参考 Celery `update_state(state='PROGRESS', meta=...)`。
- 进度值可通过 `xhjob_state` 的 `progress` / `progress_meta` 字段读回。

**代码演示**

```php
<?php
// 在长时间运行的脚本任务内（需通过 chain/自定义 worker 写入）
for ($i = 1; $i <= 100; $i++) {
    do_chunk($i);
    xhjob_report_progress($taskId, $i, json_encode(['chunk' => $i]));
}
```

**生产建议**：进度上报频率不宜过高（建议按 5%-10% 步进），避免 IPC 压力；越界值会被静默丢弃，调用方需自行校验。

---

### `xhjob_inspect`

```php
xhjob_inspect(string $mode, ?string $name = null, ?string $data_dir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$mode` | `string` | 查询模式：`active` / `registered` / `scheduled` / `stats`（默认 `stats`） |
| `$name` | `?string` | 服务名 |
| `$data_dir` | `?string` | 数据目录 |

**返回值**：`string`。成功为 JSON（`active`/`registered`/`scheduled` 返回数组，`stats` 返回对象）；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。

**注意事项**
- `active`：当前运行中的任务；`registered`：cron / interval 任务；`scheduled`：有未来 `next_fire` 的任务；`stats`：聚合 `WorkerStats`（默认）。

**代码演示**

```php
<?php
$r = xhjob_inspect('stats', 'default');
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('inspect 失败：' . substr($r, 6));
}
$stats = json_decode($r, true);
print_r($stats);
```

**生产建议**：监控面板用 `stats` 看全局负载，`active` 看实时并发，`scheduled` 预判未来调度压力。

---

## `Xhjob` 链式构建类

`Xhjob` 是 Rust 侧暴露的链式任务构建器（与 ThinkPHP 扩展包的 `TaskBuilder` PHP 类功能等价但来源不同）。所有配置方法返回 `&mut self`（PHP 侧可链式调用），`dispatch()` 终结并返回 task_id。

> **命名注意**：ext-php-rs 会把 Rust 的 `snake_case` 方法名自动转为 PHP 的 `camelCase`（如 `with_retry` → `withRetry`）。但 **指定任务 ID 的方法实际暴露名为 `id`**（不是 `withId`）——尽管其 Rust doc 注释写的是 `PHP: withId(string $id): $this`，由于 `id` 是单词无下划线、不触发转换，**PHP 侧真实方法名是 `id()`**。下文签名均按真实暴露名书写。

### 入口与绑定

#### `Xhjob::task`

```php
Xhjob::task(): Xhjob
```

**参数**：无。

**返回值**：`Xhjob`。新建一个使用默认 `TaskBuilder` 的实例。

**错误契约**：不抛异常。

**注意事项**：静态工厂入口，等价于 `new Xhjob()`（构造受 `#[php_class]` 控制，外部通过 `task()` 创建）。

**代码演示**

```php
<?php
$x = Xhjob::task();
```

**生产建议**：所有链式调用的起点。

#### `service`

```php
service(string $name): $this
```

**参数**：`$name` — 服务名。

**返回值**：`$this`。

**错误契约**：不抛异常（服务名校验延迟到 `dispatch`）。

**注意事项**：绑定到命名服务，后续 `dispatch()` 路由到该服务 daemon。

**代码演示**：`Xhjob::task()->service('cron-svc')->viaShell('echo hi')->dispatch();`

**生产建议**：多服务场景显式绑定，避免依赖默认 `default`。

#### `dataDir`

```php
dataDir(string $dir): $this
```

**参数**：`$dir` — 数据目录路径。

**返回值**：`$this`。

**错误契约**：不抛异常。

**注意事项**：用于备份 / 迁移 / 恢复场景，把 IPC socket 解析到自定义目录。

**代码演示**：`Xhjob::task()->dataDir('/var/lib/xhjob')->...`

**生产建议**：仅在多目录部署时使用。

### 任务类型

#### `viaShell`

```php
viaShell(string $cmd): $this
```

**参数**：`$cmd` — shell 命令。

**返回值**：`$this`。

**错误契约**：不抛异常。

**注意事项**：切换为 shell 任务，命令通过子进程执行。

**代码演示**：`Xhjob::task()->viaShell('ls -la')->dispatch();`

**生产建议**：命令中避免拼接用户输入，防注入。

#### `viaHttp`

```php
viaHttp(string $method, string $url): $this
```

**参数**：`$method` — HTTP 方法；`$url` — 请求 URL。

**返回值**：`$this`。

**错误契约**：不抛异常。

**注意事项**：切换为 http 任务。

**代码演示**：`Xhjob::task()->viaHttp('GET', 'https://api.x/y')->dispatch();`

**生产建议**：非幂等方法（POST/PUT/DELETE/PATCH）默认对 5xx 不重试，需重试时声明 `idempotent(true)`。

### HTTP 专属

#### `withHeaders`

```php
withHeaders(array $headers): $this
```

**参数**：`$headers` — 键值对请求头（PHP 关联数组转 `Vec<(String,String)>`）。

**返回值**：`$this`。

**错误契约**：不抛异常（覆盖之前设置）。

**注意事项**：关联数组直接传入即可。

**代码演示**：`->withHeaders(['Authorization' => 'Bearer x'])->...`

**生产建议**：敏感头（token）从环境变量取，勿硬编码。

#### `withBody`

```php
withBody(string $body): $this
```

**参数**：`$body` — 请求体字符串。

**返回值**：`$this`。

**错误契约**：不抛异常。

**注意事项**：JSON 请求体需自行 `json_encode`。

**代码演示**：`->withBody(json_encode(['k' => 'v']))->...`

**生产建议**：大 body 注意超时配置。

#### `withProxy`

```php
withProxy(string $proxy): $this
```

**参数**：`$proxy` — 代理 URL（支持 `http://` / `https://` / `socks5://` / `socks5h://`，可含 `user:pass@`）。

**返回值**：`$this`。

**错误契约**：不抛异常。

**注意事项**：写到任务顶层 `proxy` 字段（非 `payload.proxy`）。

**代码演示**：`->withProxy('socks5://127.0.0.1:1080')->...`

**生产建议**：内网爬取 / 出海请求走代理。

### Shell 专属

#### `withEncoding`

```php
withEncoding(string $from): $this
```

**参数**：`$from` — 编码标签（`GBK` / `Big5` / `Shift_JIS` / `auto` 等，大小写不敏感）。

**返回值**：`$this`。

**错误契约**：不抛异常；非法标签在解码时按默认处理。

**注意事项**：stdout/stderr 字节流按此编码解码为 UTF-8；`auto` 在 Windows 触发 OEM 代码页检测（Unix 无操作）。

**代码演示**：`->withEncoding('GBK')->viaShell('chcp 936 && dir')->...`

**生产建议**：Windows / 中文环境 shell 任务务必设置编码，避免乱码。

### 触发器

| 方法 | 签名 | 写入字段 | 说明 |
| --- | --- | --- | --- |
| `cron` | `cron(string $expr): $this` | `cron` | 5 字段 cron 表达式 |
| `orCron` | `orCron(array $exprs): $this` | `or_cron` | 附加 cron，任一匹配即触发（F-1） |
| `skipDates` | `skipDates(array $ts): $this` | `skip_dates` | 跳过指定日期（任务时区） |
| `workdaysOnly` | `workdaysOnly(): $this` | `workdays_only` | 仅工作日（周一至周五）触发 |
| `every` | `every(int $secs): $this` | `interval` | 固定间隔秒；与 `cron`/`runAt` 同时设时后者优先 |
| `runAt` | `runAt(int $ts): $this` | `run_at` | 一次性绝对时间戳，最高优先级 |
| `startAt` | `startAt(int $ts): $this` | `start_date` | 起始时间戳，之前不触发 |
| `endAt` | `endAt(int $ts): $this` | `end_date` | 结束时间戳，之后转 `success` 终态 |
| `jitter` | `jitter(int $secs): $this` | `jitter` | 调度抖动，避免惊群；`runAt` 任务忽略 |
| `withTimezone` | `withTimezone(string $tz): $this` | `timezone` | IANA 时区，`next_fire` 按此时区计算 |
| `misfireGraceTime` | `misfireGraceTime(int $secs): $this` | `misfire_grace_time` | 误触发宽限秒，0=用全局默认 60s |

**错误契约**：均不抛异常（校验延迟到 `dispatch`，非法时区 / cron 会导致 `dispatch` 返回 `error:`）。

**注意事项**
- `runAt` 优先级最高（覆盖 `cron` + `interval`）。
- `misfireGraceTime` 仅对 cron 任务有效，配合 `coalesce` 决定漏触发是合并执行还是跳过。

**代码演示**

```php
<?php
Xhjob::task()
    ->viaShell('backup.sh')
    ->cron('0 2 * * 1-5')        // 工作日凌晨 2 点
    ->withTimezone('Asia/Shanghai')
    ->misfireGraceTime(300)
    ->dispatch();
```

**生产建议**：跨时区任务显式设 `withTimezone`，勿依赖系统时区。

### 重试 / 超时

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `withRetry` | `withRetry(int $max, int $delay = 1): $this` | 最大重试次数与间隔 |
| `retryBackoff` | `retryBackoff(bool $on = true): $this` | 指数退避 `min(delay*2^(attempts-1), delay*60)` |
| `timeout` | `timeout(int $secs): $this` | 硬超时，到期 SIGKILL |
| `softTimeout` | `softTimeout(int $secs): $this` | 软超时，到期 SIGTERM，未退出再 SIGKILL；`<=0` 清除 |
| `maxExecutions` | `maxExecutions(int $n): $this` | 最大执行次数，0=无限 |

**错误契约**：均不抛异常。负值会被归零处理（`maxExecutions` 负值转 0，`softTimeout` `<=0` 清除为 None）。

**注意事项**
- `softTimeout` 必须严格小于 `timeout` 才生效；HTTP 任务忽略 `softTimeout`。

**代码演示**

```php
<?php
Xhjob::task()
    ->viaShell('long-job.sh')
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->softTimeout(60)->timeout(90)
    ->dispatch();
```

**生产建议**：长任务务必设 `softTimeout < timeout`，给业务优雅退出窗口。

### 并发

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `priority` | `priority(int $p): $this` | 优先级，数值越大越优先 |
| `maxInstances` | `maxInstances(int $n): $this` | 最大并发实例数 |
| `allowOverlap` | `allowOverlap(bool $on = true): $this` | 允许重叠执行 |
| `rateLimit` | `rateLimit(int $count, int $window): $this` | 滑动窗口限流，`count=0` 关闭 |

**错误契约**：不抛异常。

**代码演示**

```php
<?php
Xhjob::task()
    ->viaHttp('GET', 'https://api/third-party')
    ->cron('*/1 * * * *')
    ->rateLimit(10, 60)        // 60 秒内最多 10 次
    ->maxInstances(1)
    ->dispatch();
```

**生产建议**：调用第三方 API 必设 `rateLimit`，防被限流封禁。

### 可靠性

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `coalesce` | `coalesce(bool $on = true): $this` | 合并漏触发为一次执行 |
| `persist` | `persist(bool $on = true): $this` | 持久化任务定义 |
| `acksLate` | `acksLate(bool $on = true): $this` | 延迟确认；daemon 重启时重置 Running→Pending |
| `acksOnFailure` | `acksOnFailure(bool $on = true): $this` | `false`=失败不 ack，无限重试至成功 / 取消 |
| `idempotent` | `idempotent(bool $on = true): $this` | 声明 HTTP 任务幂等，非幂等方法也重试 5xx |
| `ignoreResult` | `ignoreResult(bool $on = true): $this` | fire-and-forget，不存结果；与 `resultTtl>0` 冲突时本项优先 |
| `resultTtl` | `resultTtl(int $secs): $this` | 结果保留秒数，0=永久 |
| `expires` | `expires(int $secs): $this` | Pending 超 `secs` 秒转 `expired` 终态；0=不过期 |

**错误契约**：不抛异常。

**代码演示**

```php
<?php
Xhjob::task()
    ->viaHttp('POST', 'https://api/charge')
    ->idempotent(true)->acksLate(true)
    ->withRetry(5, 10)
    ->dispatch();
```

**生产建议**：幂等的副作用任务用 `idempotent(true)` + `acksLate(true)` 保至少一次执行；非幂等任务保持默认不重试。

### 元数据 / 身份

#### `withMeta`

```php
withMeta(string $json): $this
```

**参数**：`$json` — 任意 JSON 字符串元数据。

**返回值**：`$this`。

**错误契约**：不抛异常。

**代码演示**：`->withMeta(json_encode(['order_id' => 42]))->...`

#### `tag`

```php
tag(string $tag): $this
```

**参数**：`$tag` — 标签名（空串静默忽略，重复标签去重）。

**返回值**：`$this`。

**代码演示**：`->tag('billing')->tag('nightly')->...`

#### `id`

```php
id(string $id): $this
```

**参数**：`$id` — 任务 ID。**须匹配 `^[A-Za-z0-9_-]{1,64}$`**。

**返回值**：`$this`。

**错误契约**：不抛异常。**空串** 视为清除（回退自动生成 UUID）；**非法字符 / 超长** 会在 `tracing::warn!` 记录后回退为自动生成（不 panic，因跨 extern "C" panic 是 UB）。

**注意事项**
- > ⚠️ **真实方法名是 `id`，不是 `withId`**。Rust doc 注释虽写 `withId`，但 ext-php-rs 对单词方法名不做转换，PHP 侧暴露为 `id()`。调用 `withId()` 会触发 "Call to undefined method" 错误。
- ID 校验规则防止两类 bug：以 `error:` 开头的 id 会破坏 PHP 侧 `str_starts_with($r, 'error:')` 判错契约；含 JSON / SQL 元字符的 id 会引发下游解析问题。

**代码演示**

```php
<?php
Xhjob::task()
    ->viaShell('cleanup.sh')
    ->id('nightly-cleanup')        // 注意是 id() 不是 withId()
    ->replaceExisting(true)
    ->cron('0 3 * * *')
    ->dispatch();
```

**生产建议**：业务幂等任务用稳定业务 id（如 `order-{$id}-timeout`）+ `replaceExisting(true)`，避免重复派发。

#### `replaceExisting`

```php
replaceExisting(bool $on = true): $this
```

**参数**：`$on`。

**返回值**：`$this`。

**注意事项**：`true` 且已设 `id` 时，dispatch 覆盖同 id 任务（全量覆写）。

**代码演示**：见 `id`。

### 终结

#### `dispatch`

```php
dispatch(): string
```

**参数**：无（服务名 / 数据目录通过 `service()` / `dataDir()` 预绑定）。

**返回值**：`string`。成功为 task_id；失败以 `error:` 前缀返回。

**错误契约**：`str_starts_with($r, 'error:')` 为真即失败。内部经 `block_on` 同步执行 `builder.dispatch()`，错误统一格式化为 `error: <e>`。

**注意事项**
- 调用后 builder 内部状态被 `take` 清空，**不可重复 dispatch 同一实例**。
- 与 `TaskBuilder::dispatch($service, $dataDir)` 的区别：`Xhjob` 通过 `service()` / `dataDir()` 绑定参数，不接受 dispatch 参数。

**代码演示**

```php
<?php
$r = Xhjob::task()
    ->service('cron-svc')
    ->viaShell('echo hi')
    ->cron('0 * * * *')
    ->dispatch();
if (str_starts_with($r, 'error:')) {
    throw new RuntimeException('dispatch 失败：' . substr($r, 6));
}
$taskId = $r;
```

**生产建议**：封装统一包装函数，把 `error:` 检测收敛到一处。
