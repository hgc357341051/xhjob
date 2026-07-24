# Xhjob 纯静态网页文档站点（Docsify）Spec

## Why
上一轮 `publish-usage-docs-site` 用 Jekyll + GitHub Actions 流水线交付了文档站点，但用户要求「网页版，不需要流水线自动编译打包」——即去掉 Ruby/Jekyll 构建依赖与 CI 流水线，改成零构建的纯静态网页站点。

Docsify 是最匹配方案：一个 `index.html` + 一组 markdown 文件，浏览器直接打开即可渲染，无需 `npm run build`、无需 Ruby、无需 GitHub Actions。部署可选 GitHub Pages 的 "Deploy from a branch" 静态模式（无构建步骤）、本地 `python -m http.server`、或任意静态服务器。

## What Changes
- **BREAKING（相对上一轮 Jekyll 方案）**：删除 `.github/workflows/docs.yml`（GitHub Actions 流水线）
- **BREAKING**：删除 `docs/_config.yml`（Jekyll 配置）
- **BREAKING**：删除现有 `docs/*.md` 全部 Jekyll 文档（含 `index.md`、6 个分组索引页 `getting-started.md`/`api-reference.md`/`core-features.md`/`advanced.md`/`production.md`/`support.md`、19 篇内容文档）——这些文档带 Jekyll front matter，无法被 Docsify 直接复用
- 新增 Docsify 入口：`docs/index.html`（引入 docsify.js + 插件，配置侧边栏/封面/搜索/代码高亮/主题/自定义样式）
- 新增 `docs/_sidebar.md`（Docsify 左侧导航，6 分组层级，带分组标题分隔与图标 emoji）
- 新增 `docs/_coverpage.md`（封面页：项目 Logo + 项目名 + 一句话简介 + 徽章（PHP 版本/Rust/License/Stars）+ 三个 CTA 按钮：快速开始/API 参考/GitHub + 背景渐变色）
- 新增 `docs/.nojekyll`（告知 GitHub Pages 静态模式不忽略 `_` 开头文件）
- 新增 `docs/README.md`（Docsify 默认首页内容：项目介绍 + 核心特性卡片网格 + 快速开始代码 + 三种部署方式 + 技术栈说明）
- 新增 `docs/assets/css/custom.css`（自定义主题样式：配色变量、字体、代码块、表格、徽章、卡片、封面渐变、暗色模式适配、移动端响应式）
- 新增 `docs/assets/img/logo.svg`（项目 Logo SVG，用于封面与导航栏）
- 新增 19 篇内容 markdown（无 front matter，纯 GFM）：覆盖快速开始/架构/安装配置/函数参考/TaskBuilder/TaskManager/触发器/重试超时/并发控制/持久化崩溃恢复/编排/进度事件/线程池模式对比/后台队列/定时任务/CLI-FPM 共用/ThinkPHP8 集成/排障
- 每篇文档遵循统一模板：概述 → 函数签名/方法签名 → 参数说明 → 返回值 → 注意事项 → 代码演示 → 生产建议
- 代码演示全部基于真实 API（`src/lib.rs` 导出函数 + `releases/xhjob-thinkphp8-extend/Xhjob/` 类），禁止伪代码
- 双线程池模式对比页含：实现差异表、性能特征表、配置项、选型决策树、可复现基准对比说明
- 生产场景演示至少 4 个端到端示例：后台任务队列、定时任务、CLI 与 FPM 共用、ThinkPHP 8 集成

## Impact
- Affected specs: 取代 `publish-usage-docs-site`（该 spec 产物将被删除并由本 spec 取代）
- Affected code:
  - 删除：`.github/workflows/docs.yml`、`docs/_config.yml`、整个 `docs/*.md`
  - 新增：`docs/index.html`、`docs/_sidebar.md`、`docs/_coverpage.md`、`docs/.nojekyll`、`docs/README.md`、`docs/*.md`（19 篇内容）
  - 不修改任何 Rust/PHP 源码（纯文档/HTML 产出）
  - 不回滚用户既有改动（上一轮 docs/ 是 spec 工作产物，本次按用户明确指令删除重建）
