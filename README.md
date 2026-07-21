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
- `tags`（A15）：用户标签数组，便于客户端按 tag 过滤

`state_filter` 可选值：`PENDING` / `RUNNING` / `INTERRUPTED` / `SUCCESS` / `FAILED` / `CANCELLED` / `EXPIRED`

### TaskState 枚举

| 状态 | 说明 | 是否终态 |
|------|------|----------|
| `PENDING` | 已入队等待执行 | 否 |
| `RUNNING` | 正在执行 | 否 |
| `INTERRUPTED` | daemon 异常退出时被中断（非终态，可恢复） | 否 |
| `SUCCESS` | 执行成功 | 是 |
| `FAILED` | 执行失败（重试耗尽） | 是 |
| `CANCELLED` | 被 `xhjob_cancel` 取消 | 是 |
| `EXPIRED` | Pending 任务超过 `expires` 上限（C6） | 是 |

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

## Interval 周期触发（every）

参考 APScheduler `IntervalTrigger`，通过 `every(int $secs)` 让任务以固定秒数周期触发，无需编写 cron 表达式。适用于「每 60 秒健康检查」「每 30 秒拉取指标」这类没有自然 cron 表达式的周期任务。

```php
<?php
xhjob_start();

// 每 60 秒触发一次 shell 任务
$id = Xhjob::task()
    ->viaShell('echo hi')
    ->every(60)
    ->dispatch();

var_dump(xhjob_state($id));

xhjob_stop();
```

**与 cron / runAt 的互斥关系**（优先级从高到低）：

1. `runAt` 最高 —— 若同时设置，`cron` 与 `every` 均被忽略（仅触发一次）
2. `cron` 次之 —— 若同时设置，`every` 被忽略
3. `every` 最低 —— 仅在没有 `cron` 与 `runAt` 时生效

冲突时 daemon 会在日志中输出 warning，dispatch 不会失败。`every(0)` 视为未设置。

## DateTrigger 一次性触发（runAt）

参考 APScheduler `DateTrigger`，通过 `runAt(int $ts)` 指定一个绝对 Unix 时间戳，任务在该时刻触发一次后立即进入 `SUCCESS` 终态。适用于「5 分钟后刷新缓存」「明天 9 点发公告」这类一次性未来任务。

```php
<?php
xhjob_start();

// 5 分钟后触发一次 HTTP 调用
$id = Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/refresh')
    ->runAt(time() + 300)
    ->dispatch();

var_dump(xhjob_state($id));

xhjob_stop();
```

- `runAt` 是优先级最高的调度方式：与 `cron` / `every` 同设时仅 `runAt` 生效
- 触发后任务立即进入 `SUCCESS` 终态，不会再次触发
- `jitter` 对 `runAt` 任务无效（一次性触发需要精确时间戳）
- `next_fire` 等于 `runAt` 设置的 Unix 时间戳

## Jitter 随机抖动

参考 APScheduler `jitter`，通过 `jitter(int $secs)` 在 cron / interval 任务的 `next_fire` 上叠加一个 `[0, secs]` 范围内的随机偏移，避免大量任务在同一秒触发造成的「惊群效应」（thundering herd）。

```php
<?php
xhjob_start();

// 100 个相同 cron 表达式的任务，使用 jitter(30) 让触发时刻分散在 30s 内
for ($i = 0; $i < 100; $i++) {
    Xhjob::task()
        ->viaHttp('GET', "https://api.example.com/ping?n={$i}")
        ->cron('* * * * *')  // 每分钟
        ->jitter(30)         // next_fire 随机延迟 0~30 秒
        ->dispatch();
}

xhjob_stop();
```

- `jitter(0)`（默认）= 无随机偏移，按 cron / interval 精确触发
- 每次 `next_fire` 重新计算时都重新采样随机偏移
- 对 `runAt` 任务无效（一次性触发需要精确时间戳）

## 任务级过期（expires）

参考 APScheduler `expires`，通过 `expires(int $secs)` 为任务设置「Pending 阶段超时」：若任务自 `created_at` 起在 `expires` 秒内仍未开始执行（仍处于 `PENDING` 状态），自动转为 `EXPIRED` 终态，daemon 不再调度该任务。

```php
<?php
xhjob_start();

// 任务必须在 10 秒内被 daemon 拾取执行；否则置为 EXPIRED
$id = Xhjob::task()
    ->viaShell('echo hello')
    ->expires(10)
    ->dispatch();

// 10 秒后查询：若 daemon 因过载未及时拾取，state 将为 EXPIRED
sleep(12);
$state = xhjob_state($id);
echo "state={$state['state']}\n";  // EXPIRED 或 SUCCESS

xhjob_stop();
```

**与 `resultTtl` 的区别**：

| 配置 | 作用对象 | 行为 |
|------|----------|------|
| `expires` | Pending 任务 | 阻止未执行的任务继续排队，转入 `EXPIRED` 终态 |
| `resultTtl` | 已执行任务的 result 行 | 任务终态后定时清理 result（task 行保留） |

- `expires(0)`（默认）= 永不过期，Pending 任务一直等待
- 只影响 `PENDING` 状态任务；`RUNNING` 任务不会被中断

## 任务重新入队（requeue）

参考 Celery `requeue`，通过 `xhjob_requeue(string $id)` 把处于终态（`CANCELLED` / `FAILED` / `EXPIRED`）或已取消的任务重新置为 `PENDING`，重置 `attempts=0`、`next_fire=now`，让 daemon 重新调度执行。适用于「失败任务修复 bug 后重跑」「误取消后恢复」场景。

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaShell('echo first-try')
    ->dispatch();

// 等待执行完成
usleep(500_000);
$state = xhjob_state($id);
echo "first run: state={$state['state']}\n";  // SUCCESS

// 重新入队，再次执行
$ok = xhjob_requeue($id);
echo "requeue: " . ($ok ? "OK" : "FAIL") . "\n";

usleep(500_000);
$state = xhjob_state($id);
echo "second run: state={$state['state']} attempts={$state['attempts']}\n";

