# Tasks

## 阶段一：站点骨架与发布流水线
- [x] Task 1: 搭建 Jekyll 站点骨架（Just-The-Docs 主题）
  - [x] SubTask 1.1: 新建 `docs/_config.yml`，配置 theme、title、description、baseurl（`/xhjob`）、permalink、nav 配置
  - [x] SubTask 1.2: 新建 `docs/index.md`（首页：一句话介绍 + 核心特性 + 快速开始入口 + 导航说明）
  - [x] SubTask 1.3: 新建 `.github/workflows/docs.yml`，配置 GitHub Pages 构建发布（push 到 main 触发，permissions: pages: write、id-token: write，使用 `configure-pages` + `upload-pages-artifact` + `deploy-pages`）
  - [x] SubTask 1.4: 仓库 Settings → Pages 源设为 GitHub Actions（文档说明，非代码）

## 阶段二：入门篇
- [x] Task 2: 编写「快速开始」文档 `docs/quickstart.md`
  - [x] SubTask 2.1: 前置条件（PHP 8.0+、Linux x86_64、bash）
  - [x] SubTask 2.2: 下载 `releases/xhjob-php8.2-linux-x86_64.so` + `php -d extension=<so>` 加载验证
  - [x] SubTask 2.3: 第一个 shell 任务（`TaskBuilder::shell('echo hi')->dispatch()` + `xhjob_state`）
  - [x] SubTask 2.4: 第一个 cron 任务（5 字段表达式 + 持久化）
  - [x] SubTask 2.5: 注意事项（daemon 是独立进程、CLI vs FPM、扩展加载失败排查）
- [x] Task 3: 编写「架构概览」文档 `docs/architecture.md`
  - [x] SubTask 3.1: 组件图（PHP 进程 / daemon / IPC / store / executor / scheduler / pool）
  - [x] SubTask 3.2: 请求流程（FPM dispatch → IPC → daemon → pool → executor → store）
  - [x] SubTask 3.3: daemon 生命周期（start/stop/restart/status/healthCheck）
  - [x] SubTask 3.4: 与 Celery/APScheduler 概念映射表
- [x] Task 4: 编写「安装与配置」文档 `docs/install-config.md`
  - [x] SubTask 4.1: 扩展加载（`extension=` ini / `PHP_INI_SCAN_DIR` / `dl()` 限制说明）
  - [x] SubTask 4.2: env var 全表（`XHJOB_SERVICE` / `XHJOB_DATA_DIR` / `XHJOB_SOCK_DIR` / `XHJOB_PID_DIR` / `XHJOB_LOG_DIR` / `XHJOB_IPC_TIMEOUT_SECS` / `XHJOB_POOL_MODE` / `XHJOB_ASYNC_POOL_SIZE` / `XHJOB_THREAD_POOL_SIZE` / `XHJOB_PERSIST` / `XHJOB_API_TOKEN` / `XHJOB_CONFIG_FILE` / `XHJOB_SHELL_TIMEOUT` / `XHJOB_MAX_PENDING`）
  - [x] SubTask 4.3: 配置文件 `/etc/xhjob/config`（`KEY=VALUE` 格式、env 覆盖优先级）
  - [x] SubTask 4.4: 路径解析优先级表（显式参数 > 细分 env > `XHJOB_DATA_DIR` > 平台默认）
  - [x] SubTask 4.5: 服务名校验规则（`^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`）
  - [x] SubTask 4.6: PID 文件双行格式（pid + starttime 防 PID 复用）

## 阶段三：API 参考篇
- [x] Task 5: 编写「PHP 函数参考」文档 `docs/api-functions.md`（全部 27 个 `xhjob_*` 函数 + `Xhjob` 类）
  - [x] SubTask 5.1: 生命周期类（`xhjob_start/stop/restart/status/run_daemon`）—— 每个含签名/参数表/返回/错误契约/注意事项/代码演示
  - [x] SubTask 5.2: 任务派发与查询类（`xhjob_dispatch/state/result/get/list`）—— 含 `"error:"` 前缀契约说明
  - [x] SubTask 5.3: 任务控制类（`xhjob_remove/pause/resume/cancel/requeue/reschedule/modify`）
  - [x] SubTask 5.4: 编排类（`xhjob_chain/chain_state/group/group_state/chord/chord_state`）
  - [x] SubTask 5.5: 事件与进度类（`xhjob_events/pull_events/report_progress/inspect`）
  - [x] SubTask 5.6: `Xhjob` PHP 类（Rust 侧链式 builder，列出全部 camelCase 方法）