- 数据来源（事实基线，写文档时必须对齐）：
  - PHP 导出函数：`/workspace/src/lib.rs`（27 个 `xhjob_*` 函数 + `Xhjob` 类，camelCase 方法）
  - 线程池：`/workspace/src/pool/coroutine_pool.rs`（async）、`/workspace/src/pool/thread_pool.rs`（thread）、`XHJOB_POOL_MODE` 读取点 `/workspace/src/scheduler/queue.rs`
  - TaskBuilder/TaskManager/ServiceProvider/Facade/helper：`/workspace/releases/xhjob-thinkphp8-extend/Xhjob/`
  - 配置：`/workspace/releases/xhjob-thinkphp8-extend/config/xhjob.php`、`/workspace/src/config.rs`
  - IPC/路径/CLI-FPM 共用：`/workspace/src/ipc/mod.rs`、`/workspace/src/service/mod.rs`、`/workspace/src/daemon/mod.rs`
  - EventType 枚举：`/workspace/src/store/mod.rs`（多词值用下划线：`lease_held`/`hung_detected`/`max_instances_reached`/`rate_limited`）
  - env var 实际读取点：用 Grep 在 `/workspace/src/` 搜 `std::env::var("XHJOB_` 收集真实变量名

## ADDED Requirements

### Requirement: Docsify 零构建站点骨架
系统 SHALL 在 `docs/` 目录下提供 Docsify 站点：一个 `index.html` 引入 docsify.js（CDN）+ 搜索插件 + 代码高亮插件 + emoji 插件，配置 `loadSidebar`、`coverpage`、`subMaxLevel`、`search` 选项。无需任何构建步骤，浏览器直接打开 `docs/index.html` 即可渲染。

#### Scenario: 本地直接打开
- **WHEN** 用户在文件管理器双击 `docs/index.html`，或执行 `python -m http.server -d docs`
- **THEN** 浏览器渲染出封面页 + 左侧导航 + 右侧内容，无需预先 `npm install` 或 `bundle install`

#### Scenario: 无 CI 流水线
- **WHEN** 仓库 main 分支收到 push
- **THEN** 不触发任何 GitHub Actions 构建任务（`.github/workflows/docs.yml` 已删除）；部署采用 GitHub Pages "Deploy from a branch" 静态模式（main / `/docs`）或其他静态服务器，全程无编译

### Requirement: 左侧导航与封面
系统 SHALL 通过 `docs/_sidebar.md` 提供 6 分组左侧导航；通过 `docs/_coverpage.md` 提供封面页（项目名、一句话简介、快速开始按钮）。

#### Scenario: 导航分组
- **WHEN** 用户浏览任意内容页
- **THEN** 左侧导航显示分组：入门（快速开始/架构/安装配置）、API 参考（函数/TaskBuilder/TaskManager）、核心能力（触发器/重试超时/并发控制/持久化崩溃恢复/编排/进度事件）、进阶（线程池模式对比）、生产实战（后台队列/定时任务/CLI-FPM 共用/ThinkPHP8 集成）、排障

### Requirement: 函数参考完整性
系统 SHALL 为 `src/lib.rs` 导出的全部 27 个 `xhjob_*` 函数和 `Xhjob` PHP 类提供参考内容，每个函数含：PHP 签名、参数表、返回值、错误契约（`"error: ..."` 前缀或 `false`）、注意事项、可运行代码演示。

#### Scenario: 查询单个函数
- **WHEN** 用户在导航点击「PHP 函数参考」并搜索 `xhjob_dispatch`
- **THEN** 看到 `xhjob_dispatch(string $task_json, ?string $name = null, ?string $data_dir = null): string` 签名、参数表、返回 task_id 或 `"error: ..."`、注意事项、代码演示

### Requirement: TaskBuilder 与 TaskManager API 完整性
系统 SHALL 为 TaskBuilder 全部静态工厂与链式方法（40+）、TaskManager 全部 public 方法提供参考内容，每个方法含签名、用途、注意事项、代码演示。