xhjob_stop();
```

- 仅终态任务可被 requeue；`PENDING` / `RUNNING` 任务返回 `false`
- `cron` 任务被 requeue 后保留 cron 表达式，按 cron 继续周期触发
- 重置 `attempts=0`，但不重置 `execution_count`（历史执行次数累计）

## Retry 指数退避（retryBackoff）

参考 Celery `retry_backoff`，通过 `retryBackoff(bool $on)` 启用指数退避重试。开启后重试间隔随失败次数指数增长，公式 `min(retry_delay * 2^(attempts-1), retry_delay * 60)`，避免对故障后端持续高频重试。

```php
<?php
xhjob_start();

// 固定退避：每次重试间隔 2s
Xhjob::task()
    ->viaHttp('GET', 'https://flaky.example.com/api')
    ->withRetry(5, 2)
    ->retryBackoff(false)  // 默认
    ->dispatch();

// 指数退避：重试间隔 2s, 4s, 8s, 16s, 32s（上限 2*60=120s）
Xhjob::task()
    ->viaHttp('GET', 'https://flaky.example.com/api')
    ->withRetry(5, 2)
    ->retryBackoff(true)
    ->dispatch();

xhjob_stop();
```

**时间序列对比**（`retry_delay=2s`，5 次重试）：

| 失败次数 | 固定退避 | 指数退避 |
|----------|----------|----------|
| 1 | 2s | 2s |
| 2 | 2s | 4s |
| 3 | 2s | 8s |
| 4 | 2s | 16s |
| 5 | 2s | 32s |

- 上限为 `retry_delay * 60`，避免指数爆炸（如 `retry_delay=2` 时上限 120s）
- `retryBackoff(false)`（默认）= 固定 `retry_delay` 间隔
- 与 `withRetry($max, $delay)` 配合使用，`retry_delay` 即为指数退避的基数

## max_instances 并发实例数（A10）

参考 APScheduler `max_instances`，通过 `maxInstances(int $n)` 显式设置同一任务允许的最大并发实例数。当 N>1 时允许 N 个实例并行；N=1 时与 `allowOverlap(false)` 等价（默认）。

```php
<?php
xhjob_start();

// 允许 2 个并发实例：长任务运行中可继续触发新实例
$id = Xhjob::task()
    ->viaShell('sleep 5 && echo done')
    ->cron('*/1 * * * * *')
    ->maxInstances(2)
    ->dispatch();

var_dump(xhjob_state($id));

xhjob_stop();
```

**与 `allowOverlap` 的关系**：

| 配置 | 行为 |
|------|------|
| `maxInstances(1)`（默认）+ `allowOverlap(false)` | 严格串行（默认） |
| `maxInstances(1)` + `allowOverlap(true)` | 1 个实例（N 优先于 overlap） |
| `maxInstances(N>1)` | 允许 N 个并发实例，忽略 `allowOverlap` |

- `maxInstances` 优先级高于 `allowOverlap`：一旦 `maxInstances>1` 显式设置，`allowOverlap` 被忽略
- 超过 N 个并发实例时新触发被跳过，并记录 `max_instances_reached` 事件
- 与 `coalesce` 配合：`coalesce=true` 合并错过的触发为一次，仍受 `maxInstances` 约束

## reschedule 在线修改 cron（A11）

参考 APScheduler `reschedule_job`，通过 `xhjob_reschedule(string $id, string $cron, ...)` 在线修改 cron 任务的 cron 表达式，保留 `state` / `execution_count` / `attempts` / `meta` 等运行时状态，仅替换 `cron` 字段并按新表达式重新计算 `next_fire`。

```php
<?php
xhjob_start();

// 初始：每分钟触发
$id = Xhjob::task()
    ->viaShell('echo hi')
    ->cron('*/1 * * * * *')
    ->persist(true)
    ->dispatch();
echo "Initial cron: " . xhjob_state($id)['cron'] . "\n";  // (cron 不在 state，请用 xhjob_get)

// 在线改为每 5 秒触发
$ok = xhjob_reschedule($id, '*/5 * * * * *');
echo "Reschedule: " . ($ok ? "OK" : "FAIL") . "\n";

// 验证：execution_count 保留，cron 已更新
$task = json_decode(xhjob_get($id), true);
echo "New cron={$task['cron']}, execution_count={$task['execution_count']}\n";

xhjob_stop();
```

- 仅对 cron 任务有效；`interval` / `runAt` 任务返回 `false`
- 终态任务（`SUCCESS` / `FAILED` / `CANCELLED` / `EXPIRED`）返回 `false`
- 新 cron 表达式语法错误时返回 `false`，原 cron 不变
- 与 `persist(true)` 配合：daemon restart 后新 cron 表达式保留

## xhjob_get 单任务详情查询（A12）

参考 APScheduler `get_job`，通过 `xhjob_get(string $id)` 返回完整 Task JSON（包含所有配置字段），区别于 `xhjob_state`（仅返回 `StateInfo` 视图的核心状态字段）。

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/health')
    ->withRetry(3, 2)
    ->maxInstances(2)
    ->tags('monitor', 'critical')
    ->rateLimit(10, 60)
    ->dispatch();

// xhjob_get 返回完整配置 JSON
$json = xhjob_get($id);
$task = json_decode($json, true);
echo "id={$task['id']}\n";
echo "retry_max={$task['retry_max']}\n";
echo "max_instances={$task['max_instances']}\n";
echo "tags=" . implode(',', $task['tags']) . "\n";
echo "rate_limit_count={$task['rate_limit_count']}\n";

// xhjob_state 返回精简状态视图
$state = xhjob_state($id);
echo "state={$state['state']}\n";

xhjob_stop();
```

**`xhjob_get` vs `xhjob_state` 字段对比**：

| 字段 | `xhjob_state` | `xhjob_get` |
|------|----------------|-------------|
| `state` / `attempts` / `created_at` / `meta` | ✅ | ✅ |
| `interval` / `run_at` / `jitter` / `expires` | ✅ | ✅ |
| `retry_backoff` / `ignore_result` / `acks_late` / `soft_timeout` | ✅ | ✅ |
| `cron` / `retry_max` / `retry_delay` / `timeout` / `priority` | ❌ | ✅ |
| `max_instances` / `coalesce` / `timezone` / `misfire_grace_time` | ❌ | ✅ |
| `tags` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure` | ❌ | ✅ |
| `allow_overlap` / `replace_existing` / `proxy` / `encoding` | ❌ | ✅ |

- 任务不存在或 daemon 不可达时返回 `null`
- 返回的 JSON 字符串可用 `json_decode($json, true)` 解码为关联数组

## ignoreResult fire-and-forget（C9）

参考 Celery `ignore_result`，通过 `ignoreResult(bool $on)` 跳过 `save_result` 步骤，任务终态后不写入 result 行，`xhjob_result()` 返回 null。任务状态机仍正常运转（`PENDING → RUNNING → SUCCESS/FAILED`）。适用于「调用即忘」的高吞吐任务，调用方不关心返回值。

```php
<?php
xhjob_start();

