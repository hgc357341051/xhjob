# Tasks

## 阶段一：清理旧 Jekyll 资产
- [ ] Task 1: 删除上一轮 Jekyll 方案的全部产物
  - [ ] SubTask 1.1: 删除 `.github/workflows/docs.yml`（GitHub Actions 流水线）
  - [ ] SubTask 1.2: 删除 `docs/_config.yml`（Jekyll 配置）
  - [ ] SubTask 1.3: 删除整个 `docs/` 目录下所有现有 `.md` 文件（index.md、6 个分组索引页、19 篇内容文档）

## 阶段二：搭建 Docsify 骨架
- [ ] Task 2: 创建 Docsify 入口与配置
  - [ ] SubTask 2.1: 创建 `docs/index.html`——引入 docsify.js（CDN unpkg）+ search 插件 + prism 代码高亮插件（PHP/Rust/Bash/JSON 语言组件）+ emoji 插件 + copy-to-clipboard 插件；配置 `repo`、`loadSidebar: true`、`coverpage: true`、`subMaxLevel: 3`、`search`（placeholder/noData/depth/maxAge）、`auto2top: true`、`name: 'Xhjob'`、`logo: 'assets/img/logo.svg'`、`themeColor: '#3B82F6'`、`extPlugin: ['assets/css/custom.css']`
  - [ ] SubTask 2.2: 创建 `docs/.nojekyll`（空文件）
  - [ ] SubTask 2.3: 创建 `docs/assets/img/logo.svg`——项目 Logo（X 字母 + 任务队列几何图形，主色 #3B82F6）
  - [ ] SubTask 2.4: 创建 `docs/assets/css/custom.css`——完整自定义主题（见 Task 2b 详述）
  - [ ] SubTask 2.5: 创建 `docs/_coverpage.md`——封面页（Logo + 标题 + 副标题 + 徽章行 `PHP 8.0+`/`Rust`/`Apache-2.0`/`Linux` + 三个 CTA 按钮：快速开始/API 参考/GitHub + 背景渐变由 custom.css 控制）
  - [ ] SubTask 2.6: 创建 `docs/_sidebar.md`——6 分组导航（带 emoji 分组标题 🚀入门/📚API参考/⚙️核心能力/🔬进阶/🏭生产实战/🛠️排障 + 分隔线 `---`）
  - [ ] SubTask 2.7: 创建 `docs/README.md`——Docsify 默认首页（项目介绍 + 核心特性卡片网格 6-8 张 + 快速开始代码 + 三种部署方式 + 技术栈说明）