- [x] Task 6: 编写「TaskBuilder API」文档 `docs/api-taskbuilder.md`
  - [x] SubTask 6.1: 静态工厂（`shell/http/chain/group/chord/fromJson`）
  - [x] SubTask 6.2: 触发器方法（`cron/every/runAt/countdown/startAt/endAt`）+ 注意事项（互斥规则、`runAt` 覆盖 `countdown`）
  - [x] SubTask 6.3: 重试与超时（`withRetry/retryBackoff/timeout/softTimeout`）+ `InvalidTaskConfigException` 场景
  - [x] SubTask 6.4: 并发控制（`priority/maxInstances/allowOverlap/coalesce/maxExecutions/jitter/expires/misfireGraceTime/rateLimit`）
  - [x] SubTask 6.5: 可靠性（`persist/acksLate/acksOnFailure/ignoreResult/resultTtl`）
  - [x] SubTask 6.6: 元数据与身份（`withId/replaceExisting/tag/tags/withMeta/withTimezone/idempotent`）+ `withId` 字符集校验 `^[A-Za-z0-9_-]{1,64}$`
  - [x] SubTask 6.7: HTTP 专属（`withHeaders/withBody/withProxy`）+ `withProxy` 写入顶层 `proxy` 字段的注意事项
  - [x] SubTask 6.8: Shell 专属（`withEncoding/withStdin/withWorkingDir/withEnv`）+ 编码值（GBK/Big5/Shift_JIS/auto）
  - [x] SubTask 6.9: 终端方法（`dispatch/toArray/toJson`）
- [x] Task 7: 编写「TaskManager API」文档 `docs/api-taskmanager.md`
  - [x] SubTask 7.1: 构造与 getter（`__construct/getName/getDataDir`）
  - [x] SubTask 7.2: 创建类（`create/createChain/createGroup/createChord/update`）
  - [x] SubTask 7.3: 查询类（`get/list/state/result/logs`）+ 异常契约（`ServiceNotRunningException` / `TaskNotFoundException`）
  - [x] SubTask 7.4: 控制类（`stop/restart/pause/resume/remove/reschedule`）
  - [x] SubTask 7.5: 编排状态（`chainState/groupState/chordState`）
  - [x] SubTask 7.6: 进度事件（`reportProgress/pullEvents/inspect`）+ inspect 四种 mode
  - [x] SubTask 7.7: 轮询等待（`waitForState/waitForResult`）+ 终态列表

## 阶段四：核心能力篇
- [x] Task 8: 编写「触发器」文档 `docs/triggers.md`
  - [x] SubTask 8.1: cron（5/6 字段表达式 + 示例表 + cron.rs 解析说明）
  - [x] SubTask 8.2: interval（`every(n)` + 首次立即触发行为）
  - [x] SubTask 8.3: runAt（一次性绝对时间戳）
  - [x] SubTask 8.4: countdown（相对延迟，与 `runAt` 互斥）
  - [x] SubTask 8.5: or_cron（多 cron 表达式并集触发）+ 代码演示
  - [x] SubTask 8.6: skip_dates（跳过日期列表）+ workdays_only（仅工作日）+ 代码演示
  - [x] SubTask 8.7: jitter（随机抖动防惊群）+ misfire_grace_time（迟到宽限）+ 代码演示
  - [x] SubTask 8.8: start_date / end_date（生效区间）
- [x] Task 9: 编写「重试与超时」文档 `docs/retry-timeout.md`
  - [x] SubTask 9.1: `withRetry(max, delay)` 基础重试 + 代码演示
  - [x] SubTask 9.2: `retryBackoff(true)` 指数退避公式 `min(delay * 2^attempts, delay * 60)`
  - [x] SubTask 9.3: `acksOnFailure(false)` 失败不 ack 无限重试
  - [x] SubTask 9.4: `timeout` 硬超时（SIGKILL）路径
  - [x] SubTask 9.5: `softTimeout` 软超时（SIGTERM → grace → SIGKILL 升级链）+ 必须 `< timeout` 注意事项
  - [x] SubTask 9.6: `expires` 任务过期自动丢弃（Pending 超过 expires 秒）