// 高吞吐埋点上报：忽略结果，节省存储
$id = Xhjob::task()
    ->viaHttp('POST', 'https://analytics.example.com/track')
    ->withBody(json_encode(['event' => 'click', 'user' => 'alice']))
    ->ignoreResult(true)
    ->dispatch();

usleep(500_000);
$state = xhjob_state($id);
echo "state={$state['state']}\n";  // SUCCESS

$result = xhjob_result($id);
var_dump($result);  // ['error' => 'no result record for this task...']

xhjob_stop();
```

**与 `resultTtl` 的区别**：

| 配置 | 行为 |
|------|------|
| `ignoreResult(true)` | 不存储 result 行（`save_result` 步骤被跳过） |
| `resultTtl(N>0)` | 存储 result，N 秒后自动清理 |
| 两者同设 | `ignoreResult` 优先，result 永不存储（warning 日志） |

## acksLate 崩溃恢复（C10）

参考 Celery `acks_late`，通过 `acksLate(bool $on)` 启用「延迟确认」语义：daemon 重启时 `RUNNING` 状态的任务自动重置为 `PENDING`，重新调度执行。适用于「任务必须执行完成」的幂等任务（如订单处理、文件转换）。

```php
<?php
xhjob_start();

// 幂等任务：daemon 崩溃后自动重跑
$id = Xhjob::task()
    ->viaShell('convert input.png output.jpg')
    ->withRetry(3, 5)
    ->acksLate(true)  // 重启后重置为 Pending
    ->persist(true)
    ->dispatch();

// 模拟 daemon 崩溃后重启
xhjob_stop();
sleep(1);
xhjob_start();

// 若任务原本是 RUNNING，重启后会被自动重置为 PENDING 重新执行
$state = xhjob_state($id);
echo "state after restart: {$state['state']}\n";

xhjob_stop();
```

**`acksLate(true)` vs `acksLate(false)` 行为对比**：

| 场景 | `acksLate(false)`（默认） | `acksLate(true)` |
|------|---------------------------|-------------------|
| daemon 正常重启 + `persist(true)` | Running → Pending（persist 模式下默认重置） | Running → Pending |
| daemon 崩溃恢复 | 保持 Running（需手动 `xhjob_requeue`） | 自动重置为 Pending |
| 非持久化任务 | 任务丢失 | 任务丢失 |

- 与 `persist(true)` 配合使用：仅在持久化场景下重启后才能恢复
- 仅对 `RUNNING` 状态任务生效；`PENDING` 任务本就会被调度
- 适用于幂等任务；非幂等任务慎用（可能重复执行）

## softTimeout 软超时（C11）

参考 Celery `soft_time_limit`，通过 `softTimeout(int $secs)` 为 Shell 任务设置软超时：先发送 `SIGTERM` 请求子进程优雅退出，宽限期（`timeout - soft_timeout` 秒）后仍不退出再 `SIGKILL` 强制终止。

```php
<?php
xhjob_start();

// Shell 任务：5s 软超时（SIGTERM），10s 硬超时（SIGKILL）
$id = Xhjob::task()
    ->viaShell('trap "echo cleaning-up; exit 0" TERM; sleep 30')
    ->timeout(10)
    ->softTimeout(5)
    ->dispatch();

usleep(6_000_000);
$state = xhjob_state($id);
echo "state after soft timeout: {$state['state']}\n";

xhjob_stop();
```

**触发流程**：

1. `soft_timeout` 秒到达 → daemon 发送 `SIGTERM` 给子进程
2. 子进程可监听 `SIGTERM` 信号做清理工作（如 `trap '...' TERM`）
3. 宽限期（`timeout - soft_timeout` 秒）后子进程仍未退出 → 发送 `SIGKILL` 强制终止

**约束**：

- 仅 Shell 任务生效；HTTP 任务忽略（HTTP 客户端无法优雅中断，warning 日志）
- `soft_timeout >= timeout` 时忽略（必须留出非零 SIGKILL 宽限期，warning 日志）
- `softTimeout(0)` = 清除软超时，仅使用 `timeout` 硬超时

## misfire_grace_time 每作业级（A13）

参考 APScheduler `misfire_grace_time`，通过 `misfireGraceTime(int $secs)` 为单个 cron 任务设置专属的 misfire 宽限窗口，覆盖全局默认 60 秒。当 `now - next_fire > grace_time` 时该次触发被判定为 misfire，按 `coalesce` 规则处理。

```php
<?php
xhjob_start();

// 高时效任务：错过 5 秒就跳过
Xhjob::task()
    ->viaShell('echo realtime-tick')
    ->cron('*/1 * * * * *')
    ->misfireGraceTime(5)
    ->coalesce(false)  // 超过 5s 直接丢弃
    ->dispatch();

// 低时效任务：可接受 5 分钟延迟
Xhjob::task()
    ->viaShell('echo batch-report')
    ->cron('0 * * * *')
    ->misfireGraceTime(300)
    ->coalesce(true)  // 合并为一次执行
    ->dispatch();

xhjob_stop();
```

**与 `coalesce` 配合规则**：

| 配置 | 错过触发的处理 |
|------|----------------|
| `misfireGraceTime(60)` + `coalesce(true)` | 在窗口内合并为一次执行 |
| `misfireGraceTime(60)` + `coalesce(false)` | 在窗口内仍执行一次；超出窗口跳过 |
| `misfireGraceTime(0)` | 使用全局默认 60 秒 |

- 仅 cron 任务生效；`interval` / `runAt` 任务忽略（warning 日志）
- 全局默认 60 秒可通过 daemon 端配置覆盖（未来版本）

## replace_existing 幂等 dispatch（A14）

参考 APScheduler `replace_existing`，通过 `id(string $id) + replaceExisting(true)` 实现部署脚本幂等：使用固定 task id，重复 dispatch 时直接替换已存在的任务（state / attempts / execution_count 重置），不报冲突错误。适用于 CI/CD 部署脚本反复执行同一任务定义的场景。

```php
<?php
xhjob_start();

