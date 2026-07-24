---
title: TaskBuilder API
parent: API 参考
nav_order: 22
---

# TaskBuilder API

`Xhjob\TaskBuilder` 是 xhjob PHP 扩展的纯 PHP 链式任务构建器，负责生成 `xhjob_dispatch` / `xhjob_chain` / `xhjob_group` / `xhjob_chord` 所需的 JSON 结构。

{: .note }
本类位于命名空间 `Xhjob`，源文件 `Xhjob/TaskBuilder.php`。所有链式方法均返回 `$this`，配置方法不联系 daemon，只有 `dispatch()` 才会触发实际派发。

## 设计要点

- **默认配置与 Rust 端对齐**：构造时初始化的 `config` 数组与 Rust `TaskBuilder` 默认值一致。特别注意：`Option<u64>` / `Option<i64>` 字段（`interval` / `run_at` / `start_date` / `end_date` / `soft_timeout`）默认输出 `null`，否则 JSON 中的 `0` 会被 Rust 反序列化为 `Some(0)`，导致 interval 任务被误判为 DateTrigger 立即转 Success 终态。
- **构造方法为 `protected`**：必须通过静态工厂 `shell` / `http` / `chain` / `group` / `chord` / `fromJson` 创建实例。
- **类型校验**：HTTP 专属方法（`withHeaders` / `withBody` / `withProxy`）与 shell 专属方法（`withEncoding` / `withStdin` / `withWorkingDir` / `withEnv`）在类型不匹配时抛 `Xhjob\Exception\InvalidTaskConfigException`。

---

## 一、静态工厂

### shell

创建 shell 任务。

```php
public static function shell(string $cmd): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$cmd` | `string` | shell 命令 |

**用途**：构建 `task_type=shell`、`payload={cmd:...}` 的任务。

```php
$b = TaskBuilder::shell('echo hello');
```

### http

创建 http 任务。

```php
public static function http(string $method, string $url): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$method` | `string` | HTTP 方法（GET / POST / PUT / DELETE ...），内部转大写 |
| `$url` | `string` | 请求 URL |

**注意事项**：payload 中 `headers` 初始化为 `(object)[]`（空对象 `{}`，非数组 `[]`），以匹配 Rust `HttpPayload.headers: HashMap<String,String>` 的反序列化要求。

```php
$b = TaskBuilder::http('POST', 'https://api.test/users');
```

### chain

创建任务链。dispatch 时调用 `xhjob_chain`，按顺序执行每个子任务，上一个任务的 stdout 作为下一个任务的 stdin。

```php
public static function chain(array $builders): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$builders` | `array` | `TaskBuilder` 实例数组 |

```php
$b = TaskBuilder::chain([
    TaskBuilder::shell('echo "line1\nline2"'),
    TaskBuilder::shell('grep line2'),
]);
$chainId = $b->dispatch();
```

### group

创建任务组。dispatch 时调用 `xhjob_group`，并行执行所有子任务。

```php
public static function group(array $builders): self
```

```php
$b = TaskBuilder::group([
    TaskBuilder::http('GET', 'https://a.test'),
    TaskBuilder::http('GET', 'https://b.test'),
]);
$groupId = $b->dispatch();
```

### chord

创建 chord（header + body 回调）。dispatch 时调用 `xhjob_chord`，并行执行所有 header 任务，全部成功后执行 callback，callback 的 meta 携带所有 header 结果。任一 header 失败时 chord 转 `partial_failed` 终态，不派发 callback。参考 Celery chord。

```php
public static function chord(array $headerBuilders, self $callback): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$headerBuilders` | `array` | header `TaskBuilder` 实例数组 |
| `$callback` | `self` | 回调 `TaskBuilder`（body） |

```php
$b = TaskBuilder::chord(
    [TaskBuilder::shell('echo 1'), TaskBuilder::shell('echo 2')],
    TaskBuilder::shell('echo done')
);
$chordId = $b->dispatch();
```

### fromJson

从 JSON 字符串反序列化构建器。

```php
public static function fromJson(string $json): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$json` | `string` | TaskBuilder JSON 字符串 |

**注意事项**：解析失败时 `config` 退化为空数组（不抛异常），调用方应在 `dispatch` 前确认配置完整。

```php
$b = TaskBuilder::fromJson('{"task_type":"shell","payload":{"cmd":"echo hi"},"cron":"0 * * * *"}');
$id = $b->dispatch();
```