- [x] Task 10: 编写「并发控制」文档 `docs/concurrency.md`
  - [x] SubTask 10.1: `maxInstances(n)` 最大并发实例
  - [x] SubTask 10.2: `allowOverlap(true)` 允许重叠执行
  - [x] SubTask 10.3: `coalesce(true)` 合并漏触发
  - [x] SubTask 10.4: `rateLimit(count, window)` 滑动窗口限流
  - [x] SubTask 10.5: `priority(n)` 优先级调度
  - [x] SubTask 10.6: `maxExecutions(n)` 最大执行次数（0=无限）
- [x] Task 11: 编写「持久化与崩溃恢复」文档 `docs/persistence-recovery.md`
  - [x] SubTask 11.1: `persist(true)` 启用 SQLite+WAL（需 `--all-features` 编译）+ 未启用 fallback InMemoryStore 警告
  - [x] SubTask 11.2: `acksLate(true)` 崩溃后 Running 任务重置为 Pending 重新派发
  - [x] SubTask 11.3: execution_lease（worker_pid + worker_starttime spawn 时同步写入）+ LeaseHeld 事件
  - [x] SubTask 11.4: watchdog 任务假死检测（timeout * factor 无完成 → 取消标记 Interrupted）
  - [x] SubTask 11.5: PID 复用防护（pid + starttime 双校验）
  - [x] SubTask 11.6: SQLite 完整性检查（启动 `PRAGMA quick_check`）+ `PRAGMA foreign_keys=ON`
  - [x] SubTask 11.7: daemon SIGKILL 崩溃恢复流程（stale pid 清理 → DB 一致性 → reset_running_to_pending → lease check）
- [x] Task 12: 编写「编排（chain/group/chord）」文档 `docs/orchestration.md`
  - [x] SubTask 12.1: chain 顺序执行 + 上一步结果传递 + 失败中断
  - [x] SubTask 12.2: group 并行执行 + 全部完成汇总
  - [x] SubTask 12.3: chord（header 并行 + callback 回调，header 全成功才派 callback，partial_failed 终态）
  - [x] SubTask 12.4: 三者状态查询（chainState/groupState/chordState）
- [x] Task 13: 编写「进度上报与事件」文档 `docs/progress-events.md`
  - [x] SubTask 13.1: `reportProgress(id, percent, meta)` 任务内上报（0-100）+ 代码演示
  - [x] SubTask 13.2: `pullEvents(sinceTs, eventType)` 全局事件流 + EventType 枚举全表（started/succeeded/failed/missed/cancelled/paused/resumed/expired/max_instances_reached/rate_limited/interrupted/hung_detected/lease_held/unknown）
  - [x] SubTask 13.3: `events(sinceTs, taskId)` 单任务事件过滤
  - [x] SubTask 13.4: `inspect(mode)` 四种模式（active/registered/scheduled/stats）+ 输出示例

## 阶段五：进阶篇
- [x] Task 14: 编写「双线程池模式对比」文档 `docs/pool-modes.md`
  - [x] SubTask 14.1: `XHJOB_POOL_MODE` 配置（async 默认 / thread / coroutine 别名）+ 启动时读取一次、切换需 stop+start
  - [x] SubTask 14.2: 实现差异表（async=tokio M:N / thread=std::thread 1:1；worker 名 `xhjob-tokio` vs `xhjob-worker-N`）
  - [x] SubTask 14.3: 性能特征对比表（默认并发上限 1024 vs CPU 核数；每任务内存 KB vs MB；IO/CPU 密集表现；隔离性）
  - [x] SubTask 14.4: 配置项（`XHJOB_ASYNC_POOL_SIZE` 默认 1024、`XHJOB_THREAD_POOL_SIZE` 默认 num_cpus）
  - [x] SubTask 14.5: 何时选用决策树（IO 密集/高并发短任务 → async；CPU 密集/强隔离/严格并发上限 → thread）
  - [x] SubTask 14.6: 切换代码演示（`putenv("XHJOB_POOL_MODE=thread"); xhjob_start(...)`）
  - [x] SubTask 14.7: 可复现基准对比说明（引用 `test_xhjob_pool_diff.php` 的 `/proc/{pid}/task` 线程名验证 + 并发时间戳验证法）