// 部署脚本可重复执行：每次 dispatch 都用相同 id，老任务被替换
$id = Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/health')
    ->cron('*/5 * * * *')
    ->id('deploy-health-check-prod')  // 固定 id
    ->replaceExisting(true)           // 已存在则替换
    ->persist(true)
    ->dispatch();

echo "Dispatched (idempotent): {$id}\n";
// 第二次执行部署脚本时返回相同 id，老任务被完全重置
$state = xhjob_state($id);
echo "state={$state['state']} attempts={$state['attempts']}\n";

xhjob_stop();
```

- `id('')` 视为未设置（使用自动生成的 UUID）
- `replaceExisting(false)`（默认）+ id 冲突 → dispatch 返回 `error: <msg>`
- `replaceExisting(true)` 仅在 `id` 设置时生效；未设 id 时仍走自动生成路径
- 替换时 state / attempts / execution_count 全部重置，meta / cron 表达式以新值为准

## tags 作业分组（A15）

参考 APScheduler `tags`，通过 `tag(string $tag)` 链式调用为任务附加多个标签，便于业务侧分组管理与过滤。标签存储在 `Task.tags` 数组中，可通过 `xhjob_get` / `xhjob_state` 查询，并参与 `xhjob_list` 客户端过滤。

```php
<?php
xhjob_start();

// 监控类任务
Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/health')
    ->cron('*/30 * * * * *')
    ->tag('monitor')->tag('critical')->tag('prod')
    ->dispatch();

// 备份类任务
Xhjob::task()
    ->viaShell('tar czf /tmp/backup.tgz /var/data')
    ->cron('0 2 * * *')
    ->tag('backup')->tag('nightly')
    ->dispatch();

// 列出所有任务，按 tag 在客户端过滤
$tasks = json_decode(xhjob_list(), true);
$monitors = array_filter($tasks, fn($t) => in_array('monitor', $t['tags'] ?? []));
foreach ($monitors as $t) {
    echo "monitor task: {$t['id']} cron={$t['cron']}\n";
}

xhjob_stop();
```

- 空字符串与重复 tag 被静默忽略
- `TaskSummary` 字段含 `tags` 数组，`xhjob_list` 返回值可见
- 服务端 `list_tasks(state, tag_filter)` 支持 tag 过滤；PHP 层 `xhjob_list` 暂未暴露第三参数，可在客户端 `array_filter` 过滤

## rateLimit 每任务限流（C12）

参考 Celery `rate_limit`，通过 `rateLimit(int $count, int $windowSecs)` 为单个任务设置滑动窗口限流：在 `windowSecs` 秒内最多触发 `count` 次。超限触发被跳过并记录 `rate_limited` 事件，`next_fire` 推进一个 window。

```php
<?php
xhjob_start();

// 外部 API 限流：10 秒内最多 3 次调用
$id = Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/ratelimited')
    ->cron('*/1 * * * * *')  // 每秒尝试触发
    ->rateLimit(3, 10)       // 但 10s 内最多 3 次
    ->dispatch();

// 持续 30 秒，观察实际触发次数 ≈ 9 次
sleep(30);
$state = xhjob_state($id);
echo "execution_count={$state['execution_count']}\n";  // 约 9 次

xhjob_stop();
```

**与 `maxInstances` 的区别**：

| 配置 | 维度 | 行为 |
|------|------|------|
| `maxInstances(N)` | 并发实例数 | 同时运行实例数不超过 N（控制资源占用） |
| `rateLimit(C, W)` | 时间窗口触发数 | W 秒内最多触发 C 次（控制外部 API 速率） |

- `rateLimit(0, *)` = 无限流（默认）
- 滑动窗口算法：每次触发记录时间戳，触发前检查窗口内已触发数
- 超限触发被跳过（不重试），`next_fire` 推进一个 window

## acksOnFailure 失败不放弃（C13）

参考 Celery `acks_on_failure`，通过 `acksOnFailure(bool $on)` 控制任务失败时是否尊重 `retry_max` 上限。`acksOnFailure(false)` 时失败任务忽略 `retry_max`，持续重试直到成功或被取消，与 `acksLate` 互补用于「必须成功」的关键任务。

```php
<?php
xhjob_start();

// 关键任务：失败后无限重试（忽略 retry_max）
$id = Xhjob::task()
    ->viaHttp('POST', 'https://payment.example.com/charge')
    ->withBody(json_encode(['order' => 'A123', 'amount' => 100]))
    ->withRetry(3, 5)          // 名义上 3 次重试
    ->acksOnFailure(false)    // 但失败后忽略 retry_max，持续重试
    ->dispatch();

// 任务会一直重试直到成功，或被 xhjob_cancel / xhjob_remove 终止
$state = xhjob_state($id);
echo "state={$state['state']}\n";

xhjob_stop();
```

**与 `acksLate` 互补关系**：

| 配置 | 解决的问题 |
|------|-----------|
| `acksLate(true)` | daemon 崩溃后 `RUNNING` 任务自动重跑（崩溃恢复） |
| `acksOnFailure(false)` | 任务失败后忽略 `retry_max` 持续重试（必达） |
| 两者同设 | 崩溃 + 失败双重保障 |

- `acksOnFailure(true)`（默认）= 失败时尊重 `retry_max`，重试耗尽后置 `FAILED`
- `acksOnFailure(false)` + `withRetry(N, M)` = 名义重试 N 次，实际无限重试
- 与 `xhjob_cancel` 配合：手动终止持续重试的任务

## worker_max_tasks_per_child daemon 自我回收（C14）

参考 Celery `worker_max_tasks_per_child`，通过环境变量 `XHJOB_MAX_TASKS_PER_CHILD=N` 设置 daemon 进程在累计执行 N 次任务后自动退出，由外层 supervisor（systemd / supervisord / docker restart=always / PHP 调用方）拉起新进程。用于防止内存泄漏累积、定期刷新进程状态。

```bash
# 启动 daemon 时指定：每执行 1000 个任务后自动退出
XHJOB_MAX_TASKS_PER_CHILD=1000 php -d extension=xhjob.so your-app.php

