# 持久化与崩溃恢复

> 核心能力篇 · SQLite + WAL 持久化 · daemon SIGKILL 后自愈 · 假死检测 · PID 复用防护 · 结果加密

Xhjob 的「持久化崩溃恢复」是它区别于一般内存任务队列的核心能力。本文档按 **签名 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构，逐项展开 9 个关键机制。

---

## 1. persist(true) — 启用 SQLite + WAL 持久化

### 签名

```php
// TaskBuilder（self 链式）
public function persist(bool $on = true): self
```

### 行为说明

`persist(true)` 让单个任务落到 SQLite + WAL 持久化存储：任务定义、状态机迁移、执行结果、事件流全部写入磁盘。daemon 重启后能从 DB 恢复所有未完成任务，而不会丢失。

### 注意事项

- 持久化能力是 **编译时 feature**，必须用 `cargo build --all-features`（或 `--features persist`）编译扩展与 daemon，否则 `persist(true)` 调用会被接受但 **回退到 InMemoryStore**，并在 daemon 日志输出告警：
  ```
  WARN xhjob::store] persist feature not enabled, fallback to InMemoryStore
  ```
- InMemoryStore 回退后，daemon 进程一旦退出，**所有未完成任务全部丢失**，`acksLate(true)` 也会失效（无持久化可恢复）。
- WAL 模式启用 `PRAGMA journal_mode=WAL`，读写不互相阻塞，但要求 `data_dir` 所在分区可写且支持 mmap。

### 代码演示

```php
<?php
// 派发一个启用持久化的任务
$taskId = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('tar -czf /backup/www.tgz /var/www')
        ->timeout(300)
        ->persist(true)        // 落 SQLite + WAL
        ->acksLate(true)       // daemon 重启时重置为 Pending 重新派发
        ->toJson()
);

echo "task_id = {$taskId}\n";
```

### 生产建议

- 编译产物分发时，**始终发布 `--all-features` 版本**，并在启动脚本里用 `xhjob_inspect('stats')` 校验 `store_backend` 字段是否为 `sqlite`，若为 `in_memory` 立即告警。
- `data_dir` 单独挂载到 SSD / 高 IOPS 云盘，避免与业务日志共用 inode。

---

## 2. XHJOB_PERSIST 环境变量 — 全局持久化开关

### 签名

```bash
# 环境变量（无 PHP 签名）
XHJOB_PERSIST=true|false|0|1
```

### 行为说明

`XHJOB_PERSIST` 控制 daemon 启动时的默认存储后端，与编译时 feature 联动：

| 编译 feature | `XHJOB_PERSIST` 取值 | 实际行为 |
|---|---|---|
| persist 开（`--all-features`） | 未设置 | 默认 `true`，使用 SQLite |
| persist 开 | `"0"` / `"false"` | **禁用** 持久化，回退 InMemoryStore（不告警，用户显式选择） |
| persist 关 | 未设置 | 默认 `false`，InMemoryStore |
| persist 关 | `"1"` / `"true"` | **强制启用**，但 feature 关时无 SQLite 实现 → 仅打印告警并回退到 InMemoryStore |

### 注意事项

- `XHJOB_PERSIST=1` 在 feature 关闭时 **不会** 让持久化真正生效，它只是触发一条 `WARN` 告警并回退；不要把它当成「强制开关」。
- 单任务级 `persist(true)` 优先级高于环境变量：即使 `XHJOB_PERSIST=false`，调用了 `persist(true)` 的任务仍尝试落盘（若 feature 开启）。
- 环境变量在 daemon 启动时读取一次，运行期修改不生效。

### 代码演示

```bash
# 编译时启用 persist feature
cargo build --release --all-features

# 启动 daemon：显式开启持久化（默认就是 true，这里只是强调）
XHJOB_PERSIST=true php -d extension=./xhjob.so your-app.php

# 编译时未启用 feature，强制开启 → 仅告警回退
XHJOB_PERSIST=1 php -d extension=./xhjob.so your-app.php
# daemon 日志：
# WARN xhjob::store] XHJOB_PERSIST=1 but persist feature not compiled, fallback to InMemoryStore
```

### 生产建议

- 部署清单（runbook）里写死 `XHJOB_PERSIST=true`，并在容器健康检查里调用 `xhjob_inspect('stats')` 断言 `store_backend == "sqlite"`，否则容器标记为 unhealthy。

