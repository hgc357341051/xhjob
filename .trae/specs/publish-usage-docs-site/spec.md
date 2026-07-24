# Xhjob 使用说明站点（GitHub Pages）Spec

## Why
xhjob 是 Rust + ext-php-rs 实现的 PHP 异步任务调度扩展，功能完备（27 个导出函数、40+ TaskBuilder 方法、双线程池模式、链/组/chord 编排、持久化与崩溃恢复），但目前只有仓库 README 和散落的 `test_xhjob*.php` 脚本，缺少面向最终用户的结构化文档。用户在选型、集成、排障时无从下手，尤其对「双线程池模式区别」「CLI/FPM 共用服务」「ThinkPHP 8 集成」等生产关键点没有权威说明。

通过 GitHub Pages 托管一个静态文档站点，让每个功能都有「函数签名 + 注意事项 + 可运行代码演示」，并给出多个生产级端到端示例，显著降低上手与排障成本。

## What Changes
- 新增 `docs/` 目录，承载 GitHub Pages 站点源码（Jekyll + Just-The-Docs 主题，零构建依赖、原生 GitHub Pages 支持）
- 新增 `.github/workflows/docs.yml`，push 到 main 时自动构建并发布 Pages
- 新增 `docs/_config.yml`，配置 Jekyll + 主题 + 导航
- 新增 16 篇内容文档（见下文 ADDED Requirements），覆盖：快速开始、架构、安装配置、函数参考、TaskBuilder/TaskManager API、触发器、重试超时、并发控制、持久化与崩溃恢复、双线程池模式对比、编排、进度事件、生产场景演示、ThinkPHP 8 集成、故障排查
- 每篇文档遵循统一模板：概述 → 函数签名/方法签名 → 参数说明 → 返回值 → 注意事项（踩坑点）→ 可运行代码演示 → 生产建议
- 代码演示全部基于真实 API（来自 `src/lib.rs` 导出函数与 `releases/xhjob-thinkphp8-extend/Xhjob/` 类），禁止伪代码
- 双线程池模式对比页面包含：实现差异表、性能特征表、配置项、何时选用决策树、可复现的基准对比脚本说明
- 生产场景演示至少包含 4 个端到端示例：后台任务队列、定时任务（cron + 时区 + 持久化）、CLI 与 FPM 共用服务连接、ThinkPHP 8 第三方类库方式集成

## Impact
- Affected specs: 无（首个文档类 spec，与既有 `full-code-reaudit-v4` 等代码审查 spec 正交）
- Affected code:
  - 新增：`docs/`（整站）、`.github/workflows/docs.yml`、`docs/_config.yml`、`docs/index.md`、`docs/*.md`
  - 不修改任何 Rust/PHP 源码（纯文档产出）
  - 不回滚用户既有改动
- 数据来源（事实基线，写文档时必须对齐）：
  - PHP 导出函数：`/workspace/src/lib.rs`（27 个 `xhjob_*` 函数 + `Xhjob` 类）
  - 线程池：`/workspace/src/pool/coroutine_pool.rs`（async）、`/workspace/src/pool/thread_pool.rs`（thread）、`XHJOB_POOL_MODE` 读取点 `/workspace/src/scheduler/queue.rs:779`
  - TaskBuilder/TaskManager/ServiceProvider/Facade/helper：`/workspace/releases/xhjob-thinkphp8-extend/Xhjob/`
  - 配置：`/workspace/releases/xhjob-thinkphp8-extend/config/xhjob.php`、`/workspace/src/config.rs`
  - IPC/路径/CLI-FPM 共用：`/workspace/src/ipc/mod.rs`、`/workspace/src/service/mod.rs`、`/workspace/src/daemon/mod.rs`
  - 已有演示：`/workspace/releases/xhjob-thinkphp8-extend/test_xhjob*.php`、`/workspace/tp/repro/repro_*.php`

## ADDED Requirements

### Requirement: 站点骨架与发布
系统 SHALL 在 `docs/` 目录下提供 Jekyll 站点，使用 Just-The-Docs 主题，通过 `.github/workflows/docs.yml` 在 push 到 main 时自动发布到 GitHub Pages。

#### Scenario: 访问站点首页
- **WHEN** 用户访问 `https://<owner>.github.io/xhjob/`
- **THEN** 看到首页（`docs/index.md`），含项目一句话介绍、核心特性列表、快速开始入口、左侧导航树

#### Scenario: 自动部署
- **WHEN** 仓库 main 分支收到 push 且 `docs/` 或 workflow 文件有变更
- **THEN** GitHub Actions 触发 `docs.yml`，构建 Jekyll 站点并发布到 Pages，无需人工干预

### Requirement: 文档导航与统一模板
系统 SHALL 提供左侧导航树，按 16 个主题分组；每篇文档遵循统一模板：概述 → 签名 → 参数 → 返回值 → 注意事项 → 代码演示 → 生产建议。

#### Scenario: 左侧导航分组
- **WHEN** 用户浏览任意文档页
- **THEN** 左侧导航显示分组：入门（快速开始/架构/安装配置）、API 参考（函数/TaskBuilder/TaskManager）、核心能力（触发器/重试超时/并发控制/持久化崩溃恢复/编排/进度事件）、进阶（线程池模式对比）、生产实战（后台队列/定时任务/CLI-FPM 共用/ThinkPHP8 集成）、排障