## 阶段六：生产实战篇
- [x] Task 15: 编写「后台任务队列」文档 `docs/prod-background-queue.md`
  - [x] SubTask 15.1: 架构图（FPM 请求 → dispatch 非阻塞 → daemon 队列 → executor → store；CLI worker 轮询 result）
  - [x] SubTask 15.2: FPM 侧代码（`TaskBuilder::shell(...)->ignoreResult(true)->rateLimit(100, 60)->dispatch()` + 立即返回 task_id）
  - [x] SubTask 15.3: CLI worker 代码（`while` 轮询 `xhjob_state` + `xhjob_result` + 超时处理）
  - [x] SubTask 15.4: 进度上报代码（任务脚本内 `xhjob_report_progress`）
  - [x] SubTask 15.5: 失败重试配置（`withRetry(3, 5)->retryBackoff(true)`）
  - [x] SubTask 15.6: 注意事项（FPM 不要 `waitForResult` 阻塞、worker 用 `nice` 降权、`ignoreResult` 节省 DB）
- [x] Task 16: 编写「定时任务」文档 `docs/prod-cron.md`
  - [x] SubTask 16.1: cron 表达式 + `withTimezone('Asia/Shanghai')` 时区
  - [x] SubTask 16.2: `persist(true)` 持久化（daemon 重启不丢任务定义）
  - [x] SubTask 16.3: `acksLate(true)` 崩溃恢复（Running 任务重启后重派）
  - [x] SubTask 16.4: `maxExecutions(n)` 限制总执行次数
  - [x] SubTask 16.5: `coalesce(true)` 合并漏触发（宕机期间多次 cron 只补一次）
  - [x] SubTask 16.6: `skipDates` 节假日跳过 + `workdaysOnly()` 仅工作日
  - [x] SubTask 16.7: 完整端到端代码演示 + 注意事项（cron 6 字段支持秒级、时区必须设置否则用系统本地）
- [x] Task 17: 编写「CLI 与 FPM 共用服务连接」文档 `docs/prod-cli-fpm-share.md`
  - [x] SubTask 17.1: 共用原理（同一 service_name + data_dir → 同一 socket/pid/db；daemon 是独立进程，CLI/FPM 都是 IPC 客户端）
  - [x] SubTask 17.2: 启动 daemon 的两种方式（CLI `xhjob_start` 长驻 / FPM 请求内 `ensureRunning` 拉起 + `xhjob_server.php` 跨进程模式）
  - [x] SubTask 17.3: 路径解析优先级表（显式参数 > `XHJOB_SOCK_DIR/PID_DIR/LOG_DIR` > `XHJOB_DATA_DIR` > `/run/xhjob` > `/var/run/xhjob` > `/tmp`）
  - [x] SubTask 17.4: 多服务隔离（不同 service_name 完全独立 daemon + DB）
  - [x] SubTask 17.5: PID 文件双行格式（pid + starttime）防 PID 复用
  - [x] SubTask 17.6: `XHJOB_IPC_TIMEOUT_SECS`（默认 5s）防 FPM worker 在 daemon 死锁时被永久阻塞（`max_execution_time` 不中断 C 级阻塞）
  - [x] SubTask 17.7: 完整代码演示（systemd 启 daemon + FPM 请求 dispatch + CLI 查状态）