---

## 3. acksLate(true) — 延迟确认与崩溃恢复

### 签名

```php
public function acksLate(bool $on = true): self
public function acksOnFailure(bool $on = true): self
```

### 行为说明

`acksLate(true)` 启用「延迟确认」语义：任务进入 `Running` 后**不立即从队列移除**，只有任务终态（success/failed/expired）确认后才移除。daemon 因 SIGKILL / OOM / 断电崩溃后，重启流程会执行 `reset_running_to_pending`：把所有 `Running` 任务重置为 `Pending` 重新派发。

`acksOnFailure(false)` 配合使用时，失败任务不会从队列移除，等于「无限重试」直到成功或过期。

### 注意事项

- **仅 `persist=true` 时生效**。InMemoryStore 下 daemon 一死全部丢失，无从重置。
- 可能产生 **重复执行**：任务实际上已经跑完，但崩溃发生在写终态之前，重启后会重跑。业务侧必须 **幂等**。
- `reset_running_to_pending` 会被 lease check 进一步过滤（见第 4 节），避免重跑仍在执行的任务。

### 代码演示

```php
<?php
// 幂等的报表生成任务：崩溃后自动重跑
$taskId = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('php /app/bin/generate-report.php --date=$(date +%F) --idempotent')
        ->timeout(600)
        ->persist(true)
        ->acksLate(true)            // 崩溃后重置为 Pending 重派
        ->acksOnFailure(false)      // 失败不确认 → 无限重试直到成功
        ->withRetry(3, 5)
        ->toJson()
);
```

### 生产建议

- 所有 `acksLate(true)` 任务**必须幂等**：用唯一键 + UPSERT，或前置 `SELECT ... FOR UPDATE` 抢锁。
- 对不可重做的副作用（发短信、扣款），把幂等键写到任务 `meta`，业务脚本先查再执行。

---

## 4. execution_lease — 防重复执行的租约

### 签名

```php
// 内部机制，无 PHP API；通过事件流可观测
// EventType::LeaseHeld = "lease_held"
```

### 行为说明

任务被 spawn 到 worker 时，**同步**（非异步）写入 `execution_lease` 记录：

```
execution_lease {
    task_id,
    worker_pid,         // /proc/self/stat 字段 1
    worker_starttime,   // /proc/{pid}/stat 字段 22（进程启动时钟 ticks）
}
```

崩溃恢复时，对每条 `Running` 任务执行 lease check：

1. 读取 `worker_pid` 与 `worker_starttime`；
2. 若 `/proc/{worker_pid}/stat` 字段 22 仍等于 `worker_starttime` → **该 worker 仍存活**，跳过重新入队，记录 `LeaseHeld` 事件防止重复执行；
3. 若 PID 已不存在，或 starttime 不匹配（PID 被复用）→ 视为孤儿，重置为 `Pending` 重新派发。

### 注意事项

- `lease_held` 事件名是 **带下划线** 的（早期 serde `rename_all = "lowercase"` 会错写成 `leaseheld`，现已改为手写实现）。
- starttime 是内核时钟 ticks，**跨重启不会复用**，是 PID 复用防护的核心依据。
- 如果 worker 进程被 `kill -STOP` 暂停（而非崩溃），lease 仍判活，任务不会被重派 —— 用 watchdog 处理这种假死。

### 代码演示

```php
<?php
// 观察 LeaseHeld 事件：daemon 重启后，仍存活的 worker 不会被重派
$since = time() - 3600;
$events = json_decode(xhjob_pull_events($since, 'lease_held'), true);
foreach ($events as $ev) {
    echo "[lease_held] task={$ev['task_id']} ts={$ev['ts']} payload={$ev['payload']}\n";
}
```

### 生产建议

- 监控 `lease_held` 事件频率：频繁出现说明 daemon 频繁重启或 worker 长寿命任务多，应排查稳定性。
- 不要手动 `kill -9` worker 进程绕过 lease；用 `xhjob_cancel($taskId)` 走正规取消通道。

---

## 5. watchdog — 假死检测

### 签名

```bash
# 环境变量
XHJOB_WATCHDOG_INTERVAL=5     # 巡检间隔（秒），0 = 禁用
XHJOB_WATCHDOG_FACTOR=2       # 阈值系数：runtime > timeout * factor 判定假死
```