### Requirement: 函数参考完整性
系统 SHALL 为 `src/lib.rs` 中导出的全部 27 个 `xhjob_*` 函数和 `Xhjob` PHP 类提供独立参考页，每个函数包含：PHP 签名、参数表（名/类型/默认/说明）、返回值、错误契约（`"error: ..."` 前缀或 `false`）、注意事项、可运行代码演示。

#### Scenario: 查询单个函数
- **WHEN** 用户打开 `xhjob_dispatch` 参考页
- **THEN** 看到 `xhjob_dispatch(string $task_json, ?string $name = null, ?string $data_dir = null): string` 签名、参数表、返回 task_id 或 `"error: ..."`、注意事项（task_json 必须是 TaskBuilder::toJson() 产物；error 前缀用于区分 task_id）、代码演示（构建+派发+读取状态）

### Requirement: TaskBuilder 与 TaskManager API 完整性
系统 SHALL 为 TaskBuilder 全部静态工厂与链式方法（40+ 个）、TaskManager 全部 public 方法提供参考页，每个方法含签名、用途、注意事项、代码演示。

#### Scenario: TaskBuilder 方法查询
- **WHEN** 用户查询 `softTimeout` 方法
- **THEN** 看到 `softTimeout(int $secs): self` 签名、说明（SIGTERM→SIGKILL 升级链）、注意事项（必须 `< timeout`；HTTP 任务不支持会抛 `InvalidTaskConfigException`）、代码演示

### Requirement: 双线程池模式对比
系统 SHALL 提供专门的线程池模式对比页，包含：async 与 thread 的实现差异表、性能特征表、配置项（`XHJOB_POOL_MODE` / `XHJOB_ASYNC_POOL_SIZE` / `XHJOB_THREAD_POOL_SIZE`）、何时选用决策树、可复现基准对比说明（引用 `test_xhjob_pool_diff.php` 的 `/proc/{pid}/task` 验证法）。

#### Scenario: 模式选型
- **WHEN** 用户阅读对比页
- **THEN** 能看到决策表：IO 密集/高并发短任务 → async；CPU 密集/强隔离/严格并发上限 → thread；并看到切换方式（`putenv("XHJOB_POOL_MODE=thread")` 后 `xhjob_start`，daemon 启动时读取一次，切换需 stop+start）

### Requirement: 生产场景端到端演示
系统 SHALL 提供至少 4 个生产级端到端示例，每个含完整可运行代码、架构图说明、注意事项。

#### Scenario: 后台任务队列
- **WHEN** 用户阅读「后台任务队列」示例
- **THEN** 看到：FPM 请求内 `xhjob_dispatch` 非阻塞派发 + 立即返回 task_id、CLI worker 轮询 `xhjob_result`、`ignoreResult` 优化、`rateLimit` 限流、进度上报、失败重试配置

#### Scenario: 定时任务
- **WHEN** 用户阅读「定时任务」示例
- **THEN** 看到：cron 表达式 + `withTimezone('Asia/Shanghai')` + `persist(true)` + `acksLate(true)` 崩溃恢复 + `maxExecutions` + `coalesce` 合并漏触发 + `skipDates` 节假日跳过

#### Scenario: CLI 与 FPM 共用服务连接
- **WHEN** 用户阅读「CLI 与 FPM 共用服务连接」示例
- **THEN** 看到：同一 `service_name` + `data_dir` 让 CLI daemon 与 FPM 请求连同一 daemon；路径解析优先级（显式参数 > `XHJOB_SOCK_DIR/PID_DIR/LOG_DIR` > `XHJOB_DATA_DIR` > 平台默认）；PID 文件双行格式（pid+starttime 防 PID 复用）；`XHJOB_IPC_TIMEOUT_SECS` 防 FPM worker 阻塞

#### Scenario: ThinkPHP 8 第三方类库集成
- **WHEN** 用户阅读「ThinkPHP 8 集成」示例
- **THEN** 看到：composer 安装方式、`ServiceProvider` 自动注册、`Xhjob` Facade 用法、`xhjob_manager()/xhjob_service()/xhjob_task()` helper、config 键、controller + route 端点、生产 controller 调用示例

### Requirement: 故障排查页
系统 SHALL 提供故障排查页，覆盖常见错误及解决方法。

#### Scenario: 常见错误查询
- **WHEN** 用户遇到 `Call to undefined function Xhjob\xhjob_status()`
- **THEN** 在排障页找到根因（扩展未加载）与解决方案（`php -d extension=<so>` 或配置 `PHP_INI_SCAN_DIR`）

### Requirement: 代码演示可运行性
系统 SHALL 保证所有代码演示基于真实 API 签名，不得使用伪代码或不存在的方法。

#### Scenario: 代码演示核对
- **WHEN** 审核任意代码演示
- **THEN** 其调用的函数/方法/参数与 `src/lib.rs` 导出或 `releases/xhjob-thinkphp8-extend/Xhjob/*.php` 一致；env var 名与 `src/` 实际读取点一致
