# xhjob

XHJob 是一个基于 Rust（ext-php-rs 0.15）开发的高性能 PHP 异步任务调度扩展，采用 master-worker 多进程池架构（参考 PHP-FPM），提供 真正的 PHP handler 并发执行、链式 API、cron 定时任务，以及跨平台独立 daemon 守护进程模式，无需 C 桥接层，也无需任何外部依赖（supervisor / crontab / Swoole）。

## 特性

- 跨平台 daemon（Unix `double-fork` + Windows `CreateProcessW`），PHP 退出后任务继续运行
- 真 Rust 多线程池 + tokio 多协程池，HTTP 任务异步非阻塞
- 链式 API：`Xhjob::task()->viaHttp(...)->withRetry(...)->cron(...)->dispatch()`
- Cron 调度器（5/6 段表达式），支持重叠控制、coalesce、misfire_grace_time
- HTTP 与 Shell 双执行器，支持超时与重试
- 可选 SQLite 持久化（`persist` cargo feature），daemon 重启后自动恢复任务
- **多服务实例**（Task 21）：同一主机可启动多个独立 daemon
- **HTTP / SOCKS5 代理**（Task 22）：`withProxy('socks5://user:pass@host:port')`
- **Shell 输出编码转换**（Task 23）：`withEncoding('GBK')`，`auto` 模式自动检测
- **Cron 自定义时区**（Task 24）：`withTimezone('Asia/Shanghai')`
- **自定义数据目录**（Task 26）：`xhjob_start('svc', '/var/lib/xhjob')` 统一指定 pid/sock/db/log 落盘位置，便于备份/迁移/恢复

## 安装

### 依赖

- Rust 工具链（推荐 stable，需支持 ext-php-rs 0.15）
- PHP 8.x 开发包（`php-config` 在 PATH 中）
- 编译时需要 `libclang`（bindgen 依赖）

### 编译

```bash
# 默认编译（仅内存存储）
cargo build --release

# 启用 SQLite 持久化
cargo build --release --features persist
```

产物：

- Unix：`target/release/libxhjob.so`
- Windows：`target/release/xhjob.dll`

### 加载扩展

在 `php.ini` 中添加：

```ini
extension=/path/to/libxhjob.so
```

或运行时通过 `-d extension=target/release/libxhjob.so` 加载。

## 基础用法

```php
<?php
// 1. 启动 daemon（默认服务名为 "default"）
xhjob_start();

// 2. 通过链式 API dispatch 一个 HTTP 任务
$id = Xhjob::task()
    ->viaHttp('POST', 'https://httpbin.org/post')
    ->withHeaders(['X-Foo' => 'bar'])
    ->withBody(json_encode(['k' => 'v']))
    ->withRetry(3, 2)
    ->timeout(30)
    ->dispatch();
echo "task id: {$id}\n";

// 3. 查询状态与结果
$state  = xhjob_state($id);
$result = xhjob_result($id);
var_dump($state, $result);

// 4. 关闭 daemon
xhjob_stop();
```

## 多服务实例

XHJob 支持在同一主机上启动多个独立的 daemon 实例，每个实例由服务名标识，拥有独立的 PID 文件、IPC socket、SQLite 数据库与日志文件。任务调度彼此隔离，互不干扰。

### 服务名规则

