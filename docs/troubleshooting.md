---
title: 故障排查
parent: 排障
nav_order: 61
---

# 故障排查

本篇汇总 xhjob 在安装、启动、派发、运行、恢复各阶段可能遇到的典型问题，按「症状 → 根因 → 解决方法」三段式组织，所有命令与路径均基于真实 API 与默认数据目录 `/run/xhjob`。

---

## 1. 扩展未加载

### 症状

PHP 报错：

```text
Fatal error: Uncaught Error: Call to undefined function Xhjob\xhjob_status()
```

或 `php -m | grep xhjob` 无输出。

### 根因

`xhjob.so` 未被 PHP 加载。常见场景：

- 扩展 `.so` 文件未复制到 PHP 的 `extension_dir`
- `php.ini` 未添加 `extension=xhjob.so`
- CLI 与 FPM 使用不同 ini 文件，只在一边加载
- 使用 `dl()` 动态加载（在新 SAPI 下受限，不可靠）

### 解决方法

**1）显式指定扩展验证加载：**

```bash
# 用 -d 临时加载并检查
php -d extension=/path/to/xhjob.so -m | grep xhjob
```

若有输出，说明 `.so` 本身可用，问题在 ini 配置。

**2）配置额外 ini 扫描目录：**

```bash
# 查看当前 extension_dir
php -i | grep extension_dir

# 写一个独立 ini 文件
cat > /etc/php/8.2/cli/conf.d/50-xhjob.ini <<'EOF'
extension=xhjob.so
EOF

# 或通过环境变量指定扫描目录
export PHP_INI_SCAN_DIR=/etc/php/8.2/cli/conf.d
php -m | grep xhjob
```

**3）关于 `dl()`：**

`dl()` 在 PHP 5.3 起已被废弃，且仅在 CGI/CLI SAPI 下可用，在 FPM/Embed 下不可用。**不要依赖 `dl('xhjob.so')`**，应通过 `php.ini` 或 `conf.d` 静态加载。

---

## 2. daemon 启动失败

### 症状

```php
$ok = xhjob_start();
var_dump($ok); // bool(false)
```

`xhjob_start()` 返回 `false`，daemon 未起来。

### 根因

- socket 目录（默认 `/run/xhjob`）无写权限或被其他用户占用
- `/run/xhjob` 无法创建（`/run` 通常是 tmpfs，需 root 或对应权限）
- 已有同名 service 的 daemon 在跑（端口/socket 占用）

### 解决方法

**1）检查并修复目录权限（0o700）：**

```bash
# 查看目录是否存在及权限
ls -ld /run/xhjob

# 手动创建并设置权限（0o700 = 仅属主可读写执行）
mkdir -p /run/xhjob && chmod 700 /run/xhjob
chown $(id -u):$(id -g) /run/xhjob
```

**2）查看 daemon 日志定位具体错误：**

```bash
# 默认 service 名为 default，日志路径为 /run/xhjob/xhjob.{name}.log
cat /run/xhjob/xhjob.default.log
```

**3）确认是否已有进程占用 socket：**

```bash
# 查看 pid 文件
cat /run/xhjob/xhjob.default.pid

# 检查 socket 文件
ls -l /run/xhjob/xhjob.default.sock

# 若残留旧进程，先清理
kill $(cat /run/xhjob/xhjob.default.pid) 2>/dev/null
rm -f /run/xhjob/xhjob.default.sock /run/xhjob/xhjob.default.pid
```

清理后重新调用 `xhjob_start()`。

---

## 3. `xhjob_dispatch` 返回 `error: ...`

### 症状

```php
$res = xhjob_dispatch('my-service', $taskJson);
// 返回字符串："error: invalid service name"
```

### 根因

`xhjob_dispatch` 在派发前会做参数校验与 daemon 连通性检查，任一失败即返回 `error: <原因>` 前缀字符串。常见三类：

