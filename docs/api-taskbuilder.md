# TaskBuilder API

`Xhjob\TaskBuilder` 是 ThinkPHP 8 扩展包提供的 PHP 端 Fluent Builder，用于构建 `xhjob_dispatch` / `xhjob_chain` / `xhjob_group` / `xhjob_chord` 所需的任务配置 JSON。

> 源码：`releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php`，命名空间 `Xhjob`。

## 设计要点

- **构造方法 `protected`**：必须通过静态工厂 `shell()` / `http()` / `chain()` / `group()` / `chord()` / `fromJson()` 实例化。
- **链式方法全部返回 `self`**：可在一条链上连续配置。
- **终结方法 3 个**：`dispatch()` 派发到 daemon、`toArray()` 导出数组、`toJson()` 导出 JSON。
- **HTTP / Shell 专属方法**：在错误任务类型上调用抛 `Xhjob\Exception\InvalidTaskConfigException`。
- **默认配置**：`timeout=30`、`retry_max=0`、`max_instances=1`、`coalesce=true`、`acks_on_failure=true`；Rust 侧 `Option<u64>` / `Option<i64>` 字段（`interval` / `run_at` / `start_date` / `end_date` / `soft_timeout`）默认输出 `null`，避免被反序列化为 `Some(0)` 导致 interval 任务误判为 DateTrigger 立即转 Success 终态。

## 错误契约

| 场景 | 行为 |
| --- | --- |
| `dispatch()` 收到 daemon 的 `error:` 前缀返回 | 抛 `InvalidTaskConfigException`，消息为 `dispatch 失败：<error 内容>` |
| `toJson()` 编码失败 | 抛 `InvalidTaskConfigException`，消息为 `TaskBuilder 配置无法编码为 JSON：<json_last_error_msg>` |
| HTTP 专属方法用于非 http 任务 | 抛 `InvalidTaskConfigException` |
| Shell 专属方法用于非 shell 任务 | 抛 `InvalidTaskConfigException` |
| 链式 setter 方法 | 不抛异常，校验延迟到 `dispatch` |

> 底层 `xhjob_dispatch` 等 string 返回值用 `error:` 前缀表达失败；`TaskBuilder::dispatch()` 把该前缀转为异常，调用方无需手动 `str_starts_with` 判错。

---

## 静态工厂（6）

### `shell`

```php
public static function shell(string $cmd): self
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$cmd` | `string` | shell 命令 |

**返回值**：`self`。`task_type=shell`、`payload=['cmd' => $cmd]` 的 builder。

**错误契约**：不抛异常。

**注意事项**：默认 `task_type` 即为 `shell`；此工厂同时设置 payload。

**代码演示**

```php
<?php
use Xhjob\TaskBuilder;

$b = TaskBuilder::shell('echo hello');
```

**生产建议**：命令避免拼接用户输入，防 shell 注入；需动态参数时用 `escapeshellarg`。

---

### `http`

```php
public static function http(string $method, string $url): self
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$method` | `string` | HTTP 方法（会被 `strtoupper` 规范化） |
| `$url` | `string` | 请求 URL |

**返回值**：`self`。`task_type=http`，payload 含 `method` / `url` / `headers={}` / `body=null` / `proxy=null`。

**错误契约**：不抛异常。

**注意事项**：`headers` 默认为 `(object)[]`，保证 `json_encode` 输出 `{}` 而非 `[]`（Rust `HttpPayload.headers` 是 `HashMap`，`[]` 会被 serde 拒绝）。

**代码演示**

```php
<?php
$b = TaskBuilder::http('POST', 'https://api.example.com/notify');
```

**生产建议**：非幂等方法（POST/PUT/DELETE/PATCH）默认对 5xx 不重试；需重试时显式 `idempotent(true)`。

---

### `chain`

```php
public static function chain(array $builders): self
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$builders` | `array` | `TaskBuilder` 实例数组（按顺序执行） |