# systemd unit 配置示例
[Service]
Environment=XHJOB_MAX_TASKS_PER_CHILD=1000
ExecStart=/usr/bin/php -d extension=xhjob.so /opt/app/start.php
Restart=always
RestartSec=2
```

```php
<?php
// PHP 调用方配合：daemon 自我退出后由调用方重启
$retries = 0;
while ($retries < 100) {
    xhjob_start();
    // daemon 退出后 xhjob_status()['running'] = false
    while (xhjob_status()['running'] === 'true') {
        sleep(1);
    }
    $retries++;
    echo "daemon recycled, restarting (round {$retries})\n";
}
```

- `XHJOB_MAX_TASKS_PER_CHILD=0`（默认）= 不限，daemon 永不自我退出
- daemon 退出前会完成当前正在执行的任务，避免硬中断
- 与 `systemd Restart=always` / `supervisord autorestart=true` / `docker restart=always` 配合使用
- 任务计数包含成功 + 失败，不包含取消 / 跳过

## timezone per-job 每作业独立时区（A16）

> 本节内容已与上文「## Cron 自定义时区」小节合并覆盖：每个 cron 任务可通过 `withTimezone(string $tz)` 指定独立的 IANA 时区，`next_fire` 按该时区计算。本节补充 per-job 时区的细节与跨国应用场景。

参考 APScheduler `CronTrigger(timezone=...)`，xhjob 支持每个 cron 任务携带独立的 IANA 时区标识：

```php
<?php
xhjob_start();

// 跨国应用：纽约、上海、伦敦三地分别按本地 9 点执行日报任务
Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/report?region=us')
    ->cron('0 9 * * *')
    ->withTimezone('America/New_York')
    ->dispatch();

Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/report?region=cn')
    ->cron('0 9 * * *')
    ->withTimezone('Asia/Shanghai')
    ->dispatch();

Xhjob::task()
    ->viaHttp('GET', 'https://api.example.com/report?region=uk')
    ->cron('0 9 * * *')
    ->withTimezone('Europe/London')
    ->dispatch();

xhjob_stop();
```

**关键行为**：

- **IANA 时区标识**：必须为合法的 IANA 时区名（如 `Asia/Shanghai`、`America/New_York`、`UTC`），完整列表见 [IANA Time Zone Database](https://www.iana.org/time-zones)
- **解析失败回退**：非法时区字符串在 `dispatch()` 时立即返回错误，任务不入队（不回退到全局时区）
- **非 cron 任务忽略**：`interval` / `runAt` 任务忽略 `timezone` 字段（按 UTC + 本地时区计算）
- **跨国应用场景**：同一服务内可为不同区域的任务设置不同时区，无需为每个区域启动独立 daemon

## Event listener API 任务执行事件流（A17）

参考 APScheduler `add_listener` + `EVENT_JOB_*`，通过 `xhjob_events(int $since_ts, ?string $task_id = null, string $name = "default", ?string $data_dir = null): string` 查询自 `since_ts` 以来的任务执行事件流。事件持久化到 store，daemon 周期性清理超过 TTL（默认 24 小时）的事件。

```php
<?php
xhjob_start();

$id = Xhjob::task()
    ->viaShell('echo event-demo')
    ->dispatch();

usleep(500_000);

// 查询过去 1 小时内的全部事件
$since = time() - 3600;
$json = xhjob_events($since);
$events = json_decode($json, true);
foreach ($events as $e) {
    echo "ts={$e['ts']} task={$e['task_id']} type={$e['event_type']}";
    if (isset($e['payload'])) echo " payload={$e['payload']}";
    echo "\n";
}

// 按任务 id 过滤
$task_events = json_decode(xhjob_events($since, $id), true);
echo "events for {$id}: " . count($task_events) . "\n";

xhjob_stop();
```

**事件类型清单**（`event_type` 字段取值）：

| `event_type` | 触发时机 |
|---------------|----------|
| `started` | 任务进入 `RUNNING` 状态 |
| `succeeded` | 任务执行成功，进入 `SUCCESS` |
| `failed` | 任务执行失败（含重试） |
| `missed` | cron 触发被判定为 misfire 并跳过 |
| `cancelled` | 任务被 `xhjob_cancel` 取消 |
| `paused` | 任务被 `xhjob_pause` 暂停 |
| `resumed` | 任务被 `xhjob_resume` 恢复 |
| `expired` | Pending 任务超时进入 `EXPIRED` |
| `max_instances_reached` | 触发因 `maxInstances` 上限被跳过 |
| `rate_limited` | 触发因 `rateLimit` 限流被跳过 |

**TTL 与清理**：

- 默认 TTL = 24 小时（`EVENT_TTL_SECS = 24 * 3600`）
- daemon 后台 `scan_once` 周期清理（每 60 秒一次节流）
- 未来可通过环境变量 `XHJOB_EVENTS_TTL_SECS` 覆盖（当前为常量）

**监控集成场景**：

- 接入 Prometheus / Grafana：定时轮询 `xhjob_events` 推送到 metrics
- 接入 ELK / Loki：将事件流写入日志聚合系统
- 业务告警：监听 `failed` / `expired` 事件触发 PagerDuty / 飞书告警

## coalesce 显式 per-job 行为（A18）

参考 APScheduler `coalesce`，通过 `coalesce(bool $on)` 为单个任务显式设置 missed trigger 的合并 / 丢弃行为。`coalesce(true)`（默认）将 daemon 离线期间错过的所有触发合并为一次执行；`coalesce(false)` 在 misfire_grace_time 窗口外直接丢弃。

```php
<?php
xhjob_start();

// 高一致性任务：合并所有错过，确保至少执行一次
Xhjob::task()
    ->viaShell('echo critical-tick')
    ->cron('*/1 * * * * *')
    ->coalesce(true)              // 合并 missed
    ->misfireGraceTime(3600)      // 1 小时内的错过都补执行
    ->dispatch();

// 实时任务：超过 grace 直接丢弃，不补执行
Xhjob::task()
    ->viaShell('echo realtime-tick')
    ->cron('*/1 * * * * *')
    ->coalesce(false)             // 丢弃 missed
    ->misfireGraceTime(5)         // 仅 5 秒内的错过执行一次
    ->dispatch();