1. **服务名校验失败**：service_name 必须匹配 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`（字母开头，仅含字母数字下划线连字符，长度 1–32）
2. **task_json 非法**：JSON 解析失败或缺少必填字段
3. **daemon 不可达**：socket 不存在或 daemon 未启动

### 解决方法

**解析 error 前缀定位原因：**

```php
$res = xhjob_dispatch($service, $taskJson);
if (is_string($res) && str_starts_with($res, 'error: ')) {
    $reason = substr($res, 7);
    // 根据 $reason 分类处理
    match (true) {
        str_contains($reason, 'service name') => /* 见下文服务名规范 */,
        str_contains($reason, 'json')         => /* 检查 task_json 结构 */,
        str_contains($reason, 'daemon') || str_contains($reason, 'connect') =>
            /* 启动 daemon 或查 status */,
        default => error_log("xhjob_dispatch 未知错误: $reason"),
    };
}
```

**1）服务名规范：**

```php
// 合法
xhjob_dispatch('default', $taskJson);
xhjob_dispatch('myService_1', $taskJson);
xhjob_dispatch('order-pay', $taskJson);

// 非法（数字开头 / 含非法字符 / 过长）
xhjob_dispatch('1service', $taskJson);   // error: 数字开头
xhjob_dispatch('my.service', $taskJson); // error: 含点号
xhjob_dispatch(str_repeat('a', 33), $taskJson); // error: 超长
```

**2）task_json 校验：**

```php
// 先用 PHP 解码自检
$decoded = json_decode($taskJson, true);
if (json_last_error() !== JSON_ERROR_NONE) {
    throw new RuntimeException('task_json 非法: ' . json_last_error_msg());
}
// 确保必填字段存在（type / payload 等）
```

**3）daemon 不可达：**

```php
// 查询 daemon 状态
$status = xhjob_status();
if (!is_array($status) || empty($status)) {
    // daemon 未启动，重新拉起
    xhjob_start();
}
```

---

## 4. 任务卡 Running

### 症状

任务状态长时间停留在 `running`，超过设定的 timeout 仍未变 `failed` 或 `success`。

### 根因

子进程在 IO 上 hang（典型场景：FIFO 管道阻塞、NFS 卡顿、DNS 解析不返回），**此场景不触发 tokio 的 timeout**——tokio timeout 只对 async future 的 await 点生效，对底层 C 级阻塞 IO 无能为力。

### 解决方法

**1）配置 watchdog 假死检测：**

watchdog 是 **daemon 级**机制（非单任务配置），通过环境变量在 daemon 启动时设定：扫描间隔 `XHJOB_WATCHDOG_INTERVAL`（默认 5s），阈值因子 `XHJOB_WATCHDOG_FACTOR`（默认 2）。当 running 任务的耗时超过 `timeout * factor` 时，watchdog 判定假死并强制取消。

```bash
# 启动 daemon 前设置（factor=2 时，timeout=30s 的任务超过 60s 即被判假死）
export XHJOB_WATCHDOG_INTERVAL=5
export XHJOB_WATCHDOG_FACTOR=2
```

```php
// 任务侧仅需设 timeout，watchdog 阈值 = timeout * factor（30 * 2 = 60s）
$taskId = TaskBuilder::shell('curl http://slow-host')
    ->timeout(30)                          // 任务超时 30s
    ->dispatch();