服务名必须匹配正则 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`：

- 必须以英文字母开头
- 后续字符可以是英文字母、数字、下划线、连字符
- 最长 32 个字符
- 不传服务名时使用默认值 `"default"`

### 路径推导

每个服务名 `name` 对应的运行时路径：

| 资源 | Unix | Windows |
|------|------|---------|
| PID 文件 | `${XHJOB_PID_DIR:-/tmp}/xhjob.{name}.pid` | `%TEMP%\xhjob.{name}.pid` |
| IPC socket | `${XHJOB_SOCK_DIR:-/tmp}/xhjob.{name}.sock` | `\\.\pipe\xhjob-{name}` |
| SQLite DB | `${XHJOB_DB_DIR:-/tmp}/xhjob.{name}.db` | `%TEMP%\xhjob.{name}.db` |
| 日志文件 | `${XHJOB_LOG_DIR:-/tmp}/xhjob.{name}.log` | `%TEMP%\xhjob.{name}.log` |

各资源目录可单独通过环境变量 `XHJOB_PID_DIR` / `XHJOB_SOCK_DIR` / `XHJOB_DB_DIR` / `XHJOB_LOG_DIR` 自定义，也可通过统一的 `XHJOB_DATA_DIR` 或函数参数 `data_dir` 一次性指定（详见下一节"自定义数据目录"）。

目录解析优先级（高 → 低）：

1. PHP 函数参数 `data_dir`（`xhjob_start($name, $data_dir)` 等）
2. 细粒度环境变量（`XHJOB_PID_DIR` / `XHJOB_SOCK_DIR` / `XHJOB_DB_DIR` / `XHJOB_LOG_DIR`）
3. 统一环境变量 `XHJOB_DATA_DIR`
4. 平台默认（Unix：`/tmp`；Windows：`%TEMP%`）

> Windows IPC 使用 Named Pipe（`\\.\pipe\xhjob-{name}`），不占用文件系统路径，因此 `data_dir` 对 Windows IPC 无影响；但 PID/DB/Log 仍受 `data_dir` 控制。

## 自定义数据目录

通过可选的 `data_dir` 参数可一次性指定一个服务所有运行时文件（PID / sock / SQLite DB / 日志）的存放目录。常见用途：

- **数据迁移**：将整个目录复制到新机器即可恢复服务
- **备份/恢复**：定时备份该目录即可保留全部任务历史
- **隔离运行**：测试环境与生产环境使用不同目录互不干扰
- **权限控制**：将目录放在受控路径下（如 `/var/lib/xhjob`）

### 函数 API

所有顶层函数均新增可选的 `data_dir` 参数：

```php
xhjob_start(string $name = "default", string $data_dir = null): bool
xhjob_stop(string $name = "default", string $data_dir = null): bool
xhjob_restart(string $name = "default", string $data_dir = null): bool
xhjob_status(string $name = "default", string $data_dir = null): array
xhjob_dispatch(string $task_json, string $name = "default", string $data_dir = null): string
xhjob_state(string $id, string $name = "default", string $data_dir = null): array
xhjob_result(string $id, string $name = "default", string $data_dir = null): array
```

`$data_dir` 为 `null` 或空字符串时回退到环境变量和平台默认。指定的目录若不存在，daemon 启动 / IPC bind 时会自动 `mkdir -p` 创建。

### 链式 API

```php
$id = Xhjob::task()
    ->service('cron-svc')
    ->dataDir('/var/lib/xhjob')
    ->viaShell('echo hello')
    ->dispatch();
```

`Xhjob::dataDir(string $dir): $this` 在 PHP 中暴露为 `dataDir()`（snake→camel 自动转换）。

### 示例：备份/迁移

```php
<?php
// 生产环境：所有文件落在 /var/lib/xhjob
xhjob_start('cron-svc', '/var/lib/xhjob');

// ... 投递任务 ...

// 备份：直接打包 /var/lib/xhjob 即可（停服或在线备份都行）
// 恢复：将备份解压到新机器的 /var/lib/xhjob，然后启动 daemon
//       daemon 会自动读取已存在的 .db 文件，恢复活跃任务
xhjob_stop('cron-svc', '/var/lib/xhjob');
```

```bash
# 备份
tar czf xhjob-backup-$(date +%Y%m%d).tar.gz /var/lib/xhjob

# 迁移到新机器
scp xhjob-backup-*.tar.gz new-host:/tmp/
ssh new-host 'mkdir -p /var/lib/xhjob && tar xzf /tmp/xhjob-backup-*.tar.gz -C /'

# 在新机器上启动 daemon，自动恢复
ssh new-host 'php -d extension=xhjob.so -r "xhjob_start(\"cron-svc\", \"/var/lib/xhjob\");"'
```

### 链式 API（服务名绑定）

通过 `Xhjob::task()->service($name)->...` 将 builder 绑定到指定服务：

```php
<?php
xhjob_start('cron-svc');
xhjob_start('queue-svc');

// dispatch 到 cron-svc
$id1 = Xhjob::task()
    ->service('cron-svc')
    ->viaShell('echo hello-from-cron-svc')
    ->dispatch();

// dispatch 到 queue-svc
$id2 = Xhjob::task()
    ->service('queue-svc')
    ->viaHttp('GET', 'https://httpbin.org/get')
    ->dispatch();

// 查询时也需指定服务名
var_dump(xhjob_state($id1, 'cron-svc'));
var_dump(xhjob_state($id2, 'queue-svc'));