- [x] Task 18: 编写「ThinkPHP 8 第三方类库集成」文档 `docs/prod-thinkphp8.md`
  - [x] SubTask 18.1: composer 安装（`composer require xhjob/thinkphp8-extend` 或本地 path repository）
  - [x] SubTask 18.2: 手动安装方式（copy `Xhjob/` 到 `extend/`、copy config、append route、注册 service）
  - [x] SubTask 18.3: `ServiceProvider` 自动注册（`extra.think.services`）+ 绑定 `xhjob.service` / `xhjob.manager` 单例
  - [x] SubTask 18.4: `Xhjob` Facade 用法（`\Xhjob\facade\Xhjob::create(...)`）+ 全方法 @method 列表
  - [x] SubTask 18.5: helper 函数（`xhjob_manager() / xhjob_service() / xhjob_task($cmd)`）
  - [x] SubTask 18.6: config 键全表（`service_name/data_dir/api_token/pool_mode` + 对应 env var）
  - [x] SubTask 18.7: HTTP 端点全表（25 个路由：GET/POST/DELETE + 控制器动作）
  - [x] SubTask 18.8: 生产 controller 调用示例（创建 cron 任务 + 查状态 + 取结果 + 取消）
  - [x] SubTask 18.9: 注意事项（`api_token` 必须配置否则中间件抛 500；route 中 demo 必须为 POST 防爬虫；`stop/restart` 无 id 时操作 daemon）

## 阶段七：排障篇
- [x] Task 19: 编写「故障排查」文档 `docs/troubleshooting.md`
  - [x] SubTask 19.1: 扩展未加载（`Call to undefined function Xhjob\xhjob_status()`）→ `php -d extension=<so>` 或 `PHP_INI_SCAN_DIR`
  - [x] SubTask 19.2: daemon 启动失败（`xhjob_start` 返回 false）→ 检查 socket 目录权限、端口占用、`/run/xhjob` 创建
  - [x] SubTask 19.3: `xhjob_dispatch` 返回 `error: ...` → 解析错误前缀，常见：服务名校验失败、task_json 非法、daemon 不可达
  - [x] SubTask 19.4: 任务卡 Running → watchdog 配置、`xhjob_cancel` + lease 检查
  - [x] SubTask 19.5: SQLite 损坏 → `PRAGMA quick_check` 启动检查、备份 db 文件
  - [x] SubTask 19.6: FPM worker 阻塞 → `XHJOB_IPC_TIMEOUT_SECS` 调小
  - [x] SubTask 19.7: persist feature 未启用 → 确认 `--all-features` 编译 + `XHJOB_PERSIST!=0`
  - [x] SubTask 19.8: zombie 进程残留 → `kill -9` + `pcntl_waitpid` reap（测试环境 daemon 是 PHP 子进程）

## 阶段八：导航与验证
- [x] Task 20: 配置左侧导航（`docs/_config.yml` 的 `nav` 或各文档 front matter `parent`/`nav_order`）
  - [x] SubTask 20.1: 入门分组（quickstart/architecture/install-config）
  - [x] SubTask 20.2: API 参考分组（api-functions/api-taskbuilder/api-taskmanager）
  - [x] SubTask 20.3: 核心能力分组（triggers/retry-timeout/concurrency/persistence-recovery/orchestration/progress-events）
  - [x] SubTask 20.4: 进阶分组（pool-modes）
  - [x] SubTask 20.5: 生产实战分组（prod-background-queue/prod-cron/prod-cli-fpm-share/prod-thinkphp8）
  - [x] SubTask 20.6: 排障分组（troubleshooting）
- [x] Task 21: 代码演示真实性核对（对照 `src/lib.rs` 与 `releases/xhjob-thinkphp8-extend/Xhjob/*.php`）
  - [x] SubTask 21.1: 核对所有 env var 名与 `src/` 读取点一致
  - [x] SubTask 21.2: 核对所有函数签名与 `src/lib.rs` 导出一致
  - [x] SubTask 21.3: 核对所有 TaskBuilder/TaskManager 方法与 PHP 类一致
  - [x] SubTask 21.4: 核对所有 EventType 枚举值与 `src/store/mod.rs` 一致（含 `lease_held` 而非 `leaseheld`）

# Task Dependencies
- Task 1（站点骨架）是所有内容文档的前置（提供 `_config.yml` 与主题）
- Task 2-19 可在 Task 1 完成后并行（各文档独立）
- Task 20（导航）依赖 Task 2-19 全部完成（需所有文档文件名确定）
- Task 21（核对）依赖 Task 5-18 完成（需有代码演示可核对）
- 无外部代码依赖（纯文档产出，不触碰 Rust/PHP 源码）