```

> watchdog 没有对应的 TaskBuilder 方法——它由 daemon 全局配置驱动，对所有 running 任务统一生效。

**2）手动取消卡死任务：**

```php
// 取消指定任务
$ok = xhjob_cancel($taskId);
if ($ok) {
    echo "任务 $taskId 已取消\n";
}
```

**3）检查 `worker_pid` lease：**

```php
// 查询任务详情，确认 worker_pid 是否还活着
$state = xhjob_state($taskId);
if (isset($state['worker_pid'])) {
    $pid = $state['worker_pid'];
    // 在 shell 检查进程
    // ps -p $pid -o pid,stat,cmd
}
```

若 `worker_pid` 已不存在但任务仍 `running`，说明 lease 未被回收，需 watchdog 介入或手动 `xhjob_cancel`。

---

## 5. SQLite 损坏

### 症状

- daemon 启动报错（如 `database disk image is malformed`）
- 查询返回异常数据或 `xhjob_status()` 失败
- 日志中出现 SQLite 错误

### 根因

- 磁盘满（写入未完整落盘）
- 硬件故障 / 断电导致 WAL 损坏
- 文件系统异常

### 解决方法

**1）启动时自动检测：**

daemon 启动时会执行 `PRAGMA quick_check` 自动检测 SQLite 完整性，若失败会在日志中输出。查看日志：

```bash
cat /run/xhjob/xhjob.default.log | grep -i -E 'sqlite|corrupt|pragma'
```

**2）备份 db 文件：**

```bash
# 先备份再处理，避免误删
cp /run/xhjob/xhjob.default.db /tmp/backup.db
# 同时备份 WAL 与 SHM（若存在）
cp /run/xhjob/xhjob.default.db-wal /tmp/backup.db-wal 2>/dev/null
cp /run/xhjob/xhjob.default.db-shm /tmp/backup.db-shm 2>/dev/null
```

**3）手动校验：**

```bash
sqlite3 /run/xhjob/xhjob.default.db "PRAGMA quick_check;"
sqlite3 /run/xhjob/xhjob.default.db "PRAGMA integrity_check;"
```

**4）删除重建（数据丢失）：**

```bash
# 停掉 daemon
kill $(cat /run/xhjob/xhjob.default.pid) 2>/dev/null

# 删除损坏的 db 及附属文件
rm -f /run/xhjob/xhjob.default.db
rm -f /run/xhjob/xhjob.default.db-wal
rm -f /run/xhjob/xhjob.default.db-shm

# 重新启动 daemon，会自动建新库
php -r 'xhjob_start();'
```

---

## 6. FPM worker 阻塞

### 症状

- Nginx 返回 502 / 504
- FPM worker 数被占满（`pm.status` 显示全部 busy）
- Web 请求长时间无响应

### 根因

daemon 死锁或卡死后，FPM worker 在 `read_exact`（等待 daemon 响应）上永久阻塞。**PHP 的 `max_execution_time` 不会中断 C 级阻塞调用**（如 socket read），因此 worker 不会被回收。

### 解决方法

**1）调小 `XHJOB_IPC_TIMEOUT_SECS`（让 worker fail fast）：**

```bash
# 默认 5s，调到 2s 让 FPM worker 快速失败
export XHJOB_IPC_TIMEOUT_SECS=2

# 在 FPM pool 配置中设置环境变量（/etc/php/8.2/fpm/pool.d/www.conf）
# env[XHJOB_IPC_TIMEOUT_SECS] = 2
```

或在 PHP 启动阶段设置：

```php
// 仅在 CLI/CLI-like 场景生效，FPM 需通过 pool.conf 的 env[...] 注入
putenv('XHJOB_IPC_TIMEOUT_SECS=2');
```

调小后，`xhjob_dispatch` / `xhjob_state` 等 IPC 调用会在 2s 内超时返回，FPM worker 不会被永久占用。

**2）重启 FPM 释放被占 worker：**

```bash
# 重启 FPM（仅紧急情况下使用，会中断在跑请求）
sudo systemctl reload php8.2-fpm
# 或
sudo systemctl restart php8.2-fpm
```

**3）根治：排查 daemon 死锁原因**

```bash
# 查看 daemon 日志
tail -100 /run/xhjob/xhjob.default.log

# 查看 daemon 线程栈（若有 gdb）
gdb -p $(cat /run/xhjob/xhjob.default.pid) -batch -ex 'thread apply all bt'
```

---

## 7. persist feature 未启用

### 症状

- daemon 崩溃后任务全部丢失
- lease（worker_pid + starttime）失效，任务可能重复执行
- watchdog / crash-recovery 不生效

### 根因

编译 daemon 时未启用 `persist` feature，运行时 fallback 到 `InMemoryStore`——所有状态仅在内存中，进程退出即丢失。

### 解决方法

**1）确认当前是否启用持久化：**

```bash
# 检查环境变量
echo $XHJOB_PERSIST