xhjob_stop();
```

**与 `misfireGraceTime` 配合规则**：

| `coalesce` | `misfireGraceTime` | 错过触发的处理 |
|------------|---------------------|----------------|
| `true` | N 秒 | N 秒内的所有错过触发合并为一次执行 |
| `false` | N 秒 | N 秒内的错过仍执行一次；超过 N 秒的错过直接丢弃 |
| `true` | 0（默认） | 使用全局 60s，合并为一次 |
| `false` | 0（默认） | 使用全局 60s，超过则丢弃 |

- 仅 cron 任务生效；`interval` / `runAt` 任务忽略
- daemon 离线 / 重启期间错过的触发在恢复后由 `scan_once` 统一判定
- 与 `maxInstances` 配合：合并 / 丢弃后仍受并发实例数约束

## Task chain 顺序流水线（C15）

参考 Celery `chain(t1, t2, t3)`，通过 `xhjob_chain(array $task_configs, string $name = "default", ?string $data_dir = null): string` 创建顺序流水线：daemon 按顺序依次 dispatch 链中任务，每步成功后才执行下一步；任一步失败则整链置为 `failed` 并跳过剩余步骤。链记录持久化到 store，daemon 重启后可恢复进度。

```php
<?php
xhjob_start();

// 3 步 ETL 流水线：extract → transform → load
$tasks = [
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'curl -s https://api.example.com/raw > /tmp/raw.json'],
    ],
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'jq "[.items[] | {id, name}]" /tmp/raw.json > /tmp/clean.json'],
    ],
    [
        'task_type' => 'shell',
        'payload'    => ['cmd' => 'aws s3 cp /tmp/clean.json s3://bucket/clean-$(date +%s).json'],
    ],
];

$chain_id = xhjob_chain(json_encode($tasks));
echo "Chain dispatched: {$chain_id}\n";

// 轮询链状态：pending → running → succeeded / failed
for ($i = 0; $i < 60; $i++) {
    $json = xhjob_chain_state($chain_id);
    $state = json_decode($json, true);
    echo "state={$state['state']} step={$state['current_step']}/" . count($state['tasks']) . "\n";
    if ($state['state'] === 'succeeded' || $state['state'] === 'failed') break;
    sleep(1);
}

xhjob_stop();
```

**链状态机**：`pending` → `running` → `succeeded`（全部成功） / `failed`（任一步失败）

- `xhjob_chain_state(string $chain_id)` 返回完整 `ChainRecord` JSON（`chain_id` / `tasks` / `current_step` / `state` / `created_at` / `updated_at`）
- 每步任务的 `meta` 自动注入 `xhjob_chain_id` 字段，便于在 `xhjob_events` 中关联
- 失败中断后剩余步骤被跳过；已执行步骤的状态保留
- 与 `persist(true)` 配合：daemon 重启后链进度恢复

## Task group 并行批处理（C16）

参考 Celery `group(t1, t2, t3)`，通过 `xhjob_group(array $task_configs, string $name = "default", ?string $data_dir = null): string` 创建并行批处理：daemon 同时 dispatch 组内全部任务，每个任务独立执行。组状态随子任务完成情况实时更新。

```php
<?php
xhjob_start();

// 3 任务并行批处理：批量下载
$tasks = [
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/a.jpg https://example.com/a.jpg']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/b.jpg https://example.com/b.jpg']],
    ['task_type' => 'shell', 'payload' => ['cmd' => 'curl -s -o /tmp/c.jpg https://example.com/c.jpg']],
];

$group_id = xhjob_group(json_encode($tasks));
echo "Group dispatched: {$group_id}\n";

// 轮询组完成率
for ($i = 0; $i < 60; $i++) {
    $json = xhjob_group_state($group_id);
    $state = json_decode($json, true);
    $summary = $state['summary'] ?? ['total' => 0, 'succeeded' => 0, 'failed' => 0, 'pending' => 0];
    echo "state={$state['state']} total={$summary['total']} ok={$summary['succeeded']} fail={$summary['failed']} pend={$summary['pending']}\n";
    if ($state['state'] === 'succeeded' || $state['state'] === 'failed' || $state['state'] === 'partial_failed') break;
    sleep(1);
}

xhjob_stop();
```

**组状态机**：`pending` → `running` → `succeeded`（全部成功） / `partial_failed`（部分失败） / `failed`（全部失败）

- `xhjob_group_state(string $group_id)` 返回完整 `GroupRecord` JSON + `summary` 字段（`total` / `succeeded` / `failed` / `pending`）
- 子任务并行执行受 `maxInstances` 与 `rateLimit` 约束（每任务单独应用）
- 与 `persist(true)` 配合：daemon 重启后组状态可恢复
- 批量处理场景：批量发邮件、批量下载、批量数据迁移

## worker_max_memory_per_child 基于内存 daemon 自我回收（C17）

参考 Celery `worker_max_memory_per_child`，通过环境变量 `XHJOB_MAX_MEMORY_PER_CHILD=N`（单位：字节）设置 daemon 进程的 RSS 上限。daemon 在每次任务执行后检查当前 RSS，超过上限时优雅退出，由 supervisor 拉起新进程。用于防止内存泄漏导致的 OOM。

```bash
# 启动 daemon 时指定：RSS 超过 512MB 自动退出
XHJOB_MAX_MEMORY_PER_CHILD=536870912 php -d extension=xhjob.so your-app.php

# supervisor 配置示例
[program:xhjob]
command=/usr/bin/php -d extension=xhjob.so /opt/app/start.php
environment=XHJOB_MAX_MEMORY_PER_CHILD="536870912"
autorestart=true
```

```php
<?php
// 启动 daemon 时通过环境变量指定内存上限
putenv('XHJOB_MAX_MEMORY_PER_CHILD=536870912');  // 512 MB
xhjob_start();

// 执行多个内存密集任务，触发 daemon 自我回收
for ($i = 0; $i < 100; $i++) {
    Xhjob::task()
        ->viaShell('cat /dev/urandom | head -c 100M > /dev/null')
        ->dispatch();
}

// daemon 在某次任务后 RSS 超过 512MB 即退出
// 由 systemd / supervisor 自动重启
while (xhjob_status()['running'] === 'true') {
    sleep(1);
}
echo "daemon recycled due to memory limit\n";