**返回值**：`self`。chain 模式 builder，`dispatch()` 时调用 `xhjob_chain`。

**错误契约**：不抛异常（数组元素类型校验延迟到 `dispatch`）。

**注意事项**
- chain 是顺序管道：每个子任务 stdout 作为下一个子任务的 stdin。
- 内部用 `array_values($builders)` 重建索引。

**代码演示**

```php
<?php
$b = TaskBuilder::chain([
    TaskBuilder::shell('echo hello-world'),
    TaskBuilder::shell('grep hello'),
]);
$chainId = $b->dispatch();
```

**生产建议**：chain 步骤不宜过多（建议 ≤10），任一步失败会中断整链。

---

### `group`

```php
public static function group(array $builders): self
```

**参数**：`$builders` — `TaskBuilder` 实例数组（并行执行）。

**返回值**：`self`。group 模式 builder，`dispatch()` 时调用 `xhjob_group`。

**错误契约**：不抛异常。

**注意事项**：并行派发所有子任务；终态 `success`（全成功）/ `partial_failed`（部分失败）/ `failed`（全失败）。

**代码演示**

```php
<?php
$b = TaskBuilder::group([
    TaskBuilder::http('GET', 'https://a'),
    TaskBuilder::http('GET', 'https://b'),
]);
```

**生产建议**：批量任务用 `rateLimit` 限流，防止瞬时打爆下游。

---

### `chord`

```php
public static function chord(array $headerBuilders, self $callback): self
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$headerBuilders` | `array` | header `TaskBuilder` 数组（并行） |
| `$callback` | `self` | 回调 `TaskBuilder`（全部 header 成功后执行） |

**返回值**：`self`。chord 模式 builder，`dispatch()` 时调用 `xhjob_chord`。

**错误契约**：不抛异常。

**注意事项**
- 参考 Celery chord：header 全部成功后才派发 callback，callback 的 `meta` 携带所有 header 结果。
- 任一 header 失败时 chord 转 `partial_failed` 终态，**不派发 callback**。

**代码演示**

```php
<?php
$b = TaskBuilder::chord(
    [TaskBuilder::shell('shard-1.sh'), TaskBuilder::shell('shard-2.sh')],
    TaskBuilder::shell('merge.sh')
);
```

**生产建议**：Map-Reduce 场景的理想原语。

---

### `fromJson`

```php
public static function fromJson(string $json): self
```

**参数**：`$json` — TaskBuilder JSON 字符串。

**返回值**：`self`。反序列化后的 builder，`config` 为 `json_decode($json, true)` 结果（非法 JSON 时 `config=[]`）。

**错误契约**：不抛异常（非法 JSON 退化为空配置）。

**注意事项**：反序列化后是普通 builder（非 chain/group/chord 模式）；如需还原编排模式需自行重建。

**代码演示**

```php
<?php
$json = file_get_contents('/tmp/task-snapshot.json');
$b = TaskBuilder::fromJson($json)->replaceExisting(true);
$id = $b->dispatch();
```

**生产建议**：用于任务配置迁移 / 快照恢复；恢复前确认 JSON 完整性。

---

## 触发器（8）

| 方法 | 签名 | 写入字段 | 说明 |
| --- | --- | --- | --- |
| `cron` | `cron(string $expr): self` | `cron` | 5 字段 cron 表达式 |
| `every` | `every(int $secs): self` | `interval` | 固定间隔秒 |
| `runAt` | `runAt(int $ts): self` | `run_at` | 一次性绝对时间戳 |
| `countdown` | `countdown(int $secs): self` | `countdown` | 倒计时；等价 `runAt(time()+secs)`；与 `runAt` 同时设时 `runAt` 优先 |
| `startAt` | `startAt(int $ts): self` | `start_date` | 起始时间戳 |
| `endAt` | `endAt(int $ts): self` | `end_date` | 结束时间戳 |
| `jitter` | `jitter(int $secs): self` | `jitter` | 调度抖动 |
| `withTimezone` | `withTimezone(string $tz): self` | `timezone` | 时区标识符 |