---

## 二、触发器

### cron

设置 5 字段 cron 表达式。

```php
public function cron(string $expr): self
```

```php
TaskBuilder::shell('backup')->cron('0 2 * * *'); // 每天 2 点
```

### every

设置固定间隔（秒），对应 IntervalTrigger。

```php
public function every(int $secs): self
```

```php
TaskBuilder::shell('poll')->every(60); // 每 60 秒
```

### runAt

设置一次性运行时间戳（Unix 秒），对应 DateTrigger。

```php
public function runAt(int $ts): self
```

```php
TaskBuilder::shell('one-shot')->runAt(time() + 300); // 5 分钟后
```

### countdown

设置倒计时延迟（秒），等价于 `runAt(time() + $secs)`。参考 Celery `apply_async(countdown=N)`。

```php
public function countdown(int $secs): self
```

{: .warning }
**`countdown` 与 `runAt` 互斥**：同时设置时 **`runAt` 优先**，`runAt` 会覆盖 `countdown`。

```php
TaskBuilder::shell('delayed')->countdown(120)->dispatch(); // 2 分钟后
// 下面这个 runAt 会覆盖 countdown
TaskBuilder::shell('at-3pm')->countdown(120)->runAt(strtotime('today 15:00'));
```

### startAt

设置任务起始时间戳，此前 cron 触发被跳过。

```php
public function startAt(int $ts): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->startAt(strtotime('2026-08-01'));
```

### endAt

设置任务结束时间戳，此后任务转 Success 终态。

```php
public function endAt(int $ts): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->endAt(strtotime('2026-12-31'));
```

---

## 三、重试与超时

### withRetry

设置重试策略。

```php
public function withRetry(int $max, int $delay = 1): self
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$max` | `int` | — | 最大重试次数 |
| `$delay` | `int` | `1` | 重试间隔（秒） |

```php
TaskBuilder::shell('flaky-cmd')->withRetry(3);        // delay 默认 1
TaskBuilder::shell('flaky-cmd')->withRetry(3, 5);     // 重试 3 次，间隔 5 秒
```

### retryBackoff

启用 / 禁用指数退避重试。启用后重试延迟按 `min(retry_delay * 2^(attempts-1), retry_delay * 60)` 增长。参考 Celery `retry_backoff`。

```php
public function retryBackoff(bool $on = true): self
```

```php
TaskBuilder::shell('flaky-cmd')->withRetry(5, 2)->retryBackoff(true);
```

### timeout

设置单次执行超时（秒）。

```php
public function timeout(int $secs): self
```

```php
TaskBuilder::shell('long-job')->timeout(120);
```

### softTimeout

设置软超时（秒），对应 Celery `soft_time_limit`。shell 执行器在 `soft_timeout` 秒后发 SIGTERM，超时仍未退出再发 SIGKILL。

```php
public function softTimeout(int $secs): self
```

{: .warning }
**必须严格小于 `timeout`**，否则配置非法。**HTTP 任务不支持软超时**——对 http 任务调用本方法时不会立即报错，但派发后 Rust 侧会忽略该字段；若需在构建期强制校验，应自行检查 `task_type` 并抛 `InvalidTaskConfigException`。

```php
// shell 任务：60s 硬超时，50s 软超时
TaskBuilder::shell('job')->timeout(60)->softTimeout(50);

// HTTP 任务不应使用 softTimeout（会被忽略）
// $b = TaskBuilder::http('GET', 'https://a.test')->softTimeout(5); // 不推荐
```

---

## 四、并发控制

### priority

设置优先级（数值越大越优先）。

```php
public function priority(int $p): self
```

```php
TaskBuilder::shell('urgent-job')->priority(10);
```

### maxInstances

设置最大并发实例数。

```php
public function maxInstances(int $n): self
```

```php
TaskBuilder::shell('job')->cron('* * * * *')->maxInstances(3);
```

### allowOverlap

是否允许同一任务重叠执行（默认 false）。

```php
public function allowOverlap(bool $on = true): self
```

```php
TaskBuilder::shell('job')->cron('* * * * *')->allowOverlap(true);
```

### coalesce

是否合并误触发（默认 true）：`true` 把错过的触发合并为一次（仍执行一次），`false` 直接跳过。

