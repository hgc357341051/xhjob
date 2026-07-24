# 安装与配置

## 概述

本篇覆盖 xhjob 扩展的加载方式、全部环境变量、配置文件、路径解析优先级、服务名校验与 PID 文件格式。配置正确后，daemon 才能稳定常驻并与 PHP 进程正确互通。

## 函数签名 / 方法签名

配置相关无独立函数；以下为受配置影响的运行期 API：

```php
xhjob_start(?string $name = null, ?string $data_dir = null): bool
xhjob_status(?string $name = null, ?string $data_dir = null): array
```

builder 中影响配置上下文的方法：

```php
Xhjob::task()
    ->service(string $name): Xhjob   // 指定服务名
    ->dataDir(string $dir): Xhjob    // 指定统一数据目录
    ->dispatch(): string;
```

## 参数说明

| 参数 | 说明 |
|------|------|
| `$name` / `service($name)` | 服务名，校验 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`，非法回退 `default` |
| `$data_dir` / `dataDir($dir)` | 统一数据目录，覆盖各 `XHJOB_*_DIR` 解析 |

## 返回值

- `xhjob_start`：`bool`。
- `xhjob_status`：键值对数组（`running` / `pid` / `error`）。

## 注意事项

- **env 优先于配置文件**：`/etc/xhjob/config` 的 KEY=VALUE 在 daemon 启动时由 `load_config_file()` 注入进程环境，但**已存在的 env 不被覆盖**。
- **路径 fallback 分两套**：sock_dir 走 `/run/xhjob > /var/run/xhjob > /tmp`；pid_dir / log_dir / db_dir 仅 fallback `/tmp`。
- **dl() 受限**：多数 SAPI（含 FPM，及 `enable_dl=Off` 的 CLI）下 `dl()` 不可用，生产用 `extension=` 永久加载。

## 扩展加载

### 方式一：php.ini 永久加载（推荐）

```ini
; /etc/php/8.2/fpm/conf.d/50-xhjob.ini
extension=/path/to/xhjob-php8.2-linux-x86_64.so
```

CLI 与 FPM 各自的 `conf.d` 都要放（或用 `PHP_INI_SCAN_DIR` 共享同一扫描目录）。

### 方式二：PHP_INI_SCAN_DIR 共享扫描目录

```bash
# 把 xhjob 的 ini 放到独立目录，追加到扫描路径
export PHP_INI_SCAN_DIR=/etc/php/8.2/fpm/conf.d:/etc/xhjob-ini
# 在 /etc/xhjob-ini/50-xhjob.ini 放 extension=/path/to/xhjob-php8.2-linux-x86_64.so
```

### 方式三：命令行临时加载

```bash
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so -m | grep xhjob
php -d extension=/path/to/xhjob-php8.2-linux-x86_64.so \
    -r 'echo function_exists("xhjob_dispatch") ? "ok" : "no";'