**参数**：各方法单参数（详见签名表）。

**返回值**：均为 `self`。

**错误契约**：均不抛异常；cron / 时区 / 时间戳的合法性校验延迟到 daemon 派发时。

**注意事项**
- `countdown` 与 `runAt` 同时设置时，daemon 侧 `runAt` 优先（`countdown` 仅在 `run_at` 为空时由 `time()+countdown` 推导）。
- `every` 写入 `interval` 字段；与 `cron` / `runAt` 同时设置时后者优先级更高。

**代码演示**

```php
<?php
use Xhjob\TaskBuilder;

// 工作日凌晨 2 点备份，上海时区
$b = TaskBuilder::shell('backup.sh')
    ->cron('0 2 * * 1-5')
    ->withTimezone('Asia/Shanghai')
    ->jitter(30);

// 5 分钟后执行一次
$b2 = TaskBuilder::shell('send-reminder.sh')
    ->countdown(300);
```

**生产建议**：跨时区任务务必显式 `withTimezone`，勿依赖系统时区；高并发定时任务加 `jitter` 避免惊群。

---

## 重试 / 超时（6）

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `withRetry` | `withRetry(int $max, int $delay = 1): self` | 最大重试次数与间隔秒 |
| `maxExecutions` | `maxExecutions(int $n): self` | 最大执行次数，`0=无限` |
| `retryBackoff` | `retryBackoff(bool $on = true): self` | 指数退避 `min(delay*2^attempts, delay*60)` |
| `timeout` | `timeout(int $secs): self` | 硬超时，到期 SIGKILL |
| `softTimeout` | `softTimeout(int $secs): self` | 软超时，写入 `soft_timeout`，SIGTERM→SIGKILL 升级链 |
| `misfireGraceTime` | `misfireGraceTime(int $secs): self` | 误触发宽限秒 |

**参数**：详见签名表。

**返回值**：均为 `self`。

**错误契约**：均不抛异常。

**注意事项**
- `retryBackoff` 启用后重试延迟按 `min(delay * 2^attempts, delay * 60)` 指数增长（封顶 60 倍）。
- `softTimeout` 必须严格小于 `timeout` 才生效；shell 执行器在 `soft_timeout` 秒发 SIGTERM，未退出再在 `timeout` 秒发 SIGKILL。HTTP 任务忽略 `soft_timeout`。
- `maxExecutions=0` 表示无限执行（适合常驻 cron 任务）。

**代码演示**

```php
<?php
$b = TaskBuilder::shell('long-etl.sh')
    ->withRetry(3, 5)
    ->retryBackoff(true)
    ->softTimeout(60)
    ->timeout(90)
    ->maxExecutions(0);
```

**生产建议**：长任务务必设 `softTimeout < timeout`，给业务优雅退出窗口；易失败任务开 `retryBackoff` 避免雪崩重试。

---

## 并发（4）

| 方法 | 签名 | 说明 |
| --- | --- | --- |
| `priority` | `priority(int $p): self` | 优先级，数值越大越优先 |
| `maxInstances` | `maxInstances(int $n): self` | 最大并发实例数 |
| `allowOverlap` | `allowOverlap(bool $on = true): self` | 允许重叠执行 |
| `rateLimit` | `rateLimit(int $count, int $window): self` | 滑动窗口限流，写入 `rate_limit_count` / `rate_limit_window` |

**参数**：详见签名表。

**返回值**：均为 `self`。

**错误契约**：均不抛异常。

**注意事项**
- `maxInstances=1`（默认）+ `allowOverlap(false)` 实现串行化，避免任务重叠。
- `rateLimit` 写两个字段：`rate_limit_count` 与 `rate_limit_window`；`count=0` 关闭限流。

**代码演示**

```php
<?php
$b = TaskBuilder::http('GET', 'https://third-party/api')
    ->cron('*/1 * * * *')
    ->rateLimit(10, 60)       // 60 秒内最多 10 次
    ->maxInstances(1);
```