xhjob_stop('cron-svc');
xhjob_stop('queue-svc');
```

完整示例参见 `examples/multi_service.php`。

## HTTP 代理

对 HTTP 任务可通过 `withProxy(string $proxy)` 指定代理，支持以下协议：

- `http://host:port`
- `https://host:port`
- `socks5://host:port`
- `socks5h://host:port`（DNS 由代理解析）

代理 URL 中可携带 Basic Auth：`scheme://user:pass@host:port`。

```php
<?php
xhjob_start();

// SOCKS5 代理 + Basic Auth
$id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/ip')
    ->withProxy('socks5://user:pass@127.0.0.1:1080')
    ->timeout(30)
    ->dispatch();

var_dump(xhjob_state($id));
var_dump(xhjob_result($id));

xhjob_stop();
```

> 注：`socks5h://` 表示域名解析交由代理解析，适用于本地 DNS 无法解析目标主机的场景。

完整示例参见 `examples/proxy.php`。

## Shell 编码转换

Shell 任务在 Windows 上常因控制台代码页（如 GBK / Big5）导致 stdout/stderr 输出非 UTF-8。通过 `withEncoding(string $from)` 可让 daemon 在捕获输出后自动解码为 UTF-8：

```php
<?php
// Windows 中文系统，shell 默认输出 GBK
$id = Xhjob::task()
    ->viaShell('cmd /C echo 中文测试')
    ->withEncoding('GBK')   // 将 stdout/stderr 从 GBK 解码为 UTF-8
    ->dispatch();
```

支持的编码标签包括（大小写不敏感）：`GBK`、`Big5`、`UTF-8`、`windows-1252` 等所有 `encoding_rs::Encoding::for_label` 接受的值。

### `auto` 模式

`withEncoding('auto')` 由 daemon 自动检测目标编码：

- Windows：通过 `GetOEMCP()` 查询 OEM 代码页，常见映射：
  - `936` → `GBK`
  - `950` → `Big5`
  - `932` → `Shift_JIS`
  - `1252` → `windows-1252`
- Unix：默认 `UTF-8`（无操作）

```php
<?php
// Windows 上自动检测代码页
$id = Xhjob::task()
    ->viaShell('cmd /C echo 中文')
    ->withEncoding('auto')
    ->dispatch();
```

完整示例参见 `examples/encoding.php`。

## Cron 自定义时区

Cron 表达式默认按系统本地时区（`chrono::Local`）求值。通过 `withTimezone(string $tz)` 可显式指定 IANA 时区名（如 `Asia/Shanghai`、`America/New_York`、`UTC`），`next_fire` 将按该时区计算。

cron 表达式支持 5 段或 6 段：

- 5 段：`分 时 日 月 周`（标准 Unix cron）
- 6 段：`秒 分 时 日 月 周`（含秒级精度，第一段为秒）

例如 `*/5 * * * * *` 表示每 5 秒触发；`0 */5 * * * *` 表示每 5 分钟整触发。

```php
<?php
xhjob_start();

// 每天 09:00 北京时间触发
$id = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get')
    ->cron('0 9 * * *')
    ->withTimezone('Asia/Shanghai')
    ->persist(true)
    ->dispatch();

var_dump(xhjob_state($id));

xhjob_stop();
```

- 时区字符串需为合法的 IANA 时区名，非法值会在 `dispatch()` 时立即返回错误，任务不入队
- 同一 cron 表达式在不同时区下 `next_fire` 时间不同，可用于跨地域任务调度

完整示例参见 `examples/timezone.php`。

## Cron 执行次数限制

通过 `maxExecutions(N)` 可为 cron 任务指定最大执行次数，到达上限后任务自动终止（state=SUCCESS）。

```php
<?php
xhjob_start();

// 每 1 秒触发，最多执行 3 次
$id = Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->maxExecutions(3)
    ->dispatch();

// 等待执行完成
while (true) {
    $state = xhjob_state($id);
    if ($state['state'] === 'SUCCESS') {
        echo "executed {$state['execution_count']} times\n";
        break;
    }
    usleep(500_000);
}

xhjob_stop();
```

- `maxExecutions(0)`（默认）= 无限触发直至 daemon 停止
- 持久化场景下 daemon restart 后 execution_count 保留

## 任务暂停 / 恢复 / 取消 / 删除

参考 APScheduler `pause_job` / `resume_job` / `remove_job` 与 Celery `revoke`，提供 cron 作业生命周期管理：

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->dispatch();