### Requirement: 双线程池模式对比
系统 SHALL 提供专门的线程池模式对比页，含 async 与 thread 的实现差异表、性能特征表、配置项（`XHJOB_POOL_MODE`/`XHJOB_ASYNC_POOL_SIZE`/`XHJOB_THREAD_POOL_SIZE`）、选型决策树、可复现基准对比说明（引用 `test_xhjob_pool_diff.php` 的 `/proc/{pid}/task` 验证法）。

#### Scenario: 模式选型
- **WHEN** 用户阅读对比页
- **THEN** 能看到决策表：IO 密集/高并发短任务 → async；CPU 密集/强隔离/严格并发上限 → thread；切换方式（`putenv("XHJOB_POOL_MODE=thread")` 后 `xhjob_start`，切换需 stop+start）

### Requirement: 生产场景端到端演示
系统 SHALL 提供至少 4 个生产级端到端示例，每个含完整可运行代码、架构说明、注意事项。

#### Scenario: 后台任务队列
- **WHEN** 用户阅读「后台任务队列」示例
- **THEN** 看到：FPM 请求内 `xhjob_dispatch` 非阻塞派发 + 立即返回 task_id、CLI worker 轮询 `xhjob_result`、`ignoreResult` 优化、`rateLimit` 限流、进度上报、失败重试配置

#### Scenario: 定时任务
- **WHEN** 用户阅读「定时任务」示例
- **THEN** 看到：cron + `withTimezone('Asia/Shanghai')` + `persist(true)` + `acksLate(true)` 崩溃恢复 + `maxExecutions` + `coalesce` 合并漏触发 + `skipDates` 节假日跳过

#### Scenario: CLI 与 FPM 共用服务连接
- **WHEN** 用户阅读「CLI 与 FPM 共用服务连接」示例
- **THEN** 看到：同一 `service_name` + `data_dir` 让 CLI daemon 与 FPM 请求连同一 daemon；路径解析优先级；PID 文件双行格式防 PID 复用；`XHJOB_IPC_TIMEOUT_SECS` 防 FPM worker 阻塞

#### Scenario: ThinkPHP 8 第三方类库集成
- **WHEN** 用户阅读「ThinkPHP 8 集成」示例
- **THEN** 看到：composer 安装、`ServiceProvider` 自动注册、`Xhjob` Facade、helper 函数、config 键、controller + route 端点、生产 controller 调用示例

### Requirement: 故障排查页
系统 SHALL 提供故障排查页，覆盖常见错误及解决方法。

### Requirement: 代码演示真实性
系统 SHALL 保证所有代码演示基于真实 API 签名，不得使用伪代码或不存在的方法。

#### Scenario: 代码演示核对
- **WHEN** 审核任意代码演示
- **THEN** 其调用的函数/方法/参数与 `src/lib.rs` 导出或 `releases/xhjob-thinkphp8-extend/Xhjob/*.php` 一致；env var 名与 `src/` 实际读取点一致；EventType 枚举值与 `src/store/mod.rs` 一致（`lease_held` 非 `leaseheld`）

### Requirement: 可选部署方式说明
系统 SHALL 在 `docs/README.md` 或专门部署章节说明三种零构建部署方式，不强制任何一种。

#### Scenario: 部署方式查询
- **WHEN** 用户想知道如何发布站点
- **THEN** 文档列出：① 本地预览 `python -m http.server -d docs`；② GitHub Pages 静态模式（Settings → Pages → Source: Deploy from a branch → main `/docs`，无需 Actions）；③ 任意静态服务器（nginx/apache/CDN）直接托管 `docs/` 目录

### Requirement: 站点视觉主题与配色
系统 SHALL 通过 `docs/assets/css/custom.css` 提供统一的视觉主题，包含品牌配色、字体、组件样式，并支持亮色/暗色双模式。

#### Scenario: 品牌配色一致性
- **WHEN** 用户浏览任意页面
- **THEN** 主色调为科技蓝（`#3B82F6` 主色 / `#1E40AF` 深色），强调色橙（`#F59E0B`），中性色用 Tailwind 调色板（gray-50 到 gray-900）；链接、按钮、标题装饰线、徽章边框均使用主色调