- [ ] Task 2b: 编写 `docs/assets/css/custom.css` 完整样式
  - [ ] SubTask 2b.1: CSS 变量定义（亮色：--primary:#3B82F6, --primary-dark:#1E40AF, --accent:#F59E0B, --bg:#FFFFFF, --text:#1F2937, --code-bg:#F3F4F6 等；暗色：--bg:#0F172A, --text:#E2E8F0, --code-bg:#1E293B 等）
  - [ ] SubTask 2b.2: 封面页样式（全屏渐变背景 linear-gradient(135deg,#3B82F6,#8B5CF6)、居中 flex 布局、标题超大字号、徽章 inline-flex 圆角、CTA 按钮实心/描边两种样式）
  - [ ] SubTask 2b.3: 特性卡片网格样式（CSS Grid 响应式：repeat(auto-fill,minmax(280px,1fr))、卡片阴影 box-shadow、hover transform translateY(-4px) 过渡动画）
  - [ ] SubTask 2b.4: 代码块增强（prism 主题覆盖、复制按钮定位右上角、hover 显示、点击反馈、横向滚动 overflow-x:auto）
  - [ ] SubTask 2b.5: 表格美化（表头主色背景+白字、斑马纹 nth-child(even)、hover 行高亮、border-collapse+圆角 overflow hidden、移动端 overflow-x:auto）
  - [ ] SubTask 2b.6: 导航侧边栏样式（分组标题大写字母间距+主色、子项缩进 padding-left、当前页 active 高亮左边框、折叠展开箭头）
  - [ ] SubTask 2b.7: 主题切换按钮（右上角固定定位、三态按钮 ☀️/🌙/🖥️、localStorage 持久化、`[data-theme]` 属性切换）
  - [ ] SubTask 2b.8: 移动端响应式（@media max-width:768px：侧边栏 transform translateX 隐藏+汉堡菜单、卡片单列、表格/代码横向滚动、封面字号 clamp 缩小）

## 阶段三：入门篇
- [ ] Task 3: 编写「快速开始」`docs/quickstart.md`
  - [ ] SubTask 3.1: 前置条件（PHP 8.0+、Linux x86_64、bash）
  - [ ] SubTask 3.2: 下载预编译 so + `php -d extension=<so>` 加载验证
  - [ ] SubTask 3.3: 第一个 shell 任务（`TaskBuilder::shell('echo hi')->dispatch()` + `xhjob_state`）
  - [ ] SubTask 3.4: 第一个 cron 任务（5 字段表达式 + 持久化）
  - [ ] SubTask 3.5: 注意事项（daemon 独立进程、CLI/FPM、扩展加载失败排查）
- [ ] Task 4: 编写「架构概览」`docs/architecture.md`
  - [ ] SubTask 4.1: 组件图（PHP 进程 / daemon / IPC / store / executor / scheduler / pool）
  - [ ] SubTask 4.2: 请求流程（FPM dispatch → IPC → daemon → pool → executor → store）
  - [ ] SubTask 4.3: daemon 生命周期（start/stop/restart/status/healthCheck）
  - [ ] SubTask 4.4: 与 Celery/APScheduler 概念映射表
- [ ] Task 5: 编写「安装与配置」`docs/install-config.md`
  - [ ] SubTask 5.1: 扩展加载三种方式（`extension=` ini / `PHP_INI_SCAN_DIR` / `-d extension=`）+ `dl()` 限制
  - [ ] SubTask 5.2: env var 全表（XHJOB_SERVICE_NAME / XHJOB_DATA_DIR / XHJOB_SOCK_DIR / XHJOB_PID_DIR / XHJOB_LOG_DIR / XHJOB_IPC_TIMEOUT_SECS / XHJOB_POOL_MODE / XHJOB_ASYNC_POOL_SIZE / XHJOB_COROUTINE_POOL_SIZE / XHJOB_THREAD_POOL_SIZE / XHJOB_PERSIST / XHJOB_API_TOKEN / XHJOB_CONFIG_FILE / XHJOB_SHELL_TIMEOUT / XHJOB_MAX_PENDING）—— 用 Grep 在 src/ 核对真实变量名
  - [ ] SubTask 5.3: 配置文件 `/etc/xhjob/config`（KEY=VALUE 格式、env 覆盖优先级）
  - [ ] SubTask 5.4: 路径解析优先级表（显式参数 > 细分 env > XHJOB_DATA_DIR > 平台默认 /run/xhjob > /var/run/xhjob > /tmp）
  - [ ] SubTask 5.5: 服务名校验规则 `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`
  - [ ] SubTask 5.6: PID 文件双行格式（pid + starttime 防 PID 复用）

## 阶段四：API 参考篇
- [ ] Task 6: 编写「PHP 函数参考」`docs/api-functions.md`（全部 27 个 `xhjob_*` 函数 + `Xhjob` 类）—— **必须先 Read /workspace/src/lib.rs 核对签名**
  - [ ] SubTask 6.1: 生命周期类（xhjob_start/stop/restart/status/run_daemon）
  - [ ] SubTask 6.2: 派发查询类（xhjob_dispatch/state/result/get/list）+ `error:` 前缀契约
  - [ ] SubTask 6.3: 控制类（xhjob_remove/pause/resume/cancel/requeue/reschedule/modify）
  - [ ] SubTask 6.4: 编排类（xhjob_chain/chain_state/group/group_state/chord/chord_state）
  - [ ] SubTask 6.5: 事件进度类（xhjob_events/pull_events/report_progress/inspect）
  - [ ] SubTask 6.6: Xhjob PHP 类全部 camelCase 方法
- [ ] Task 7: 编写「TaskBuilder API」`docs/api-taskbuilder.md`—— **必须先 Read /workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskBuilder.php 核对**
  - [ ] SubTask 7.1: 静态工厂（shell/http/chain/group/chord/fromJson）
  - [ ] SubTask 7.2: 触发器（cron/every/runAt/countdown/startAt/endAt）+ 互斥规则
  - [ ] SubTask 7.3: 重试超时（withRetry/retryBackoff/timeout/softTimeout）+ InvalidTaskConfigException 场景
  - [ ] SubTask 7.4: 并发控制（priority/maxInstances/allowOverlap/coalesce/maxExecutions/jitter/expires/misfireGraceTime/rateLimit）
  - [ ] SubTask 7.5: 可靠性（persist/acksLate/acksOnFailure/ignoreResult/resultTtl）
  - [ ] SubTask 7.6: 元数据身份（withId/replaceExisting/tag/tags/withMeta/withTimezone/idempotent）+ withId 校验 `^[A-Za-z0-9_-]{1,64}$`
  - [ ] SubTask 7.7: HTTP 专属（withHeaders/withBody/withProxy 写顶层 proxy 字段）
  - [ ] SubTask 7.8: Shell 专属（withEncoding GBK/Big5/Shift_JIS/auto / withStdin/withWorkingDir/withEnv）
  - [ ] SubTask 7.9: 终端方法（dispatch/toArray/toJson）
- [ ] Task 8: 编写「TaskManager API」`docs/api-taskmanager.md`—— **必须先 Read /workspace/releases/xhjob-thinkphp8-extend/Xhjob/TaskManager.php 核对**
  - [ ] SubTask 8.1: 构造与 getter（__construct/getName/getDataDir）
  - [ ] SubTask 8.2: 创建类（create/createChain/createGroup/createChord/update）
  - [ ] SubTask 8.3: 查询类（get/list/state/result/logs）+ 异常契约
  - [ ] SubTask 8.4: 控制类（stop/restart/pause/resume/remove/reschedule）
  - [ ] SubTask 8.5: 编排状态（chainState/groupState/chordState）
  - [ ] SubTask 8.6: 进度事件（reportProgress/pullEvents/inspect 四模式）
  - [ ] SubTask 8.7: 轮询等待（waitForState/waitForResult）+ 终态列表

## 阶段五：核心能力篇
- [ ] Task 9: 编写「触发器」`docs/triggers.md`
  - [ ] SubTask 9.1: cron（5/6 字段 + 示例表）
  - [ ] SubTask 9.2: interval（every + 首次立即触发）
  - [ ] SubTask 9.3: runAt（一次性绝对时间戳）
  - [ ] SubTask 9.4: countdown（相对延迟，与 runAt 互斥）
  - [ ] SubTask 9.5: or_cron（多 cron 并集）—— 注意：仅 Rust Xhjob 类有 orCron()，PHP TaskBuilder 无
  - [ ] SubTask 9.6: skip_dates + workdays_only —— 注意：仅 Rust Xhjob 类有
  - [ ] SubTask 9.7: jitter + misfire_grace_time
  - [ ] SubTask 9.8: start_date / end_date
- [ ] Task 10: 编写「重试与超时」`docs/retry-timeout.md`
  - [ ] SubTask 10.1: withRetry 基础重试
  - [ ] SubTask 10.2: retryBackoff 指数退避 `min(delay * 2^attempts, delay * 60)`
  - [ ] SubTask 10.3: acksOnFailure(false) 无限重试
  - [ ] SubTask 10.4: timeout 硬超时 SIGKILL
  - [ ] SubTask 10.5: softTimeout 软超时升级链（必须 < timeout，HTTP 不支持）
  - [ ] SubTask 10.6: expires 任务过期丢弃
- [ ] Task 11: 编写「并发控制」`docs/concurrency.md`（maxInstances/allowOverlap/coalesce/rateLimit/priority/maxExecutions）
- [ ] Task 12: 编写「持久化与崩溃恢复」`docs/persistence-recovery.md`
  - [ ] SubTask 12.1: persist（SQLite+WAL，需 --all-features，未启用 fallback InMemoryStore 警告）
  - [ ] SubTask 12.2: acksLate 崩溃恢复
  - [ ] SubTask 12.3: execution_lease（worker_pid + worker_starttime 同步写入 + LeaseHeld 事件）
  - [ ] SubTask 12.4: watchdog 假死检测（daemon 级 env 配置，无 TaskBuilder 方法）
  - [ ] SubTask 12.5: PID 复用防护
  - [ ] SubTask 12.6: SQLite 完整性检查（PRAGMA quick_check + foreign_keys=ON）
  - [ ] SubTask 12.7: daemon SIGKILL 崩溃恢复流程
- [ ] Task 13: 编写「编排」`docs/orchestration.md`（chain/group/chord + 状态查询）
- [ ] Task 14: 编写「进度上报与事件」`docs/progress-events.md`
  - [ ] SubTask 14.1: reportProgress（0-100）
  - [ ] SubTask 14.2: pullEvents 全局事件流
  - [ ] SubTask 14.3: events 单任务过滤（用 logs() 方法，TaskManager 无独立 events() 方法）
  - [ ] SubTask 14.4: EventType 枚举全表（正确下划线拼写：lease_held/hung_detected/max_instances_reached/rate_limited）
  - [ ] SubTask 14.5: inspect 四模式

## 阶段六：进阶篇
- [ ] Task 15: 编写「双线程池模式对比」`docs/pool-modes.md`
  - [ ] SubTask 15.1: XHJOB_POOL_MODE 配置（async 默认 / thread / coroutine 别名）+ 启动时读取一次、切换需 stop+start
  - [ ] SubTask 15.2: 实现差异表（async=tokio M:N / thread=std::thread 1:1；worker 名 xhjob-tokio vs xhjob-worker-N）
  - [ ] SubTask 15.3: 性能特征对比表（并发上限 1024 vs CPU 核数；内存 KB vs MB；IO/CPU 密集；隔离性）
  - [ ] SubTask 15.4: 配置项（XHJOB_ASYNC_POOL_SIZE 默认 1024、XHJOB_THREAD_POOL_SIZE 默认 num_cpus）
  - [ ] SubTask 15.5: 选型决策树
  - [ ] SubTask 15.6: 切换代码演示
  - [ ] SubTask 15.7: 可复现基准对比说明（test_xhjob_pool_diff.php 的 /proc/{pid}/task 验证法）

## 阶段七：生产实战篇
- [ ] Task 16: 编写「后台任务队列」`docs/prod-background-queue.md`
  - [ ] SubTask 16.1: 架构图（FPM → dispatch 非阻塞 → daemon 队列 → executor → store；CLI worker 轮询）
  - [ ] SubTask 16.2: FPM 侧代码（ignoreResult + rateLimit + dispatch 立即返回 task_id）
  - [ ] SubTask 16.3: CLI worker 代码（轮询 state + result + 超时）
  - [ ] SubTask 16.4: 进度上报代码（reportProgress）
  - [ ] SubTask 16.5: 失败重试配置（withRetry + retryBackoff）
  - [ ] SubTask 16.6: 注意事项（FPM 不要 waitForResult、worker nice 降权、ignoreResult 省 DB）
- [ ] Task 17: 编写「定时任务」`docs/prod-cron.md`
  - [ ] SubTask 17.1: cron + withTimezone('Asia/Shanghai')
  - [ ] SubTask 17.2: persist(true) 持久化
  - [ ] SubTask 17.3: acksLate(true) 崩溃恢复
  - [ ] SubTask 17.4: maxExecutions(n)
  - [ ] SubTask 17.5: coalesce(true) 合并漏触发
  - [ ] SubTask 17.6: skipDates 节假日 + workdaysOnly 工作日
  - [ ] SubTask 17.7: 完整端到端代码 + 注意事项
- [ ] Task 18: 编写「CLI 与 FPM 共用服务连接」`docs/prod-cli-fpm-share.md`
  - [ ] SubTask 18.1: 共用原理（同一 service_name + data_dir → 同一 socket/pid/db）
  - [ ] SubTask 18.2: 两种启动方式（CLI xhjob_start 长驻 / FPM ensureRunning 拉起 / xhjob_server.php 跨进程）
  - [ ] SubTask 18.3: 路径解析优先级表
  - [ ] SubTask 18.4: 多服务隔离
  - [ ] SubTask 18.5: PID 文件双行格式防 PID 复用
  - [ ] SubTask 18.6: XHJOB_IPC_TIMEOUT_SECS 防 FPM worker 阻塞
  - [ ] SubTask 18.7: 完整代码演示（systemd + FPM + CLI）
- [ ] Task 19: 编写「ThinkPHP 8 集成」`docs/prod-thinkphp8.md`—— **必须先 Read /workspace/releases/xhjob-thinkphp8-extend/ 下 composer.json/README/ServiceProvider/facade/helper/config/route 核对**
  - [ ] SubTask 19.1: composer 安装 + 手动安装
  - [ ] SubTask 19.2: ServiceProvider 自动注册（extra.think.services，绑定 xhjob.service/xhjob.manager）
  - [ ] SubTask 19.3: Xhjob Facade 用法 + 全 @method 列表（含 chordState）
  - [ ] SubTask 19.4: helper 函数（xhjob_manager/xhjob_service/xhjob_task）
  - [ ] SubTask 19.5: config 键全表（service_name/data_dir/api_token/pool_mode + env）
  - [ ] SubTask 19.6: HTTP 端点全表（25 路由）
  - [ ] SubTask 19.7: 生产 controller 调用示例
  - [ ] SubTask 19.8: 注意事项（api_token 必配否则中间件抛 500；demo 用 POST；stop/restart 无 id 操作 daemon）

## 阶段八：排障篇
- [ ] Task 20: 编写「故障排查」`docs/troubleshooting.md`
  - [ ] SubTask 20.1: 扩展未加载 → -d extension= / PHP_INI_SCAN_DIR
  - [ ] SubTask 20.2: daemon 启动失败 → socket 目录权限 / /run/xhjob 创建
  - [ ] SubTask 20.3: xhjob_dispatch 返回 error: → 解析前缀（服务名校验/task_json 非法/daemon 不可达）
  - [ ] SubTask 20.4: 任务卡 Running → watchdog daemon 级配置 / xhjob_cancel / lease 检查（无 TaskBuilder::watchdogTimeout 伪方法）
  - [ ] SubTask 20.5: SQLite 损坏 → PRAGMA quick_check / 备份 db
  - [ ] SubTask 20.6: FPM worker 阻塞 → XHJOB_IPC_TIMEOUT_SECS 调小
  - [ ] SubTask 20.7: persist feature 未启用 → --all-features 编译 + XHJOB_PERSIST
  - [ ] SubTask 20.8: zombie 进程残留 → pcntl_waitpid reap

## 阶段九：部署说明与验证
- [ ] Task 21: 在 `docs/README.md` 补充部署方式说明（三选一：本地 python -m http.server / GitHub Pages 静态模式 main /docs 无 Actions / 任意静态服务器）
- [ ] Task 22: 代码演示真实性核对（对照 src/lib.rs 与 releases/xhjob-thinkphp8-extend/Xhjob/*.php）
  - [ ] SubTask 22.1: 核对所有 env var 名与 src/ 读取点一致（XHJOB_SERVICE_NAME 非 XHJOB_SERVICE）
  - [ ] SubTask 22.2: 核对所有函数签名与 src/lib.rs 导出一致
  - [ ] SubTask 22.3: 核对所有 TaskBuilder/TaskManager 方法与 PHP 类一致（无伪方法如 events()/watchdogTimeout()）
  - [ ] SubTask 22.4: 核对所有 EventType 枚举值与 src/store/mod.rs 一致（lease_held 非 leaseheld）
- [ ] Task 23: 本地启动验证（`python -m http.server -d docs` 启动后访问 http://localhost:8000 确认封面+导航+内容渲染正常，无 404）

# Task Dependencies
- Task 1（清理）是 Task 2-20 的前置（避免新旧文件混淆）
- Task 2（Docsify 骨架）是 Task 3-20 的前置（提供 index.html/_sidebar.md 框架）
- Task 3-20 可在 Task 2 完成后并行（各 markdown 独立）
- Task 21（部署说明）依赖 Task 2 的 README.md 已创建
- Task 22（核对）依赖 Task 6-19 完成
- Task 23（启动验证）依赖 Task 1-22 全部完成
- 无外部代码依赖（纯 HTML/markdown 产出，不触碰 Rust/PHP 源码）