**生产建议**：调用第三方 API 必设 `rateLimit`，防被限流封禁；串行关键任务用 `maxInstances(1)`。

---

## 可靠性（8）

| 方法 | 签名 | 写入字段 | 说明 |
| --- | --- | --- | --- |
| `coalesce` | `coalesce(bool $on = true): self` | `coalesce` | 合并漏触发为一次执行 |
| `persist` | `persist(bool $on = true): self` | `persist` | 持久化任务定义 |
| `acksLate` | `acksLate(bool $on = true): self` | `acks_late` | 延迟确认；daemon 重启时重置 Running→Pending |
| `acksOnFailure` | `acksOnFailure(bool $on = true): self` | `acks_on_failure` | `false`=失败不 ack，无限重试至成功/取消 |
| `idempotent` | `idempotent(bool $on = true): self` | `idempotent` | 声明 HTTP 任务幂等，非幂等方法也对 5xx 重试 |
| `ignoreResult` | `ignoreResult(bool $on = true): self` | `ignore_result` | fire-and-forget，不存结果 |
| `resultTtl` | `resultTtl(int $secs): self` | `result_ttl` | 结果保留秒数，`0=永久` |
| `expires` | `expires(int $secs): self` | `expires` | Pending 超 `secs` 秒转 `expired` 终态；`0=不过期` |

**参数**：详见签名表。

**返回值**：均为 `self`。

**错误契约**：均不抛异常。

**注意事项**
- `idempotent(true)` 声明后，即使 POST/PUT/DELETE/PATCH 也会对 5xx 重试；GET/HEAD/OPTIONS 是安全方法，无论此标志如何都重试。`false`（默认）时非幂等方法不重试，防止重复副作用（重复扣款等）。
- `ignoreResult(true)` 与 `resultTtl(>0)` 同时设置时，`ignoreResult` 优先（daemon 跳过 `save_result`，`xhjob_result` 返回无结果）。
- `acksLate(true)` 配合 `persist(true)` 实现崩溃恢复：daemon SIGKILL 重启后 Running 任务重置为 Pending 重新派发。

**代码演示**

```php
<?php
// 幂等的扣款任务：至少一次执行
$b = TaskBuilder::http('POST', 'https://api/charge')
    ->idempotent(true)
    ->acksLate(true)
    ->persist(true)
    ->withRetry(5, 10)
    ->idempotent(true);

// 高吞吐 fire-and-forget 任务
$b2 = TaskBuilder::shell('log-event.sh')
    ->ignoreResult(true)
    ->cron('*/1 * * * *');
```

**生产建议**：幂等副作用任务用 `idempotent(true)` + `acksLate(true)`；非幂等任务保持默认不重试以避免重复副作用。

---

## 元数据 / 身份（5）

### `withMeta`

```php
public function withMeta(?string $json): self
```

**参数**：`$json` — 任意 JSON 字符串元数据（可为 `null` 清除）。

**返回值**：`self`，写入 `meta` 字段。

**错误契约**：不抛异常。

**注意事项**：`meta` 是字符串字段，调用方需自行 `json_encode`；daemon 侧原样存储，可通过 `xhjob_state` 的 `meta` 字段读回。

**代码演示**：`->withMeta(json_encode(['order_id' => 42, 'user_id' => 7]))->...`

**生产建议**：携带业务上下文，便于排障时关联业务记录。

---

### `tags`

```php
public function tags(array $tags): self
```

**参数**：`$tags` — 标签数组（用 `array_values` 重建索引）。

**返回值**：`self`，整体覆盖 `tags` 字段。

**错误契约**：不抛异常。

**注意事项**：整体覆盖，非追加；如需追加单标签用 `tag()`。

**代码演示**：`->tags(['billing', 'nightly'])->...`

---

### `tag`

```php
public function tag(string $tag): self
```

**参数**：`$tag` — 单个标签名。

**返回值**：`self`，追加到 `tags` 数组。