```php
public function coalesce(bool $on = true): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->coalesce(false);
```

### maxExecutions

设置 cron 任务最大执行次数（0 = 无限）。

```php
public function maxExecutions(int $n): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->maxExecutions(100);
```

### jitter

设置抖动（秒），随机偏移叠加到 cron / interval 的 `next_fire` 以避免惊群。

```php
public function jitter(int $secs): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->jitter(30);
```

### expires

设置任务级过期（秒）：Pending 超过该时长转 `Expired` 终态。仅影响 Pending，不中断 Running。

```php
public function expires(int $secs): self
```

```php
TaskBuilder::shell('job')->runAt(time() + 600)->expires(300);
```

### misfireGraceTime

设置误触发宽限时间（秒）。0 = 用全局默认（60s）。仅 cron 任务有效。

```php
public function misfireGraceTime(int $secs): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->misfireGraceTime(120);
```

### rateLimit

设置速率限制：`window` 秒内最多 `count` 次触发，count=0 表示不限。参考 Celery `rate_limit`。

```php
public function rateLimit(int $count, int $window): self
```

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$count` | `int` | 窗口内允许的次数 |
| `$window` | `int` | 窗口大小（秒） |

```php
TaskBuilder::shell('job')->rateLimit(10, 60); // 每分钟最多 10 次
```

---

## 五、可靠性

### persist

是否持久化任务（默认 false）。

```php
public function persist(bool $on = true): self
```

```php
TaskBuilder::shell('job')->cron('0 * * * *')->persist(true);
```

### acksLate

启用延迟确认：daemon 重启时 Running 的 `acksLate=true` 任务自动重置为 Pending（崩溃恢复语义）。参考 Celery `acks_late`。

```php
public function acksLate(bool $on = true): self
```

```php
TaskBuilder::shell('job')->acksLate(true);
```

### acksOnFailure

失败时是否确认（默认 true）。`true` 失败遵循 `retry_max`；`false` 失败无限重试直到成功或被取消 / 删除。参考 Celery `acks_on_failure`。

```php
public function acksOnFailure(bool $on = true): self
```

```php
TaskBuilder::shell('must-succeed')->withRetry(0, 1)->acksOnFailure(false);
```

### ignoreResult

启用 fire-and-forget：daemon 跳过 `save_result`。若同时设 `ignoreResult(true)` 和 `resultTtl(>0)`，记日志且 `ignoreResult` 优先。参考 Celery `ignore_result`。

```php
public function ignoreResult(bool $on = true): self
```

```php
TaskBuilder::shell('high-throughput')->cron('* * * * *')->ignoreResult(true);
```

### resultTtl

设置结果 TTL（秒），0 = 永久保留。

```php
public function resultTtl(int $secs): self
```

```php
TaskBuilder::shell('job')->resultTtl(3600);
```

---

## 六、元数据与身份

### withId

指定任务 ID。

```php
public function withId(string $id): self
```

{: .warning }
Rust 侧校验 ID 必须 **匹配 `^[A-Za-z0-9_-]{1,64}$`**（P0-2 fix）。非法 ID 在 Rust `dispatch` 阶段被拒绝并回退为自动生成。建议在 PHP 侧提前校验，避免派发后 ID 与预期不符。

```php
TaskBuilder::shell('job')->withId('nightly-backup-2026');
```

### replaceExisting

启用后配合 `withId` 使用，dispatch 会整体替换同 id 的已存在任务。参考 APScheduler `replace_existing`。

```php
public function replaceExisting(bool $on = true): self
```

```php
TaskBuilder::shell('job')
    ->withId('nightly-backup')
    ->replaceExisting(true)
    ->cron('0 2 * * *');