// 暂停（cron 不再触发，定义保留）
xhjob_pause($id);

// 恢复（下一次 tick 恢复触发）
xhjob_resume($id);

// 取消（Pending 立即 CANCELLED 终态；Running 不强制 kill，仅停止后续重试与 cron 触发）
xhjob_cancel($id);

// 删除（从 store 中删除任务定义，正在运行的实例不受影响）
xhjob_remove($id);

xhjob_stop();
```

- `xhjob_state($id)['state']` 返回 `CANCELLED` 表示任务已被取消
- `paused` 与 `cancel_requested` 字段持久化到 SQLite，重启后保留

## 任务起始 / 结束时间

参考 APScheduler `start_date` / `end_date`，可为 cron 任务指定触发时间窗口：

```php
<?php
xhjob_start();

$now = time();

// 仅在 [now+10s, now+60s] 窗口内触发
$id = Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->startAt($now + 10)
    ->endAt($now + 60)
    ->dispatch();

xhjob_stop();
```

- `start_date` 之前 cron 不触发（但 next_fire 仍按 cron 推进）
- `end_date` 之后任务 state 自动置为 SUCCESS 终态

## 任务列表查询

参考 APScheduler `get_jobs()`，可查询当前 service 中所有任务的摘要：

```php
<?php
xhjob_start();

Xhjob::task()->viaShell('echo a')->cron('*/1 * * * * *')->dispatch();
Xhjob::task()->viaShell('echo b')->cron('*/5 * * * * *')->dispatch();

// 列出全部任务
$json = xhjob_list();
$tasks = json_decode($json, true);
foreach ($tasks as $t) {
    echo "{$t['id']} state={$t['state']} cron={$t['cron']} count={$t['execution_count']}\n";
}

// 按状态过滤
$pending = json_decode(xhjob_list('default', 'PENDING'), true);

xhjob_stop();
```

每个 TaskSummary 元素字段：

- `id` / `task_type` / `state` / `cron` / `attempts` / `priority`
- `next_fire` / `paused` / `max_executions` / `execution_count`
- `start_date` / `end_date` / `meta` / `created_at` / `finished_at`

`state_filter` 可选值：`PENDING` / `RUNNING` / `INTERRUPTED` / `SUCCESS` / `FAILED` / `CANCELLED`

## Misfire 处理

参考 APScheduler `misfire_grace_time` + `coalesce`，xhjob 处理 cron 错过触发的策略：

- **coalesce=true（默认）**：无论错过多少次触发，合并为一次执行（取最近一次）
- **coalesce=false**：在 grace_time 窗口内（默认 60 秒）的错过触发仍执行一次；超过 grace_time 的错过触发直接跳过

```php
<?php
// 默认行为（coalesce=true）
Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->dispatch();  // 即使 daemon 停了 10 分钟，恢复后只触发 1 次

// 关闭 coalesce（coalesce=false）
Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->coalesce(false)
    ->dispatch();  // daemon 停了 10 分钟，恢复后若在 grace_time 内则触发 1 次，否则跳过
```

> 注：`coalesce` 通过 `Xhjob::coalesce(bool): $this` 设置。

## 任务结果过期清理

参考 Celery `result_expires`，可为任务指定结果保留时长，超时后自动清理 result 行（保留 task 行）：

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaShell('echo hello')
    ->resultTtl(5)  // 5 秒后清理 result
    ->dispatch();

// 立即查询 - result 有 stdout
$result = xhjob_result($id);

// 等待 6 秒后查询 - result 已清理（stdout=null）
sleep(6);
$result = xhjob_result($id);

// 但 task 仍可查询
$state = xhjob_state($id);

xhjob_stop();
```

- `resultTtl(0)`（默认）= 永久保留，与当前行为一致
- daemon 后台周期性调用 `cleanup_expired_results`（每 60 秒一次节流）清理过期 result

## 任务元数据

参考 Celery `update_state` 的 meta 字段，可为任务附加任意用户元数据（JSON 字符串），便于业务侧追踪：

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaHttp('POST', 'https://api.example.com/orders')
    ->withBody(json_encode(['product' => 'widget']))
    ->withMeta(json_encode(['order_id' => 'A123', 'user' => 'alice']))
    ->dispatch();

// 查询时拿到 meta
$state = xhjob_state($id);
$meta = json_decode($state['meta'], true);
echo "order_id: {$meta['order_id']}\n";