**错误契约**：不抛异常；非数组 `tags` 会被重置为空数组再追加。

**注意事项**：可链式多次调用追加多个标签。

**代码演示**：`->tag('billing')->tag('nightly')->...`

**生产建议**：用标签做任务分类与批量查询（`xhjob_list` 支持按 tag 过滤）。

---

### `withId`

```php
public function withId(string $id): self
```

**参数**：`$id` — 任务 ID，**须匹配 `^[A-Za-z0-9_-]{1,64}$`**。

**返回值**：`self`，写入 `id` 字段。

**错误契约**：不抛异常（PHP 端不做格式校验，校验在 Rust daemon 侧进行：非法 id 会回退为自动生成 UUID 并 `tracing::warn!`）。

**注意事项**
- `TaskBuilder::withId()` 是真实方法名（与 Rust `Xhjob` 类的 `id()` 不同——后者因 ext-php-rs 不转换单词名而暴露为 `id`）。
- 配合 `replaceExisting(true)` 实现幂等派发：同 id 任务被全量覆盖。

**代码演示**

```php
<?php
$b = TaskBuilder::shell('cleanup.sh')
    ->withId('nightly-cleanup')
    ->replaceExisting(true)
    ->cron('0 3 * * *');
```

**生产建议**：业务幂等任务用稳定业务 id（如 `order-{$id}-timeout`）+ `replaceExisting(true)`，避免重复派发。

---

### `replaceExisting`

```php
public function replaceExisting(bool $on = true): self
```

**参数**：`$on`。

**返回值**：`self`，写入 `replace_existing` 字段。

**错误契约**：不抛异常。

**注意事项**：`true` 且已设 `id` 时，dispatch 覆盖同 id 任务（全量覆写）；未设 `id` 时本项无效。

**代码演示**：见 `withId`。

**生产建议**：定时任务模板用 `withId` + `replaceExisting(true)`，重复部署只更新不重复创建。

---

## HTTP 专属（3）

> 仅适用于 `http()` 创建的任务；在 shell 任务上调用抛 `InvalidTaskConfigException`。

### `withHeaders`

```php
public function withHeaders(array $h): self
```

**参数**：`$h` — 键值对请求头。

**返回值**：`self`，写入 `payload.headers`。

**错误契约**：非 http 任务抛 `InvalidTaskConfigException('withHeaders 仅适用于 http 任务')`。

**注意事项**：**空数组会转为 `(object)[]`**，确保 `json_encode` 输出 `{}`（匹配 Rust `HttpPayload.headers: HashMap<String,String>`），而非 `[]`（serde 会拒绝）。

**代码演示**

```php
<?php
$b = TaskBuilder::http('GET', 'https://api/x')
    ->withHeaders(['Authorization' => 'Bearer ' . $token, 'Accept' => 'application/json']);
```

**生产建议**：token 从环境变量取，勿硬编码；空头也安全（自动转 `{}`）。

---

### `withBody`

```php
public function withBody(string $b): self
```

**参数**：`$b` — 请求体字符串。

**返回值**：`self`，写入 `payload.body`。

**错误契约**：非 http 任务抛 `InvalidTaskConfigException`。

**注意事项**：JSON 请求体需自行 `json_encode`。

**代码演示**：`->withBody(json_encode(['event' => 'login', 'uid' => 7]))->...`

**生产建议**：大 body 注意设 `timeout`。

---

### `withProxy`

```php
public function withProxy(string $p): self
```

**参数**：`$p` — 代理地址。

**返回值**：`self`，写入**顶层 `proxy` 字段**（非 `payload.proxy`）。

**错误契约**：非 http 任务抛 `InvalidTaskConfigException`。

**注意事项**：> ⚠️ **写到顶层 `proxy` 字段，不是 `payload.proxy`**。Rust `Task.proxy` 从顶层 JSON key 读取；写到 `payload.proxy` 会被静默丢弃。这是已修复的 P0-16 bug。