```

### tag

追加单个标签。

```php
public function tag(string $tag): self
```

```php
TaskBuilder::shell('job')->tag('billing')->tag('urgent');
```

### tags

设置标签数组（覆盖原有标签）。

```php
public function tags(array $tags): self
```

```php
TaskBuilder::shell('job')->tags(['billing', 'urgent', 'nightly']);
```

### withMeta

附加用户元数据（任意 JSON 字符串或 null）。

```php
public function withMeta(?string $json): self
```

```php
TaskBuilder::shell('job')->withMeta(json_encode(['owner' => 'team-a', 'ticket' => 'JIRA-123']));
TaskBuilder::shell('job')->withMeta(null); // 清除 meta
```

### withTimezone

设置时区标识符（如 `Asia/Shanghai`），cron 在此时区下计算 `next_fire`。

```php
public function withTimezone(string $tz): self
```

```php
TaskBuilder::shell('job')->cron('0 9 * * *')->withTimezone('America/New_York');
```

### idempotent

声明 HTTP 任务为幂等（即使 POST/PUT/DELETE/PATCH 也允许重试）。默认 false 时非幂等方法在 5xx 不重试以防重复副作用；GET/HEAD/OPTIONS 始终可重试。参考 RFC 7231 §4.2.1-2。

```php
public function idempotent(bool $on = true): self
```

```php
TaskBuilder::http('POST', 'https://a.test/idempotent-endpoint')
    ->withRetry(3, 2)->idempotent(true);
```

---

## 七、HTTP 专属方法

以下方法仅适用于 `http` 类型任务，对 `shell` 任务调用会抛 `Xhjob\Exception\InvalidTaskConfigException`。

### withHeaders

设置 http 请求头（覆盖之前设置）。

```php
public function withHeaders(array $h): self
```

{: .note }
空数组会被转成 `(object)[]`，使 `json_encode` 产生 `{}` 而非 `[]`，匹配 Rust `HttpPayload.headers: HashMap<String,String>` 的反序列化要求。

```php
TaskBuilder::http('GET', 'https://a.test')
    ->withHeaders(['Authorization' => 'Bearer xxx', 'Accept' => 'application/json']);

// 显式清空
TaskBuilder::http('GET', 'https://a.test')->withHeaders([]);
```

### withBody

设置 http 请求体。

```php
public function withBody(string $b): self
```

```php
TaskBuilder::http('POST', 'https://a.test')
    ->withBody(json_encode(['name' => 'alice', 'age' => 30]));
```

### withProxy

设置 http 代理。

```php
public function withProxy(string $p): self
```

{: .warning }
**P0-16 fix**：proxy 必须写入 **顶层 `proxy` 字段**，而非 `payload.proxy`。Rust 的 `Task.proxy` 从顶层 JSON key 读取；写到 `payload.proxy` 会导致代理设置被静默丢弃。本方法已正确处理。

```php
TaskBuilder::http('GET', 'https://a.test')
    ->withProxy('socks5h://user:pass@127.0.0.1:1080');

TaskBuilder::http('GET', 'https://a.test')
    ->withProxy('http://10.0.0.1:8080');
```

---

## 八、Shell 专属方法

以下方法仅适用于 `shell` 类型任务，对 `http` 任务调用会抛 `Xhjob\Exception\InvalidTaskConfigException`。

### withEncoding

设置 shell 任务输出编码。对应 Rust `Task.encoding` 字段，stdout/stderr 字节流按此编码解码为 UTF-8。支持 `encoding_rs` 接受的所有标签（GBK / Big5 / Shift_JIS / auto 等）。P0-18 fix 补齐了此方法。

```php
public function withEncoding(string $encoding): self
```

```php
TaskBuilder::shell('chcp 936 && echo 你好')->withEncoding('GBK');
TaskBuilder::shell('echo 你好')->withEncoding('Big5');
TaskBuilder::shell('echo こんにちは')->withEncoding('Shift_JIS');
TaskBuilder::shell('echo hi')->withEncoding('auto');
```

### withStdin

设置 shell 任务的标准输入。对应 Rust `ShellPayload.stdin` 字段，内容通过管道写入子进程 stdin；未设置时 stdin 为 `/dev/null`。

```php
public function withStdin(?string $stdin): self
```

```php
TaskBuilder::shell('wc -l')->withStdin("line1\nline2\nline3\n");
TaskBuilder::shell('cat')->withStdin(null); // 不注入 stdin
```

### withWorkingDir

设置 shell 任务的工作目录。对应 Rust `ShellPayload.working_dir` 字段，子进程执行前 chdir 到该目录；未设置时继承 daemon 当前目录。

```php
public function withWorkingDir(?string $dir): self
```

```php
TaskBuilder::shell('ls -la')->withWorkingDir('/var/log');
TaskBuilder::shell('pwd')->withWorkingDir(null); // 不设置
```

### withEnv

设置 shell 任务的环境变量。对应 Rust `ShellPayload.env` 字段，键值对在默认 PATH/HOME/XHJOB_OWNER 之后注入，用户变量优先级更高（可覆盖默认 PATH）。

```php
public function withEnv(array $env): self
```

```php
TaskBuilder::shell('echo $FOO')
    ->withEnv(['FOO' => 'bar', 'LANG' => 'en_US.UTF-8']);