xhjob_stop();
```

- meta 持久化到 SQLite，daemon restart 后保留
- meta 内容由用户自定义，xhjob 不解析 JSON 结构

## 对照 APScheduler / Celery 的功能对齐

xhjob 借鉴 Python 成熟定时任务模块 [APScheduler](https://apscheduler.readthedocs.io/) 与后台任务队列模块 [Celery](https://docs.celeryq.dev/) 的设计，对齐单机版合理可用的特性：

### 已对齐 APScheduler

- ✅ Cron 表达式（5/6 段，6 段含秒）
- ✅ `max_instances` / `coalesce` / `misfire_grace_time`
- ✅ `maxExecutions` 限制执行次数（参考 APScheduler `max_executions`）
- ✅ `startAt` / `endAt` 时间窗口（参考 `start_date` / `end_date`）
- ✅ `xhjob_pause` / `xhjob_resume`（参考 `pause_job` / `resume_job`）
- ✅ `xhjob_remove`（参考 `remove_job`）
- ✅ `xhjob_list`（参考 `get_jobs`）
- ✅ `next_fire` 暴露在 `xhjob_state`（参考 `next_run_time`）

### 已对齐 Celery

- ✅ 重试机制 + 指数退避（`withRetry(max, delay)`）
- ✅ `AsyncResult` 风格的 `xhjob_state` / `xhjob_result`
- ✅ `xhjob_cancel`（参考 `revoke`）
- ✅ `resultTtl` 结果过期清理（参考 `result_expires`）
- ✅ `withMeta` 元数据（参考 `update_state` meta）
- ✅ `priority` 队列优先级
- ✅ 按错误类型真实判断是否重试（HTTP 5xx 重试 / 4xx 不重试）

### 不对齐的特性（避免过度设计）

- ❌ 分布式 worker / broker（Celery Redis/RabbitMQ 依赖）—— 超出单机目标
- ❌ 任务链 chain / group / chord（Celery canvas）—— 留待未来
- ❌ 任务事件流 events（Celery events）—— `Event` 结构体保留为未来接口，本轮不启用
- ❌ beat 调度器（Celery beat）—— cron 调度已实现
- ❌ 多 datastore 抽象（APScheduler SQLAlchemy/MongoDB/Redis）—— 仅支持 SQLite

## API 参考

### 顶层函数

| 函数 | 说明 |
|------|------|
| `xhjob_start($name="default", $data_dir=null): bool` | 启动（或确认已启动）指定服务的 daemon，可选 data_dir |
| `xhjob_stop($name="default", $data_dir=null): bool` | 停止指定服务的 daemon |
| `xhjob_restart($name="default", $data_dir=null): bool` | 重启指定服务的 daemon |
| `xhjob_status($name="default", $data_dir=null): array` | 查询 daemon 运行状态（`running`、`pid`） |
| `xhjob_dispatch($task_json, $name="default", $data_dir=null): string` | 通过 JSON 字符串 dispatch 任务，返回 task_id；失败时返回 `error: <msg>` 字符串，用 `str_starts_with($id, 'error:')` 判断 |
| `xhjob_state($id, $name="default", $data_dir=null): array` | 查询任务状态（`state`、`attempts`、`created_at`、`started_at`、`finished_at`、`last_error`、`execution_count`、`max_executions`、`next_fire`、`paused`、`start_date`、`end_date`、`meta`） |
| `xhjob_result($id, $name="default", $data_dir=null): array` | 查询任务结果（`body`、`status_code`、`stdout`、`stderr`、`exit_code`）；任务在产出结果前失败时返回 `error` 字段，提示查 `xhjob_state()` 的 `last_error` |
| `xhjob_remove(string $id, string $name = "default", ?string $data_dir = null): bool` | 删除 cron 作业定义（不影响运行中实例） |
| `xhjob_pause(string $id, string $name = "default", ?string $data_dir = null): bool` | 暂停 cron 作业（保留定义，不触发） |
| `xhjob_resume(string $id, string $name = "default", ?string $data_dir = null): bool` | 恢复已暂停的 cron 作业 |
| `xhjob_cancel(string $id, string $name = "default", ?string $data_dir = null): bool` | 取消任务（Pending→Cancelled 终态；Running→停止后续重试与 cron 触发，不强制 kill） |
| `xhjob_list(string $name = "default", ?string $state_filter = null, ?string $data_dir = null): string` | 列出任务摘要（JSON 字符串，需 json_decode） |

### `Xhjob` 类（链式 API）

| 方法 | 说明 |
|------|------|
| `Xhjob::task(): Xhjob` | 创建一个新的 builder |
| `service(string $name): $this` | 绑定到指定服务名 |
| `dataDir(string $dir): $this` | 指定 PID/sock/db/log 的统一存放目录（备份/迁移用） |
| `viaHttp(string $method, string $url): $this` | 设置为 HTTP 任务 |
| `viaShell(string $cmd): $this` | 设置为 Shell 任务 |
| `withHeaders(array $headers): $this` | 设置 HTTP headers |
| `withBody(string $body): $this` | 设置 HTTP body |
| `withProxy(string $proxy): $this` | 设置 HTTP/SOCKS5 代理 |
| `withEncoding(string $from): $this` | 设置 Shell 输出编码 |
| `withTimezone(string $tz): $this` | 设置 Cron 时区 |
| `withRetry(int $max, int $delay): $this` | 设置重试次数与基础延迟（秒） |
| `cron(string $expr): $this` | 设置 cron 表达式（5 或 6 段表达式，6 段含秒） |
| `timeout(int $secs): $this` | 设置执行超时（秒） |
| `priority(int $p): $this` | 设置任务优先级（数值越大越先执行） |
| `allowOverlap(bool $allow): $this` | 是否允许同一任务并发执行 |
| `maxInstances(int $n): $this` | 最大并发实例数 |
| `coalesce(bool $c): $this` | 是否合并错过的触发 |
| `persist(bool $p): $this` | 是否启用 SQLite 持久化 |
| `maxExecutions(int $n): $this` | 设置 cron 任务最大执行次数（0=无限，默认）；到达上限后 state=SUCCESS |
| `startAt(int $ts): $this` | 设置任务起始时间（Unix ts）；此前 cron 不触发 |
| `endAt(int $ts): $this` | 设置任务结束时间（Unix ts）；此后 state=SUCCESS 终态 |
| `resultTtl(int $secs): $this` | 设置结果保留时长（0=永久，默认）；超时后 result 自动清理但 task 保留 |
| `withMeta(string $json): $this` | 附加用户元数据（JSON 字符串），可在 xhjob_state 中读取 |
| `dispatch(): string` | 提交任务到 daemon，返回 task_id；失败时返回 `error: <msg>` 字符串 |

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `XHJOB_DATA_DIR` | 平台默认 | 统一数据目录（PID/sock/db/log 同时落入此目录），优先级低于细粒度变量；也是 daemon 子进程内的当前数据目录（由父进程自动设置，函数参数可覆盖） |
| `XHJOB_PID_DIR` (Unix) | `/tmp` | PID 文件目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_SOCK_DIR` (Unix) | `/tmp` | IPC socket 目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_DB_DIR` (Unix) | `/tmp` | SQLite 数据库目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_LOG_DIR` (Unix) | `/tmp` | 日志文件目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_SERVICE_NAME` | `default` | daemon 子进程内当前服务名（由父进程自动设置） |
| `XHJOB_PERSIST` | `0` | 设为 `1` 或 `true` 时 daemon 启用 SQLite 持久化存储；仅在 daemon 启动时读取一次，运行中修改需 restart 生效 |
| `XHJOB_THREAD_POOL_SIZE` | `num_cpus` | 线程池大小 |
| `XHJOB_COROUTINE_POOL_SIZE` | `1024` | 协程池大小 |
| `XHJOB_SHELL_TIMEOUT` | `300` | Shell 任务默认超时（秒） |

## 测试

```bash
# 编译
cargo build --release
cargo build --release --features persist

# Rust 单元测试
cargo test
cargo test --features persist

# PHP .phpt 集成测试
php -d extension=target/release/libxhjob.so tests/run-tests.php tests/

# data_dir 功能专项测试（验证 pid/sock/db/log 全部落入用户指定目录）
php -d extension=xhjob.so tests/data_dir_smoke.php

# php-cli 业务场景串联测试
bash tests/business/cli_bus/run_all.sh production-queue

# php-fpm 模拟业务测试（proc_open / HTTP 两种模式）
php -d extension=xhjob.so tests/business/fpm_sim/proc_test.php
php -d extension=xhjob.so tests/business/fpm_sim/client_test.php
```

## 许可证

参见 [LICENSE](LICENSE)。