**代码演示**：`->withProxy('socks5://user:pass@127.0.0.1:1080')->...`

**生产建议**：出海请求 / 内网爬取走代理；凭证从密钥管理取。

---

## Shell 专属（4）

> 仅适用于 `shell()` 创建的任务；在 http 任务上调用抛 `InvalidTaskConfigException`。

### `withEncoding`

```php
public function withEncoding(string $encoding): self
```

**参数**：`$encoding` — 编码标签（`GBK` / `Big5` / `Shift_JIS` / `auto` 等）。

**返回值**：`self`，写入顶层 `encoding` 字段（对应 Rust `Task.encoding`）。

**错误契约**：非 shell 任务抛 `InvalidTaskConfigException('withEncoding 仅适用于 shell 任务')`。

**注意事项**：stdout/stderr 字节流按此编码解码为 UTF-8；未设置时默认 UTF-8。

**代码演示**

```php
<?php
$b = TaskBuilder::shell('chcp 936 && dir')
    ->withEncoding('GBK');
```

**生产建议**：Windows / 中文环境 shell 任务务必设置，避免乱码导致 stderr 不可读。

---

### `withStdin`

```php
public function withStdin(?string $stdin): self
```

**参数**：`$stdin` — 标准输入内容（`null` 表示不注入）。

**返回值**：`self`，写入 `payload.stdin`（对应 Rust `ShellPayload.stdin`）。

**错误契约**：非 shell 任务抛 `InvalidTaskConfigException`。

**注意事项**：内容通过管道写入子进程 stdin；未设置时 stdin 为 `/dev/null`。

**代码演示**：`->withStdin("line1\nline2\n")->...`

**生产建议**：批量数据通过 stdin 传递比命令行参数更安全（避免参数溢出 / 注入）。

---

### `withWorkingDir`

```php
public function withWorkingDir(?string $dir): self
```

**参数**：`$dir` — 工作目录路径（`null` 表示不设置）。

**返回值**：`self`，写入 `payload.working_dir`。

**错误契约**：非 shell 任务抛 `InvalidTaskConfigException`。

**注意事项**：子进程执行前 `chdir` 到该目录；未设置时继承 daemon 当前目录。

**代码演示**：`->withWorkingDir('/var/app')->...`

**生产建议**：脚本依赖相对路径时务必设置工作目录。

---

### `withEnv`

```php
public function withEnv(array $env): self
```

**参数**：`$env` — 环境变量键值对，如 `['FOO' => 'bar']`。

**返回值**：`self`，写入 `payload.env`。

**错误契约**：非 shell 任务抛 `InvalidTaskConfigException`。

**注意事项**
- **空数组会转为 `(object)[]`**，确保 `json_encode` 输出 `{}`（匹配 Rust `ShellPayload.env` 结构）。
- 用户变量在默认 `PATH` / `HOME` / `XHJOB_OWNER` 之后注入，**优先级更高**（可覆盖默认 PATH）。

**代码演示**

```php
<?php
$b = TaskBuilder::shell('build.sh')
    ->withEnv(['NODE_ENV' => 'production', 'PATH' => '/opt/bin:' . getenv('PATH')]);
```

**生产建议**：敏感凭证通过 `withEnv` 注入而非命令行，避免进程列表泄露。

---

## 终结方法（3）

### `dispatch`

```php
public function dispatch(?string $service = null, ?string $dataDir = null): string
```

**参数**

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `$service` | `?string` | 服务名（`null` = 默认） |
| `$dataDir` | `?string` | 数据目录（`null` = 默认） |

**返回值**：`string`。成功为 task_id / chain_id / group_id / chord_id（按 builder 模式不同）。

**错误契约**：底层返回 `error:` 前缀时抛 `InvalidTaskConfigException('dispatch 失败：<error 内容>')`。调用方无需手动判 `error:` 前缀。