xhjob_stop();
```

**跨平台内存读取**（`utils::memory::current_rss_bytes()`）：

| 平台 | 实现方式 |
|------|----------|
| Linux | 解析 `/proc/self/status` 中的 `VmRSS:` 行（kB → bytes） |
| macOS | `mach_task_basic_info` 系统调用（`resident_size`，单位 bytes） |
| Windows | `GetProcessMemoryInfo` API（`WorkingSetSize`，单位 bytes） |
| 其他 | 返回 `None`，跳过内存检查 |

**与 `worker_max_tasks_per_child` 互补**：

| 配置 | 触发条件 | 适用场景 |
|------|----------|----------|
| `XHJOB_MAX_TASKS_PER_CHILD=N` | 累计执行 N 次任务 | 定期刷新进程状态 |
| `XHJOB_MAX_MEMORY_PER_CHILD=N` | RSS 超过 N 字节 | 内存泄漏场景 |
| 两者同设 | 任一条件触发即退出 | 双重保障 |

- `XHJOB_MAX_MEMORY_PER_CHILD=0`（默认）= 不限
- daemon 退出前会完成当前任务，避免硬中断
- 仅在 daemon 启动时读取一次环境变量，运行中修改需 restart 生效
- 与 systemd / supervisor / docker restart=always 配合使用

## 对照 APScheduler / Celery 的功能对齐

xhjob 借鉴 Python 成熟定时任务模块 [APScheduler](https://apscheduler.readthedocs.io/) 与后台任务队列模块 [Celery](https://docs.celeryq.dev/) 的设计，对齐单机版合理可用的特性。下表覆盖 A1-A18（APScheduler 侧）与 C1-C17（Celery 侧）共 35 项。

### 已对齐 APScheduler（A1-A18）

| 编号 | APScheduler 特性 | xhjob 实现 |
|------|------------------|------------|
| A1 | Cron 表达式（5/6 段，6 段含秒） | `cron($expr)` + `withTimezone($tz)` |
| A2 | `max_instances` | `maxInstances(int $n)`（N>1 时与 `allowOverlap` 互斥） |
| A3 | `coalesce` | `coalesce(bool $on)` |
| A4 | `misfire_grace_time`（全局默认 60s） | daemon 内置全局默认；per-job 通过 A13 覆盖 |
| A5 | `max_executions` | `maxExecutions(int $n)`（0=无限，默认） |
| A6 | `start_date` / `end_date` | `startAt(int $ts)` / `endAt(int $ts)` |
| A7 | `IntervalTrigger` | `every(int $secs)` |
| A8 | `DateTrigger` | `runAt(int $ts)` |
| A9 | `jitter` | `jitter(int $secs)` |
| A10 | `max_instances`（N>1） | `maxInstances(int $n)` 优先于 `allowOverlap` |
| A11 | `reschedule_job` | `xhjob_reschedule($id, $cron)` |
| A12 | `get_job` | `xhjob_get($id)` 返回完整 Task JSON |
| A13 | `misfire_grace_time`（per-job 覆盖） | `misfireGraceTime(int $secs)` |
| A14 | `id` / `replace_existing` | `id(string $id)` + `replaceExisting(bool $on)` |
| A15 | `tags` | `tag(string $tag)` 链式 + `xhjob_list` 客户端过滤 |
| A16 | `CronTrigger(timezone=...)` per-job | `withTimezone(string $tz)` |
| A17 | `add_listener` + `EVENT_JOB_*` | `xhjob_events($since_ts, $task_id)` |
| A18 | `coalesce` 显式 per-job | `coalesce(bool $on)` + `misfireGraceTime` 配合 |
| - | `pause_job` / `resume_job` | `xhjob_pause` / `xhjob_resume` |
| - | `remove_job` | `xhjob_remove` |
| - | `get_jobs` | `xhjob_list` |
| - | `next_run_time` | `next_fire` 暴露在 `xhjob_state` |

### 已对齐 Celery（C1-C17）

| 编号 | Celery 特性 | xhjob 实现 |
|------|-------------|------------|
| C1 | 重试机制 + `retry_backoff` | `withRetry(max, delay)` + `retryBackoff(bool)` |
| C2 | `AsyncResult` 风格状态查询 | `xhjob_state` / `xhjob_result` |
| C3 | `revoke` | `xhjob_cancel` |
| C4 | `result_expires` | `resultTtl(int $secs)` |
| C5 | `update_state` meta | `withMeta(string $json)` |
| C6 | 任务级 `expires` | `expires(int $secs)`（Pending 超时 → EXPIRED 终态） |
| C7 | `requeue` | `xhjob_requeue($id)` |
| C8 | `retry_backoff` 指数退避 | `retryBackoff(true)`：`min(retry_delay * 2^(attempts-1), retry_delay * 60)` |
| C9 | `ignore_result` | `ignoreResult(bool $on)`（跳过 save_result） |
| C10 | `acks_late` | `acksLate(bool $on)`（崩溃恢复） |
| C11 | `soft_time_limit` | `softTimeout(int $secs)`（SIGTERM → 宽限期 → SIGKILL） |
| C12 | `rate_limit` | `rateLimit(int $count, int $windowSecs)` 滑动窗口 |
| C13 | `acks_on_failure` | `acksOnFailure(bool $on)`（失败不放弃） |
| C14 | `worker_max_tasks_per_child` | 环境变量 `XHJOB_MAX_TASKS_PER_CHILD=N` |
| C15 | `chain` | `xhjob_chain($tasks_json)` + `xhjob_chain_state($id)` |
| C16 | `group` | `xhjob_group($tasks_json)` + `xhjob_group_state($id)` |
| C17 | `worker_max_memory_per_child` | 环境变量 `XHJOB_MAX_MEMORY_PER_CHILD=N`（跨平台 RSS 读取） |
| - | `priority` 队列优先级 | `priority(int $p)` |
| - | 按错误类型判断重试 | HTTP 5xx 重试 / 4xx 不重试 |

### 不对齐的特性（避免过度设计）

- ❌ 分布式 worker / broker（Celery Redis/RabbitMQ 依赖）—— 超出单机目标
- ❌ chord（Celery canvas chord）—— 留待未来
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
| `xhjob_state($id, $name="default", $data_dir=null): array` | 查询任务状态（`state`、`attempts`、`created_at`、`started_at`、`finished_at`、`last_error`、`execution_count`、`max_executions`、`paused`、`start_date`、`end_date`、`meta`、`interval`、`run_at`、`jitter`、`expires`、`retry_backoff`、`ignore_result`、`acks_late`、`soft_timeout`）；完整配置字段（`cron` / `tags` / `max_instances` / `coalesce` / `timezone` / `misfire_grace_time` / `rate_limit_count` / `rate_limit_window` / `acks_on_failure`）请用 `xhjob_get` |
| `xhjob_result($id, $name="default", $data_dir=null): array` | 查询任务结果（`body`、`status_code`、`stdout`、`stderr`、`exit_code`）；任务在产出结果前失败时返回 `error` 字段，提示查 `xhjob_state()` 的 `last_error` |
| `xhjob_remove(string $id, string $name = "default", ?string $data_dir = null): bool` | 删除 cron 作业定义（不影响运行中实例） |
| `xhjob_pause(string $id, string $name = "default", ?string $data_dir = null): bool` | 暂停 cron 作业（保留定义，不触发） |
| `xhjob_resume(string $id, string $name = "default", ?string $data_dir = null): bool` | 恢复已暂停的 cron 作业 |
| `xhjob_cancel(string $id, string $name = "default", ?string $data_dir = null): bool` | 取消任务（Pending→Cancelled 终态；Running→停止后续重试与 cron 触发，不强制 kill） |
| `xhjob_list(string $name = "default", ?string $state_filter = null, ?string $data_dir = null): string` | 列出任务摘要（JSON 字符串，需 json_decode） |
| `xhjob_requeue(string $id, string $name = "default", ?string $data_dir = null): bool` | 把终态（CANCELLED / FAILED / EXPIRED）任务重新置为 PENDING 重新调度（C7） |
| `xhjob_reschedule(string $id, string $cron, string $name = "default", ?string $data_dir = null): bool` | 在线修改 cron 任务的 cron 表达式，保留 state/execution_count/meta（A11） |
| `xhjob_get(string $id, string $name = "default", ?string $data_dir = null): ?string` | 查询单任务完整 Task JSON（含所有配置字段），任务不存在返回 null（A12） |
| `xhjob_events(int $since_ts, ?string $task_id = null, string $name = "default", ?string $data_dir = null): string` | 查询 `since_ts` 以来任务执行事件流 JSON 数组（A17），含 started/succeeded/failed/cancelled/expired/rate_limited 等类型 |
| `xhjob_chain(string $tasks_json, string $name = "default", ?string $data_dir = null): string` | 创建顺序流水线：daemon 按序执行，任一步失败则中断（C15），返回 chain_id 或 `error: ...` |
| `xhjob_chain_state(string $chain_id, string $name = "default", ?string $data_dir = null): ?string` | 查询 chain 状态 JSON（chain_id / tasks / current_step / state / created_at / updated_at）（C15） |
| `xhjob_group(string $tasks_json, string $name = "default", ?string $data_dir = null): string` | 创建并行批处理：daemon 并发 dispatch 全部任务（C16），返回 group_id 或 `error: ...` |
| `xhjob_group_state(string $group_id, string $name = "default", ?string $data_dir = null): ?string` | 查询 group 状态 JSON + summary 字段（total/succeeded/failed/pending）（C16） |

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
| `every(int $secs): $this` | 设置 IntervalTrigger 周期（A7）；与 cron/runAt 互斥，cron/runAt 优先 |
| `runAt(int $ts): $this` | 设置 DateTrigger 一次性触发时间戳（A8）；优先级最高，触发后立即 SUCCESS 终态 |
| `jitter(int $secs): $this` | 在 cron/interval 的 next_fire 上叠加 [0, secs] 随机偏移（A9），避免惊群；runAt 忽略 |
| `expires(int $secs): $this` | 任务级 Pending 超时（C6）；超时转为 EXPIRED 终态，0=不限（默认） |
| `retryBackoff(bool $on): $this` | 启用指数退避重试（C8）；间隔 = min(retry_delay * 2^(attempts-1), retry_delay * 60) |
| `softTimeout(int $secs): $this` | Shell 任务软超时（C11）；SIGTERM → 宽限期 → SIGKILL；HTTP 任务忽略；>=timeout 时忽略 |
| `misfireGraceTime(int $secs): $this` | per-job 覆盖全局默认 60s misfire 宽限窗口（A13）；0=用全局默认 |
| `id(string $id): $this` | 设置显式 task id（A14），空串视为未设置；与 replaceExisting 配合实现幂等 dispatch |
| `replaceExisting(bool $on): $this` | 已存在同 id 任务时直接替换（A14），state/attempts/execution_count 重置 |
| `tag(string $tag): $this` | 为任务附加一个 tag（A15），可链式调用多次添加多个 tag；空串与重复被忽略 |
| `rateLimit(int $count, int $windowSecs): $this` | 每任务滑动窗口限流（C12）；windowSecs 秒内最多触发 count 次；0=不限 |
| `acksOnFailure(bool $on): $this` | 失败时是否尊重 retry_max（C13）；false=无限重试直到成功；默认 true |
| `ignoreResult(bool $on): $this` | fire-and-forget 模式（C9）；跳过 save_result，xhjob_result 返回 null |
| `acksLate(bool $on): $this` | 延迟确认（C10）；daemon 重启后 RUNNING 任务自动重置为 PENDING 重新执行 |
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
| `XHJOB_MAX_TASKS_PER_CHILD` | `0` | daemon 累计执行 N 次任务后自我退出（C14），由 supervisor 拉起；0=不限。仅在 daemon 启动时读取一次 |
| `XHJOB_MAX_MEMORY_PER_CHILD` | `0` | daemon RSS 超过 N 字节后自我退出（C17），跨平台读取（Linux `/proc/self/status` / macOS `mach_task_basic_info` / Windows `GetProcessMemoryInfo`）；0=不限。仅在 daemon 启动时读取一次 |
| `XHJOB_EVENTS_TTL_SECS` | `86400`（24h） | 任务执行事件流（A17）的保留时长（秒）；超过 TTL 的事件由 daemon 后台 `scan_once` 周期清理（每 60s 一次节流）。预留环境变量，当前实现为常量 |

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