### 行为说明

watchdog 线程按 `XHJOB_WATCHDOG_INTERVAL`（默认 5 秒）巡检所有 `Running` 任务。若某任务已运行时长 `> timeout * XHJOB_WATCHDOG_FACTOR`（默认 2 倍 timeout）仍未完成，判定为假死：

1. 向 worker 发送取消信号（SIGTERM → 升级 SIGKILL）；
2. 任务状态标记为 `Interrupted`；
3. 记录 `HungDetected` 事件，payload 含 `runtime` / `timeout` / `factor`。

### 注意事项

- `XHJOB_WATCHDOG_INTERVAL=0` **禁用** watchdog，此时假死任务会一直占着 worker，依赖 lease 续命但永不完成 —— 生产环境**不建议**禁用。
- watchdog 只对**有 timeout** 的任务生效；`timeout(0)`（无超时）的任务不会被判定假死。
- `hung_detected` 是带下划线的事件名（同 `lease_held`，手写实现避免 serde lowercase 误写）。

### 代码演示

```bash
# 加严假死判定：3 秒巡检，1.5 倍 timeout 即判假死
XHJOB_WATCHDOG_INTERVAL=3 XHJOB_WATCHDOG_FACTOR=1.5 \
  php -d extension=./xhjob.so your-app.php
```

```php
<?php
// 任务：5 秒超时，watchdog 默认 2x = 10 秒无完成判假死
xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('sleep 30')   // 故意 hang
        ->timeout(5)
        ->persist(true)
        ->toJson()
);

// 约 10 秒后会看到 hung_detected + interrupted 事件
$events = json_decode(xhjob_pull_events(time() - 60, 'hung_detected'), true);
print_r($events);
```

### 生产建议

- IO 密集任务 `factor` 调到 3~5（网络抖动容忍）；CPU 密集任务 `factor` 设 1.5。
- 把 `hung_detected` 事件接入告警，单任务连续 3 次假死 → 触发 PagerDuty。

---

## 6. PID 复用防护 — 双行 PID 文件

### 签名

```
# PID 文件格式（双行）
{pid}\n
{starttime}\n
```

### 行为说明

daemon 启动时写 PID 文件（默认 `/run/xhjob.pid`），格式为**双行**：

```
12345
8394832
```

第一行是 daemon PID，第二行是 `/proc/{pid}/stat` 字段 22（starttime，时钟 ticks）。读取时**双校验**：

1. PID 存在；
2. 该 PID 当前 starttime == 文件中 starttime。

只要任一不匹配，视为「stale pid」（旧 daemon 已死，PID 被复用），直接清理并启动新 daemon。

### 注意事项

- 不能只校验 PID：Linux 会复用 PID，旧 daemon 死后新进程可能拿到同样的 PID。
- starttime 单调递增，跨重启**必然不同**，是可靠的存活证据。
- 容器环境下 `/proc` 仍是真实 PID namespace，starttime 仍然有效。

### 代码演示

```bash
# 模拟 daemon 异常退出后的 stale pid 清理
$ cat /run/xhjob.pid
12345
8394832

# 旧 daemon 已死，12345 被一个 nginx 进程复用
$ ps -p 12345 -o pid,comm
  PID COMM
12345 nginx

# 新 daemon 启动 → 读 PID 文件 → starttime 不匹配 → 视为 stale → 清理
$ XHJOB_PERSIST=true php -d extension=./xhjob.so your-app.php
# daemon 日志：
# INFO xhjob::pid] stale pid 12345 (starttime mismatch: file=8394832, actual=9201111), cleaning
# INFO xhjob::daemon] starting fresh daemon, pid=12500
```

### 生产建议

- `data_dir` 与 PID 文件目录分开：PID 文件放 `/run`（tmpfs，重启清空），`data_dir` 放持久盘。
- systemd unit 里加 `PIDFile=/run/xhjob.pid` 配合 `Type=forking`，让 systemd 也能用 starttime 校验。

---

## 7. SQLite 完整性检查

### 签名

```sql
-- daemon 启动时执行
PRAGMA quick_check;
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
```

### 行为说明

daemon 启动打开 SQLite 时，先跑 `PRAGMA quick_check` 校验库完整性：

