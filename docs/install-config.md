---
title: 安装与配置
parent: 入门
nav_order: 13
---

# 安装与配置

本页覆盖 xhjob 扩展的加载方式、全部环境变量、配置文件、运行目录路径解析、服务名校验规则与 PID 文件格式。

## 扩展加载三种方式

xhjob 是一个标准 PHP 扩展（`.so`），加载方式与普通扩展一致。`dl()` 在新 SAPI（PHP-FPM、较新 CLI）下受限，**推荐以下三种方式**：

### 1. `php.ini` 直接加载

在 `php.ini` 中添加：

```ini
; 写入 php.ini（CLI 与 FPM 各自的 ini 文件需分别配置）
extension=xhjob.so
; 或使用绝对路径，避免 extension_dir 配置差异导致找不到
; extension=/opt/xhjob/xhjob.so
```

适用于：所有 SAPI（CLI、FPM、 FrankenPHP 等）都需使用的场景。

### 2. `PHP_INI_SCAN_DIR` 额外 ini 目录

把扩展配置放到独立 ini 文件，通过扫描目录加载，便于与主 `php.ini` 解耦：

```bash
# 1. 创建独立 ini 文件
cat > /etc/php/8.2/mods-available/xhjob.ini <<'EOF'
; xhjob 扩展配置
extension=xhjob.so
EOF

# 2. 通过 PHP_INI_SCAN_DIR 让 PHP 扫描该目录
export PHP_INI_SCAN_DIR=/etc/php/8.2/mods-available
php -m | grep xhjob
```

适用于：扩展作为可选模块按需启用，不想污染主 `php.ini`。

### 3. 命令行 `-d extension=`

临时加载，不修改任何 ini 文件：

```bash
# 单次命令临时加载
php -d extension=/opt/xhjob/xhjob.so -m | grep xhjob

# 运行脚本
php -d extension=/opt/xhjob/xhjob.so my_worker.php
```

适用于：调试、一次性脚本、CI 环境。

### 关于 `dl()`

`dl()` 仅在部分旧 CLI SAPI 下可用，且在新 SAPI 受限（PHP-FPM 下完全禁用）。**不要在生产环境依赖 `dl()` 加载 xhjob**，优先用上述 `extension=` ini 指令或 `-d extension=` 命令行参数。

## 环境变量

所有 `XHJOB_*` 环境变量在 daemon 启动时读取，部分也可被 PHP 端读取用于解析路径。下表为完整清单：

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `XHJOB_SERVICE_NAME` | `default` | 服务名，决定 PID/sock/db/log 文件命名与多实例隔离 |
| `XHJOB_DATA_DIR` | `null` | 统一数据目录，设置后 PID/sock/db/log 全部置于其下 |
| `XHJOB_SOCK_DIR` | 见路径解析 | Unix 下 IPC socket 目录的细分覆盖（优先级高于 `XHJOB_DATA_DIR`） |
| `XHJOB_PID_DIR` | 见路径解析 | Unix 下 PID 文件目录的细分覆盖 |
| `XHJOB_LOG_DIR` | 见路径解析 | Unix 下日志文件目录的细分覆盖 |
| `XHJOB_IPC_TIMEOUT_SECS` | `5` | IPC 请求超时秒数，防 daemon 死锁后 FPM worker 永久阻塞 |
| `XHJOB_POOL_MODE` | `async` | 线程池模式：`async`（tokio M:N，IO 密集）或 `thread`（1:1，CPU 密集）；`coroutine` 为 `async` 的兼容别名 |
| `XHJOB_ASYNC_POOL_SIZE` | `1024` | async 模式最大并发数（推荐变量名） |
| `XHJOB_COROUTINE_POOL_SIZE` | `1024` | async 模式最大并发数的兼容别名，与上面等价 |
| `XHJOB_THREAD_POOL_SIZE` | CPU 核数 | thread 模式工作线程数 |
| `XHJOB_PERSIST` | 未启用 | 非 0 值启用 SQLite 持久化；需 `--all-features` 编译，否则回退到 InMemory |
| `XHJOB_API_TOKEN` | 未配置 | HTTP 鉴权 token（ThinkPHP 中间件用）；未配置时中间件抛 500（fail-closed，拒绝所有请求） |
| `XHJOB_CONFIG_FILE` | `/etc/xhjob/config` | 配置文件路径 |
| `XHJOB_SHELL_TIMEOUT` | `300` | shell 任务默认超时秒数（任务级 `timeout()` 会覆盖此默认值） |
| `XHJOB_MAX_PENDING` | `10000` | 待执行任务队列上限，超过则拒绝入队，防 cron 风暴打爆内存 |

> **`XHJOB_IPC_TIMEOUT_SECS` 为何重要**：`max_execution_time` 无法中断 C 级阻塞调用。若 daemon 死锁或被 SIGSTOP，没有这个 IPC 超时会让 FPM worker 永久阻塞在 `read_exact`，逐个耗尽 worker 池直至 502/504 且无法自愈。

> **`XHJOB_API_TOKEN`** 仅用于 ThinkPHP HTTP 端点的中间件鉴权（请求需带 `X-Xhjob-Token` header）。未配置时中间件 **fail-closed** 抛 `HttpException(500)`，避免无鉴权 RCE。

## 配置文件 `/etc/xhjob/config`

除环境变量外，xhjob 还支持一个简单的 `KEY=VALUE` 配置文件（类似 `.env`），在 daemon 启动时加载到进程环境。