**注意事项**
- 按构建器类型分别调用：普通 builder → `xhjob_dispatch`；chain → `xhjob_chain`；group → `xhjob_group`；chord → `xhjob_chord`。
- chord 的 callback 通过 `toJson()` 序列化（若 callback 为 null 则用 `'{}'`）。

**代码演示**

```php
<?php
use Xhjob\TaskBuilder;
use Xhjob\Exception\InvalidTaskConfigException;

try {
    $id = TaskBuilder::shell('echo hi')
        ->cron('0 * * * *')
        ->withRetry(3, 2)
        ->dispatch('cron-svc', '/var/lib/xhjob');
    echo "dispatched: $id\n";
} catch (InvalidTaskConfigException $e) {
    error_log('派发失败：' . $e->getMessage());
}
```

**生产建议**：用 try/catch 收敛异常；`$service` / `$dataDir` 在多服务部署时显式传入。

---

### `toArray`

```php
public function toArray(): array
```

**参数**：无。

**返回值**：`array`。导出当前 `config` 关联数组（含全部已配置字段与默认值）。

**错误契约**：不抛异常。

**注意事项**：导出的是构建器内部配置快照；chain/group/chord 模式下子 builder 不会自动展开（需在 `dispatch` 时才 `toArray` 各子 builder 拼成 JSON 数组）。

**代码演示**

```php
<?php
$arr = TaskBuilder::shell('echo hi')->cron('0 * * * *')->toArray();
print_r($arr);
```

**生产建议**：用于任务配置审计 / 单元测试断言；不要用它派发（派发用 `dispatch`）。

---

### `toJson`

```php
public function toJson(): string
```

**参数**：无。

**返回值**：`string`。`json_encode($this->config)` 结果。

**错误契约**：编码失败抛 `InvalidTaskConfigException('TaskBuilder 配置无法编码为 JSON：<json_last_error_msg>')`。

**注意事项**：`dispatch()` 内部即调用 `toJson()` 生成派发 JSON；空 headers / 空 env 已通过 `(object)[]` 保证输出 `{}`。

**代码演示**

```php
<?php
$json = TaskBuilder::shell('echo hi')->cron('0 * * * *')->toJson();
// 可直接传给 xhjob_dispatch($json)
```

**生产建议**：自定义派发流程（绕过 `dispatch`）时用 `toJson()` 生成标准 JSON 传给 `xhjob_dispatch`。

---

## 完整示例

```php
<?php
use Xhjob\TaskBuilder;
use Xhjob\Exception\InvalidTaskConfigException;

// 1. 复杂 shell 定时任务
try {
    $id = TaskBuilder::shell('etl.sh --delta')
        ->withId('etl-delta-nightly')
        ->replaceExisting(true)
        ->cron('0 2 * * *')
        ->withTimezone('Asia/Shanghai')
        ->withRetry(3, 60)
        ->retryBackoff(true)
        ->softTimeout(1800)->timeout(2000)
        ->maxInstances(1)
        ->persist(true)
        ->withWorkingDir('/opt/etl')
        ->withEnv(['DB_HOST' => getenv('DB_HOST')])
        ->withEncoding('UTF-8')
        ->tags(['etl', 'nightly'])
        ->withMeta(json_encode(['owner' => 'data-team']))
        ->dispatch();
} catch (InvalidTaskConfigException $e) {
    throw new RuntimeException('ETL 任务派发失败：' . $e->getMessage());
}

// 2. HTTP 幂等回调任务
$cb = TaskBuilder::http('POST', 'https://api/callback')
    ->withHeaders(['X-Signature' => $sig])
    ->withBody(json_encode(['task_id' => $id]))
    ->idempotent(true)
    ->withRetry(5, 10);

// 3. chord：并行计算 + 汇总
$chordId = TaskBuilder::chord(
    [
        TaskBuilder::shell('compute-shard-1.sh'),
        TaskBuilder::shell('compute-shard-2.sh'),
        TaskBuilder::shell('compute-shard-3.sh'),
    ],
    TaskBuilder::shell('merge-results.sh')
)->dispatch();
```