- **通过** → 继续，启用 `foreign_keys=ON`（强制外键约束），切换 `journal_mode=WAL`；
- **失败** → 输出 `WARN` 告警，记录损坏路径，**不自动修复**（防止误删数据），由运维介入。

### 注意事项

- `quick_check` 比 `integrity_check` 快，但代价是检测不到部分罕见损坏；生产环境可定期跑 `PRAGMA integrity_check` 巡检。
- `foreign_keys=ON` 必须**每次连接**都设置（SQLite 默认关闭），daemon 内所有连接池入口都强制开启。
- 若库损坏，daemon 仍会启动但持久化不可用 → 回退 InMemoryStore 并告警。

### 代码演示

```bash
# 手动巡检生产库
$ sqlite3 /var/lib/xhjob/xhjob.db "PRAGMA integrity_check;"
ok

$ sqlite3 /var/lib/xhjob/xhjob.db "PRAGMA foreign_keys;"
1
```

### 生产建议

- 每日 cron 跑 `PRAGMA integrity_check` + `PRAGMA wal_checkpoint(TRUNCATE)`，把结果接到监控。
- 备份策略：先 `wal_checkpoint(TRUNCATE)` 再 `sqlite3 .backup`，避免 WAL 文件漏备份。

---

## 8. daemon SIGKILL 崩溃恢复流程

### 签名

```
# 恢复流程（4 步，daemon 启动时执行）
1. stale pid 清理        （见第 6 节）
2. DB 一致性检查          （见第 7 节）
3. reset_running_to_pending
4. lease check            （见第 4 节）
```

### 行为说明

daemon 因 `kill -9` / OOM / 断电崩溃后，重启时按固定顺序自愈：

```
启动 daemon
  │
  ├─ ① 读 PID 文件 → starttime 校验
  │     ├─ 匹配（旧 daemon 还活着）→ 拒绝启动，报错
  │     └─ 不匹配 / 不存在 → 清理 stale pid，继续
  │
  ├─ ② 打开 SQLite → PRAGMA quick_check
  │     ├─ ok → foreign_keys=ON, journal_mode=WAL
  │     └─ fail → 告警，回退 InMemoryStore
  │
  ├─ ③ reset_running_to_pending
  │     UPDATE tasks SET state='pending' WHERE state='running'
  │
  └─ ④ lease check（对每条被重置的任务）
        ├─ worker 仍存活（starttime 匹配）→ 回退为 running，记 LeaseHeld
        └─ worker 已死 → 保持 pending，重新派发
```

### 注意事项

- 步骤 ③ ④ 是**原子的批量操作**，不会因为单个任务 lease 失败而回滚整体。
- 整个恢复流程在 daemon 启动后**毫秒级完成**，对调用方透明。
- 若步骤 ② 失败回退到 InMemoryStore，③ ④ 无意义（无持久化数据可恢复），daemon 直接以空队列启动。

### 代码演示

```bash
# 模拟 SIGKILL 崩溃 + 重启自愈
$ xhjob_dispatch ... long-running task ...   # 任务进入 Running
$ pkill -9 -f xhjob-daemon                   # 模拟崩溃

# 重启 daemon
$ XHJOB_PERSIST=true php -d extension=./xhjob.so your-app.php
# daemon 日志：
# INFO xhjob::pid] stale pid 12345 (process gone), cleaning
# INFO xhjob::store] sqlite quick_check: ok, foreign_keys=on, wal=on
# INFO xhjob::recovery] reset_running_to_pending: 3 tasks reset
# INFO xhjob::recovery] lease check: 1 lease held (pid 12340 still alive), 2 re-queued
# INFO xhjob::daemon] recovery complete, ready
```

### 生产建议

- systemd unit 配 `Restart=on-failure` + `RestartSec=2s`，让 SIGKILL 后 2 秒内自愈。
- 把 `reset_running_to_pending: N tasks reset` 这类日志接到 Grafana，监控崩溃频率。

---

## 9. XHJOB_ENCRYPTION_KEY — 任务结果加密

### 签名

```bash
# 环境变量
XHJOB_ENCRYPTION_KEY=<64 hex chars>   # 32 字节，AES-256-GCM
```

### 行为说明

设置 `XHJOB_ENCRYPTION_KEY` 后，daemon 写入持久化的**任务结果**（`result` 字段）会用 AES-256-GCM 加密：

