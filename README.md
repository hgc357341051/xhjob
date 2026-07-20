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

### 函数 API（服务名参数）

所有顶层函数均接受可选的服务名参数：

```php
xhjob_start(string $name = "default"): bool
xhjob_stop(string $name = "default"): bool
xhjob_restart(string $name = "default"): bool
xhjob_status(string $name = "default"): array
xhjob_dispatch(string $task_json, string $name = "default"): string
xhjob_state(string $id, string $name = "default"): array
xhjob_result(string $id, string $name = "default"): array
```

### 链式 API（服务名绑定）

通过 `Xhjob::service($name)->task()->...` 将 builder 绑定到指定服务：

```php
<?php
xhjob_start('cron-svc');
xhjob_start('queue-svc');

// dispatch 到 cron-svc
$id1 = Xhjob::service('cron-svc')->task()
    ->viaShell('echo hello-from-cron-svc')
    ->dispatch();

// dispatch 到 queue-svc
$id2 = Xhjob::service('queue-svc')->task()
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

## API 参考

### 顶层函数

| 函数 | 说明 |
|------|------|
| `xhjob_start($name="default", $data_dir=null): bool` | 启动（或确认已启动）指定服务的 daemon，可选 data_dir |
| `xhjob_stop($name="default", $data_dir=null): bool` | 停止指定服务的 daemon |
| `xhjob_restart($name="default", $data_dir=null): bool` | 重启指定服务的 daemon |
| `xhjob_status($name="default", $data_dir=null): array` | 查询 daemon 运行状态（`running`、`pid`） |
| `xhjob_dispatch($task_json, $name="default", $data_dir=null): string` | 通过 JSON 字符串 dispatch 任务，返回 task_id |
| `xhjob_state($id, $name="default", $data_dir=null): array` | 查询任务状态（`state`、`attempts`、`created_at`、`started_at`、`finished_at`、`last_error`） |
| `xhjob_result($id, $name="default", $data_dir=null): array` | 查询任务结果（`body`、`status_code`、`stdout`、`stderr`、`exit_code`） |

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
| `cron(string $expr): $this` | 设置 cron 表达式（5 或 6 段） |
| `timeout(int $secs): $this` | 设置执行超时（秒） |
| `priority(int $p): $this` | 设置任务优先级（数值越大越先执行） |
| `allowOverlap(bool $allow): $this` | 是否允许同一任务并发执行 |
| `maxInstances(int $n): $this` | 最大并发实例数 |
| `coalesce(bool $c): $this` | 是否合并错过的触发 |
| `persist(bool $p): $this` | 是否启用 SQLite 持久化 |
| `dispatch(): string` | 提交任务到 daemon，返回 task_id |

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `XHJOB_DATA_DIR` | 平台默认 | 统一数据目录（PID/sock/db/log 同时落入此目录），优先级低于细粒度变量；也是 daemon 子进程内的当前数据目录（由父进程自动设置，函数参数可覆盖） |
| `XHJOB_PID_DIR` (Unix) | `/tmp` | PID 文件目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_SOCK_DIR` (Unix) | `/tmp` | IPC socket 目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_DB_DIR` (Unix) | `/tmp` | SQLite 数据库目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_LOG_DIR` (Unix) | `/tmp` | 日志文件目录（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_SERVICE_NAME` | `default` | daemon 子进程内当前服务名（由父进程自动设置） |
| `XHJOB_PERSIST` | `0` | 设为 `1` 或 `true` 时 daemon 启用 SQLite 持久化存储 |
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