# 检查 db 文件是否存在（启用 persist 时会有 db 文件）
ls -l /run/xhjob/xhjob.default.db
```

若 `XHJOB_PERSIST` 未设置或 db 文件不存在，说明未启用。

**2）重新编译 daemon（启用 all-features）：**

```bash
cd /path/to/xhjob

# 用 --all-features 编译，包含 persist（SQLite + WAL）
cargo build --release --all-features

# 编译产物通常在 target/release/xhjob-daemon（或对应二进制名）
ls -l target/release/
```

**3）设置环境变量并重启 daemon：**

```bash
# 启用持久化
export XHJOB_PERSIST=1

# 重启 daemon（先停旧的）
kill $(cat /run/xhjob/xhjob.default.pid) 2>/dev/null
xhjob_start  # 或通过 PHP 调用 xhjob_start()
```

启用后 `/run/xhjob/xhjob.default.db` 会被创建，崩溃恢复、lease、watchdog 才会真正生效。

---

## 8. zombie 进程残留

### 症状

```bash
ps -ef | grep xhjob
# 看到 defunct 标记
# user  1234  1233  0  10:00 ?        00:00:00 [xhjob-daemon] <defunct>
```

### 根因

测试环境中 daemon 常作为 PHP 脚本的子进程启动，当用 `kill -9` 强杀 daemon 后，daemon 未被父进程（PHP 脚本）`wait` 回收，进入 zombie 状态。

> **注意**：生产环境 daemon 通过 `setsid` 脱离父进程（成为 session leader），由 init/systemd 接管，**不会出现此问题**。此问题仅出现在测试/开发环境直接 fork 的场景。

### 解决方法

**1）由父进程 reap（推荐）：**

若 PHP 父进程仍在运行，调用 `pcntl_waitpid` 回收 zombie：

```php
// 阻塞回收所有子进程
while (($pid = pcntl_waitpid(-1, $status, WNOHANG)) > 0) {
    echo "已回收子进程 $pid\n";
}

// 或针对特定 pid 阻塞回收
// $daemonPid = (int) file_get_contents('/run/xhjob/xhjob.default.pid');
// pcntl_waitpid($daemonPid, $status);
```

**2）kill 父进程（PHP 脚本已退出但 zombie 残留）：**

```bash
# 找到 zombie 的父进程
ps -o ppid= -p <zombie_pid>

# kill 父进程，zombie 会被 init/systemd 接管回收
kill -9 <parent_pid>
```

**3）生产环境根治：**

生产环境应通过 systemd unit 或 supervisor 启动 daemon，daemon 内部调用 `setsid()` 脱离控制终端与父进程，成为独立 session：

```bash
# systemd unit 示例（/etc/systemd/system/xhjob.service）
# [Service]
# Type=forking
# PIDFile=/run/xhjob/xhjob.default.pid
# ExecStart=/usr/local/bin/xhjob-daemon --service default
# Restart=on-failure
```

这样 daemon 由 systemd 直接管理，kill 后由 systemd 回收，不会产生 zombie。

---

## 排障 Checklist

遇到问题时按以下顺序快速定位：

| 现象 | 第一步检查 |
|------|-----------|
| 函数未定义 | `php -m \| grep xhjob` |
| `xhjob_start` 返回 false | `ls -ld /run/xhjob` + `cat /run/xhjob/xhjob.default.log` |
| dispatch 返回 error | 解析 `error:` 前缀字符串 |
| 任务卡 Running | `xhjob_status()` 看 daemon 是否健康 + `xhjob_cancel($id)` |
| daemon 启动报 SQLite 错 | `sqlite3 .../xhjob.default.db "PRAGMA quick_check;"` |
| FPM 502/504 | 调小 `XHJOB_IPC_TIMEOUT_SECS` |
| 崩溃后任务全丢 | 确认 `XHJOB_PERSIST=1` + db 文件存在 |
| ps 看到 defunct | `pcntl_waitpid` reap 或 kill 父进程 |

如以上步骤均无法解决，请提交 [GitHub Issue](https://github.com/hgc357341051/xhjob/issues) 并附上 `/run/xhjob/xhjob.default.log` 日志与 `xhjob_status()` 输出。