// 空数组转成 {} 避免 serde 反序列化失败
TaskBuilder::shell('env')->withEnv([]);
```

---

## 九、终端方法

### dispatch

派发任务到 daemon。根据构建器类型分别调用：

- 普通 builder → `xhjob_dispatch`
- chain builder → `xhjob_chain`
- group builder → `xhjob_group`
- chord builder → `xhjob_chord`

```php
public function dispatch(?string $service = null, ?string $dataDir = null): string
```

| 参数 | 类型 | 默认 | 说明 |
| --- | --- | --- | --- |
| `$service` | `?string` | `null` | 服务名，null 用默认 |
| `$dataDir` | `?string` | `null` | 数据目录，null 用默认 |

**返回值**：`string`。成功返回 `task_id` / `chain_id` / `group_id` / `chord_id`。

**异常**：当 daemon 返回 `error: ...` 时抛 `Xhjob\Exception\InvalidTaskConfigException`。

```php
// 普通任务
$id = TaskBuilder::shell('echo hi')
    ->cron('0 * * * *')
    ->withRetry(3, 2)
    ->dispatch('default', '/var/lib/xhjob');

// 链任务自动走 xhjob_chain
$chainId = TaskBuilder::chain([
    TaskBuilder::shell('echo a'),
    TaskBuilder::shell('grep a'),
])->dispatch();
```

### toArray

导出为关联数组。

```php
public function toArray(): array
```

```php
$config = TaskBuilder::shell('echo hi')->cron('0 * * * *')->toArray();
print_r($config);
```

### toJson

导出为 JSON 字符串。

```php
public function toJson(): string
```

**异常**：当配置无法编码为 JSON 时抛 `Xhjob\Exception\InvalidTaskConfigException`。

```php
$json = TaskBuilder::shell('echo hi')->cron('0 * * * *')->toJson();
// 可直接喂给 xhjob_dispatch
$taskId = xhjob_dispatch($json);
```

---

## 完整示例

### Cron Shell 任务

```php
use Xhjob\TaskBuilder;

$taskId = TaskBuilder::shell('/usr/local/bin/backup.sh')
    ->withEncoding('UTF-8')
    ->withRetry(3, 10)
    ->retryBackoff(true)
    ->timeout(1800)
    ->softTimeout(1700)
    ->cron('0 2 * * *')
    ->withTimezone('Asia/Shanghai')
    ->persist(true)
    ->acksLate(true)
    ->resultTtl(86400)
    ->withId('daily-backup')
    ->replaceExisting(true)
    ->tag('backup')
    ->tag('critical')
    ->withMeta(json_encode(['owner' => 'ops']))
    ->misfireGraceTime(300)
    ->dispatch();
```

### HTTP 任务（带代理 + 幂等重试）

```php
$taskId = TaskBuilder::http('POST', 'https://api.test/charge')
    ->withHeaders([
        'Authorization' => 'Bearer xxx',
        'Content-Type'  => 'application/json',
    ])
    ->withBody(json_encode(['amount' => 100, 'user' => 'u1']))
    ->withProxy('http://10.0.0.1:8080')
    ->withRetry(3, 2)
    ->idempotent(true)       // 声明幂等，允许 POST 在 5xx 重试
    ->timeout(30)
    ->dispatch();
```

### 任务链

```php
$chainId = TaskBuilder::chain([
    TaskBuilder::shell('curl -s https://api.test/data'),
    TaskBuilder::shell('jq ".items"'),
    TaskBuilder::shell('gzip > /tmp/data.gz'),
])->dispatch();
```

### Chord（并行 + 回调聚合）

```php
$chordId = TaskBuilder::chord(
    [
        TaskBuilder::http('GET', 'https://a.test/1'),
        TaskBuilder::http('GET', 'https://a.test/2'),
        TaskBuilder::http('GET', 'https://a.test/3'),
    ],
    TaskBuilder::shell('cat > /tmp/aggregated.json')  // callback.meta 携带 header 结果
)->dispatch();
```