- key 必须是 **64 个十六进制字符**（即 32 字节，对应 AES-256）；
- 每条结果生成随机 12 字节 nonce，密文格式 `nonce || ciphertext || tag`；
- 读取时同 key 解密。

### 注意事项

- key **非法**（非 64 hex / 长度不对）→ **回退明文存储**并打印 `WARN` 告警，不会拒绝启动。
- key **变更**后，旧结果无法解密 → 读取时返回密文原样或空，业务侧需容忍。
- 加密只覆盖 `result` 字段，**不加密** 任务定义、state、事件流（这些是调度必需的明文）。

### 代码演示

```bash
# 生成 32 字节随机 key 并转 hex
$ openssl rand -hex 32
a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2

# 启动 daemon 时注入
$ XHJOB_ENCRYPTION_KEY=a1b2c3...e5f6a1b2 \
  XHJOB_PERSIST=true \
  php -d extension=./xhjob.so your-app.php
```

```php
<?php
// 派发任务，结果会被加密落盘
$id = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('cat /etc/secrets/api-key.txt')   // 输出敏感数据
        ->persist(true)
        ->toJson()
);

// 读取时 daemon 自动解密，调用方无感
print_r(xhjob_result($id));
// Array ( [stdout] => sk-xxx... [exit_code] => 0 )
```

### 生产建议

- key 用密钥管理服务（KMS / Vault）下发，**不要**硬编码到容器镜像。
- key 轮换时，新 daemon 用新 key 写新结果，旧结果保留旧 key 解密 —— 推荐双 key 灰度过渡。
- 定期 `xhjob_inspect('stats')` 看 `encryption_enabled` 字段，断言为 `true`。

---

## 综合示例：persist + acksLate + 重启 daemon 后任务自动重派

```php
<?php
// recover-demo.php
// 演示：派发一个长任务 → 模拟 daemon 崩溃 → 重启 → 任务自动重派完成

xhjob_start();

// 1. 派发 30 秒长任务，启用持久化 + 延迟确认
$taskId = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('echo start; sleep 30; echo done')
        ->timeout(120)
        ->persist(true)
        ->acksLate(true)
        ->acksOnFailure(false)
        ->withRetry(3, 1)
        ->toJson()
);
echo "dispatched: {$taskId}\n";

// 2. 等 5 秒让任务进入 Running
sleep(5);
$state = xhjob_state($taskId);
echo "before crash: state={$state['state']}\n";   // Running

// 3. 模拟 daemon 崩溃（实际生产中是 kill -9 / OOM）
//    这里直接调 xhjob_stop() 模拟；真实场景是 pkill daemon
xhjob_stop();
echo "daemon killed, task lost from memory but persisted in SQLite\n";

// 4. 重启 daemon
xhjob_start();
echo "daemon restarted, running recovery:\n";
echo "  - stale pid cleanup\n";
echo "  - PRAGMA quick_check + foreign_keys=ON\n";
echo "  - reset_running_to_pending\n";
echo "  - lease check (worker dead → re-queue)\n";

// 5. 任务被重新派发，继续执行到完成
while (true) {
    $s = xhjob_state($taskId);
    echo "after restart: state={$s['state']}\n";
    if (in_array($s['state'], ['success', 'failed', 'interrupted'], true)) {
        break;
    }
    sleep(2);
}

print_r(xhjob_result($taskId));
// Array ( [stdout] => start\ndone\n [exit_code] => 0 )

// 6. 查事件流，确认没有重复执行（lease_held 应为 0，因为 worker 真的死了）
$events = json_decode(xhjob_pull_events(time() - 120, null), true);
$leaseHeld = count(array_filter($events, fn($e) => $e['event_type'] === 'lease_held'));
echo "lease_held events: {$leaseHeld} (expected 0, worker was killed)\n";
```

### 生产建议（综合）

- **幂等是前提**：`acksLate(true) + acksOnFailure(false)` 等于「至少一次执行」，业务必须幂等。
- **监控三件套**：`lease_held` 频率（worker 没死却重派 = bug）、`hung_detected` 频率（任务质量差）、`interrupted` 频率（假死被杀）。
- **备份三件套**：SQLite 主库 + WAL 文件 + `XHJOB_ENCRYPTION_KEY`，三者缺一不可恢复。
- **演练**：每月做一次 chaos engineering，`pkill -9 xhjob-daemon` 后验证任务自动重派，确保恢复链路在线。