```

### dl() 限制说明

`dl()` 在多数 SAPI 下被禁用（`enable_dl=Off`）或不可用，**不可靠**，不要用于加载 xhjob。请使用上述 `extension=` 或 `PHP_INI_SCAN_DIR` 方式。

## 环境变量全表

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `XHJOB_SERVICE_NAME` | `default` | 服务名，命名空间化 sock/pid/log/db 文件 |
| `XHJOB_DATA_DIR` | （无） | 统一数据目录 |
| `XHJOB_DB_DIR` | fallback `/tmp` | SQLite 文件目录 |
| `XHJOB_SOCK_DIR` | `/run/xhjob` > `/var/run/xhjob` > `/tmp` | Unix socket 目录 |
| `XHJOB_PID_DIR` | fallback `/tmp` | PID 目录（不走 /run 链） |
| `XHJOB_LOG_DIR` | fallback `/tmp` | 日志目录（不走 /run 链） |
| `XHJOB_IPC_TIMEOUT_SECS` | `5` | IPC 请求超时（秒） |
| `XHJOB_POOL_MODE` | `async` | `async` / `thread` / `coroutine`(别名) |
| `XHJOB_ASYNC_POOL_SIZE` | `1024` | async 池大小 |
| `XHJOB_COROUTINE_POOL_SIZE` | — | async 池旧别名 |
| `XHJOB_THREAD_POOL_SIZE` | `num_cpus` | thread 池大小 |
| `XHJOB_PERSIST` | `true`(编译启用 persist feature 时) / `false` | 是否持久化 |
| `XHJOB_API_TOKEN` | （空） | API token |
| `XHJOB_CONFIG_FILE` | `/etc/xhjob/config` | KEY=VALUE 配置文件路径 |
| `XHJOB_SHELL_TIMEOUT` | `300` | shell 执行器默认超时（秒） |
| `XHJOB_MAX_PENDING` | `10000` | 待处理任务上限 |
| `XHJOB_MAX_CRON_PER_TICK` | `500` | 每 tick 处理的 cron 上限 |
| `XHJOB_MAX_CONNECTIONS` | `256` | IPC 最大连接数 |
| `XHJOB_SHUTDOWN_DRAIN_SECS` | `30` | 关停排空窗口（秒） |
| `XHJOB_WATCHDOG_INTERVAL` | `5` | watchdog 巡检间隔（秒） |
| `XHJOB_WATCHDOG_FACTOR` | `2` | watchdog 倍数因子 |
| `XHJOB_HTTP_CONNECT_TIMEOUT` | `10` | http 连接超时（秒） |
| `XHJOB_HTTP_MAX_REDIRECTS` | `5` | http 最大重定向 |
| `XHJOB_MAX_TASKS_PER_CHILD` | `0`(无限) | 单 worker 最大任务数 |
| `XHJOB_MAX_MEMORY_PER_CHILD` | `0`(不限) | 单 worker 内存上限 |
| `XHJOB_IPC_NO_PEERCRED` | （未设） | 设 `1` 跳过 `SO_PEERCRED` 校验 |
| `XHJOB_OWNER` | `""` | 多租户属主 |
| `XHJOB_ENCRYPTION_KEY` | （空） | 64 hex(32 字节)，AES-256-GCM 加密任务结果 |

## 配置文件 /etc/xhjob/config

`XHJOB_CONFIG_FILE`（默认 `/etc/xhjob/config`）是 KEY=VALUE 纯文本，daemon 启动时由 `load_config_file()` 加载到进程环境。

```ini
# /etc/xhjob/config
XHJOB_SERVICE_NAME=default
XHJOB_DATA_DIR=/var/lib/xhjob
XHJOB_DB_DIR=/var/lib/xhjob/db
XHJOB_SOCK_DIR=/run/xhjob
XHJOB_PID_DIR=/run/xhjob
XHJOB_LOG_DIR=/var/log/xhjob
XHJOB_POOL_MODE=async
XHJOB_ASYNC_POOL_SIZE=2048
XHJOB_PERSIST=true
XHJOB_API_TOKEN=long-random-token
XHJOB_MAX_PENDING=20000
XHJOB_ENCRYPTION_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

**优先级**：进程已存在的 env > 配置文件 KEY=VALUE。即配置文件不会覆盖已设的环境变量，仅在对应 env 缺失时补位。

## 路径解析优先级

| 目录 | 解析顺序 |
|------|---------|
| sock_dir | `XHJOB_SOCK_DIR` → `/run/xhjob` → `/var/run/xhjob` → `/tmp` |
| pid_dir | `XHJOB_PID_DIR` → `/tmp` |
| log_dir | `XHJOB_LOG_DIR` → `/tmp` |
| db_dir | `XHJOB_DB_DIR` → `/tmp` |

> 注意差异：仅 sock_dir 走 `/run` 链；pid_dir / log_dir / db_dir 找不到时只 fallback 到 `/tmp`。

`XHJOB_DATA_DIR`：统一数据目录，设置后用于统一存放 daemon 数据；与各分项 `XHJOB_*_DIR` 同时存在时的具体覆盖关系以扩展实现为准，生产建议显式设置各分项目录以避免歧义。

## 服务名校验

规则：`^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`

- 首字符必须是字母
- 其余为字母 / 数字 / 下划线 / 连字符
- 长度 1–32
- 非法值**回退到 `default`**，不报错

```
合法：  default  my_service  svc-1  A_b-C
非法：  1svc  svc.1  服务（回退 default）
```

服务名决定文件命名空间：`<name>.sock` / `<name>.pid` / `<name>.log` / `<name>.db`。

## PID 文件格式

双行格式：

```
<pid>
<starttime>
```

- 第 1 行：daemon 进程 PID
- 第 2 行：`starttime`，取自 `/proc/{pid}/stat` 字段 22（进程启动时的时钟滴答），用于**防止 PID 复用**误判——PID 被回收后新进程可能复用同号，starttime 不同即可识别
- 旧格式单行 pid **向后兼容**

## 生产建议

- **socket 落 /run**：设 `XHJOB_SOCK_DIR=/run/xhjob`（tmpfs，重启清空，IPC 低延迟），并配 systemd 的 `RuntimeDirectory=xhjob` 自动创建。
- **db/pid/log 落持久盘**：`XHJOB_DB_DIR=/var/lib/xhjob`、`XHJOB_LOG_DIR=/var/log/xhjob`，避免重启丢 SQLite 与日志。
- **生产开 persist + 加密**：`XHJOB_PERSIST=true` + `XHJOB_ENCRYPTION_KEY=<64 hex>`，结果落盘加密。
- **设 API token**：`XHJOB_API_TOKEN` 防止未授权 IPC；如需跨用户访问再设 `XHJOB_IPC_NO_PEERCRED=1`（降低安全性，谨慎）。
- **配置文件 + env 分层**：通用项写 `/etc/xhjob/config`，实例差异项用 env 覆盖（env 优先）。