#### Scenario: 暗色模式自动适配
- **WHEN** 用户系统处于暗色模式，或手动切换主题
- **THEN** 站点自动切换暗色配色（背景 `#0F172A`、文字 `#E2E8F0`、代码块 `#1E293B`），并提供右上角主题切换按钮（亮/暗/跟随系统三态）

### Requirement: 封面页设计
系统 SHALL 通过 `docs/_coverpage.md` + custom.css 提供视觉冲击力的封面页。

#### Scenario: 封面首屏
- **WHEN** 用户首次打开站点
- **THEN** 看到全屏渐变背景（蓝紫渐变 `linear-gradient(135deg, #3B82F6, #8B5CF6)`）、居中 Logo（SVG）、超大标题「Xhjob」、副标题「PHP 异步任务调度扩展 · Rust 驱动」、一行徽章（`PHP 8.0+` `Rust` `Apache-2.0` `Linux`）、三个 CTA 按钮（「快速开始」主色实心 / 「API 参考」描边 / 「GitHub」描边带图标）

### Requirement: 首页特性卡片网格
系统 SHALL 在 `docs/README.md` 首页用卡片网格展示核心特性，每张卡片含图标 + 标题 + 一句话描述。

#### Scenario: 特性卡片展示
- **WHEN** 用户进入首页（封面后第一页）
- **THEN** 看到 6-8 张特性卡片网格（响应式：桌面 3 列 / 平板 2 列 / 手机 1 列），每张卡片含 emoji 图标、特性名（如「双线程池」「持久化崩溃恢复」「链式编排」）、一句话描述；卡片有悬浮阴影动画

### Requirement: 代码块增强
系统 SHALL 通过 prism.js 插件 + custom.css 提供增强的代码块体验。

#### Scenario: 代码块交互
- **WHEN** 用户浏览含代码示例的页面
- **THEN** 代码块支持：① PHP/Rust/Bash/JSON 语法高亮（prism 主题与站点配色协调）；② 右上角「复制」按钮（点击后变「已复制」2 秒）；③ 行号显示（可选）；④ 横向滚动不溢出

### Requirement: 表格美化
系统 SHALL 通过 custom.css 美化所有 markdown 表格，提升可读性。

#### Scenario: 表格渲染
- **WHEN** 页面含 markdown 表格（如函数参数表、env var 全表、线程池对比表）
- **THEN** 表格有：① 表头主色背景 + 白色文字；② 斑马纹行（奇数行浅灰背景）；③ hover 行高亮；④ 圆角边框；⑤ 移动端横向滚动

### Requirement: 全文搜索体验
系统 SHALL 通过 docsify-search 插件提供实时全文搜索。

#### Scenario: 搜索框
- **WHEN** 用户在顶部搜索框输入关键词（如 `softTimeout`、`lease_held`、`XHJOB_POOL_MODE`）
- **THEN** 实时显示匹配结果列表（标题 + 预览片段 + 高亮关键词），点击跳转对应章节；无结果时显示友好提示

### Requirement: 导航增强
系统 SHALL 通过 `_sidebar.md` + custom.css 提供分组清晰、带层级缩进的左侧导航。

#### Scenario: 导航分组与图标
- **WHEN** 用户展开左侧导航
- **THEN** 看到 6 个分组（入门/API 参考/核心能力/进阶/生产实战/排障），每组有分组标题（带 emoji 图标如 🚀/📚/⚙️/🔬/🏭/🛠️）+ 分隔线；子项缩进；当前页高亮；支持折叠展开

### Requirement: 移动端响应式
系统 SHALL 通过 custom.css 媒体查询保证移动端可用性。

#### Scenario: 手机访问
- **WHEN** 用户用手机（< 768px）访问站点
- **THEN** 侧边栏默认收起（汉堡菜单切换）、内容区全宽、特性卡片单列、表格横向滚动、代码块横向滚动、封面字号缩小适配