- 默认路径：`/etc/xhjob/config`（可用 `XHJOB_CONFIG_FILE` 覆盖）
- 格式：每行一个 `KEY=VALUE`，`#` 开头为注释，空行忽略，VALUE 两侧的引号会被剥离
- **优先级**：已存在的环境变量优先级更高——配置文件只填充**尚未设置**的环境变量，因此命令行 `-e` 注入的 env 始终覆盖配置文件

示例配置文件：

```ini
# Xhjob daemon 配置
XHJOB_PERSIST=1
XHJOB_SOCK_DIR=/run/xhjob
XHJOB_MAX_PENDING=5000
RUST_LOG=xhjob=debug
```

读取逻辑在 `src/config.rs::load_config_file`，daemon 启动时调用一次（在任何 `env::var` 读取之前）。文件不存在时静默跳过（向后兼容）。

## 运行目录路径解析

PID / sock / db / log 文件的目录按以下优先级解析（从高到低）：

| 优先级 | 来源 | 说明 |
|--------|------|------|
| 1 | 显式参数 | `xhjob_start($name, $dataDir)` 或 `XhjobService` 构造时传入的 `data_dir` |
| 2 | 细分 env（Unix） | `XHJOB_SOCK_DIR` / `XHJOB_PID_DIR` / `XHJOB_LOG_DIR`，分别覆盖对应文件类型 |
| 3 | `XHJOB_DATA_DIR` | 统一数据目录，设置后所有运行时文件置于其下 |
| 4 | 平台默认 | Unix：`/run/xhjob` > `/var/run/xhjob` > `/tmp`；Windows：`%TEMP%` |

### 平台默认目录与 symlink 防护

Unix 下若未配置任何目录，fallback 顺序为：

1. `/run/xhjob`（systemd tmpfs，优先）
2. `/var/run/xhjob`（无 `/run` 时）
3. `/tmp`（最后兜底，安全性较低）

**`/run/xhjob` 与 `/var/run/xhjob` 在创建时设为 `0o700` 权限**，防止 symlink 攻击与同名占位。`/tmp` 是全局可写且带 sticky bit，存在 symlink / 名称占位风险，仅作最后兜底。

文件命名规则：

| 文件 | Unix 路径 | Windows 路径 |
|------|----------|-------------|
| IPC socket | `<dir>/xhjob.{name}.sock` | `\\.\pipe\xhjob-{name}` |
| PID 文件 | `<dir>/xhjob.{name}.pid` | `%TEMP%\xhjob.{name}.pid` |
| 日志文件 | `<dir>/xhjob.{name}.log` | `%TEMP%\xhjob.{name}.log` |
| SQLite DB | `<dir>/xhjob.{name}.db` | `%TEMP%\xhjob.{name}.db` |

> Windows 命名管道位于内核独立命名空间，不使用文件系统路径，因此 `data_dir` 对 socket 路径无效（但对 PID/log/db 仍生效）。

## 服务名校验规则

服务名用于隔离多实例 daemon，校验规则（见 `src/service/mod.rs::validate`）：

```
^[a-zA-Z][a-zA-Z0-9_-]{0,31}$
```

- 最长 **32 字符**（首字符 + 后续最多 31 个）
- **首字符必须是字母**（`a-z` / `A-Z`）
- 后续字符允许：字母、数字、下划线 `_`、连字符 `-`
- 不允许：空字符串、以数字/下划线/连字符开头、含 `.` `/` 空格 或非 ASCII 字符

校验示例：

| 名称 | 是否合法 | 原因 |
|------|---------|------|
| `default` | ✅ | 合法 |
| `cron-svc` | ✅ | 合法 |
| `queue_svc` | ✅ | 合法 |
| `a1B2c3` | ✅ | 合法 |
| `1abc` | ❌ | 首字符为数字 |
| `_abc` | ❌ | 首字符为下划线 |
| `-abc` | ❌ | 首字符为连字符 |
| `abc.def` | ❌ | 含 `.` |
| `abc/def` | ❌ | 含 `/`（防路径穿越） |
| `中` | ❌ | 非 ASCII |
| 32 字符以上 | ❌ | 超长 |

> 校验不仅应用于显式传入的服务名，也应用于 `XHJOB_SERVICE_NAME` 环境变量回退路径：若 env 值非法，会打印告警并回退到 `default`，避免恶意 env 含 `../` 造成路径穿越。

## PID 文件格式

PID 文件用于判断 daemon 是否存活，并防护 PID 复用。支持两种格式：

### 新格式（双行，推荐）

```
<pid>
<starttime>
```

- 第 1 行：daemon 进程 PID
- 第 2 行：进程 starttime（Linux `/proc/<pid>/stat` 第 22 字段，单位 clock ticks）

读取方会用 `is_process_alive_with_starttime(pid, Some(starttime))` 校验：PID 存活但 starttime 不匹配，说明 PID 已被无关进程复用，判定 daemon 已死并清理 stale PID 文件。停止时也只在 starttime 已记录时才允许升级到 SIGKILL（fail-safe，避免误杀复用 PID 的无辜进程）。

### 旧格式（单行，向后兼容）

```
<pid>
```

仅一行 PID，无 starttime。读取时 `starttime = None`，退化为普通 `kill(pid, 0)` 存活判断，**不防 PID 复用**。停止超时后也**不会**升级到 SIGKILL（无法证明仍存活的 PID 是同一个 daemon），需人工介入排查。

### 何时写哪种格式

- daemon 启动时调用 `process_starttime(self_pid)` 获取 starttime；Linux 上返回 `Some`，写入双行格式
- 非 Linux 平台（无 `/proc`）返回 `None`，写入单行旧格式
- 两种格式读取方都能正确解析（旧格式兼容）

## 下一步

- 跑通第一个任务：[快速开始](quickstart/)
- 理解整体设计：[架构概览](architecture/)
