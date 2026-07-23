# 代码审查与生产环境业务模拟测试 Spec

## Why
用户要求对当前仓库（基于 Rust + ext-php-rs 的 PHP 异步任务调度扩展 xhjob，以及 ThinkPHP 8 集成包与 tp 演示项目）的全部代码进行审查评估，使**代码质量、功能完善度、bug 率、错误率**四个维度分别达到 100% 通过率；同时要求在 **tp 项目**中编写代码，模拟真实生产环境业务逻辑去端到端测试扩展的实际效果，确保扩展在生产场景下稳定可靠。

当前仓库已处于 main 分支并已 pull 到最新（commit 9b8e8ea），工作树干净。tp 项目的 `extend/Xhjob/` 集成包尚未安装到位，现有测试脚本（test_xhjob_*.php）以单元/修复验证为主，缺少覆盖真实业务流水线的端到端模拟测试。

## What Changes
- **代码审查（Rust 内核 `src/`）**：审查 daemon / executor / ipc / pool / scheduler / store / task / service / utils / config / errors 各模块，定位 bug、并发隐患、错误处理缺口，并修复至 `cargo build --release --features persist` 与 `cargo test --features persist` 100% 通过。
- **代码审查（PHP 集成包 `releases/xhjob-thinkphp8-extend/Xhjob/`）**：审查 TaskBuilder / TaskManager / XhjobService / Client / ServiceProvider / facade / Exception / helper，定位逻辑错误与契约不一致问题并修复。
- **代码审查（tp 项目 `tp/`）**：审查 `app/controller/XhjobTask.php`、`config/xhjob.php`、`route/app.php`、`app/service.php` 及现有测试脚本，修复发现的 bug。
- **安装集成包到 tp**：将 `releases/xhjob-thinkphp8-extend/Xhjob/` 复制到 `tp/extend/Xhjob/`，确保 tp 项目可直接运行生产模拟测试。
- **新增生产环境业务逻辑模拟测试**：在 tp 项目中编写覆盖典型生产场景的端到端测试代码，场景包括但不限于：
  - 订单异步处理（HTTP 下单 → shell 生成发货单 → chain 流水线）
  - 定时报表生成（cron 触发 + 进度上报 + 结果回查）
  - 批量数据 ETL（group 并行抽取 → chord 汇总回调）
  - 定时清理任务（cron + maxExecutions + 重试退避）
  - 邮件/通知发送（HTTP 回调 + 失败重试 + idempotent 标记）
  - 延迟任务（countdown 延迟派发）
  - 限流与并发控制（rateLimit + maxInstances 验证）
  - 持久化与崩溃恢复（daemon 重启后任务恢复）
- **测试运行与验证**：运行全部 Rust 测试 + tp 生产模拟测试，确认四个维度（质量/完善度/bug 率/错误率）100% 通过。
- **BREAKING**：无（仅新增测试代码与必要的 bug 修复，不改变公开 API 契约）。

## Impact
- 受影响代码：
  - `src/`（Rust 内核，必要时修复 bug）
  - `releases/xhjob-thinkphp8-extend/Xhjob/`（PHP 集成包，必要时修复 bug）
  - `tp/extend/Xhjob/`（新增：从 releases 复制安装）
  - `tp/app/controller/`（新增生产模拟测试 Controller）
  - `tp/route/app.php`（新增测试路由）
  - `tp/test_xhjob_production.php`（新增：生产业务模拟测试脚本，CLI 入口）
- 受影响能力：xhjob 全部能力（dispatch / chain / group / chord / cron / countdown / retry / timeout / rate_limit / maxInstances / persist / progress / events / inspect）。

## ADDED Requirements

### Requirement: 全量代码审查
系统 SHALL 对 Rust 内核、PHP 集成包、tp 项目三层代码完成审查，输出问题清单并对发现的问题进行修复，使代码质量与功能完善度达到 100% 通过。

#### Scenario: Rust 内核审查通过
- **WHEN** 执行 `cargo build --release --features persist`
- **THEN** 编译成功无 warning-as-error
- **WHEN** 执行 `cargo test --features persist`
- **THEN** 所有单元测试 100% 通过

#### Scenario: PHP 集成包审查通过
- **WHEN** 审查 `releases/xhjob-thinkphp8-extend/Xhjob/` 下所有 PHP 文件
- **THEN** 无逻辑错误、契约不一致、未处理异常

#### Scenario: tp 项目审查通过
- **WHEN** 审查 `tp/app/controller/XhjobTask.php`、`tp/config/xhjob.php`、`tp/route/app.php`、`tp/app/service.php` 及现有测试脚本
- **THEN** 无 bug、无路由缺失、无配置错误

### Requirement: 生产环境业务逻辑模拟测试
系统 SHALL 在 tp 项目中提供覆盖典型生产场景的端到端测试代码，模拟真实业务流水线对 xhjob 扩展进行集成验证。

#### Scenario: 生产模拟测试安装就绪
- **WHEN** 将集成包安装到 `tp/extend/Xhjob/`
- **THEN** tp 项目可通过 `use Xhjob\TaskBuilder` 等命名空间正常加载集成包

#### Scenario: 生产模拟测试覆盖核心场景
- **WHEN** 执行 `php -d extension=<xhjob.so> tp/test_xhjob_production.php`
- **THEN** 覆盖以下场景并全部 PASS：
  1. 订单异步处理（chain 流水线）
  2. 定时报表生成（cron + progress + 结果回查）
  3. 批量 ETL（group + chord 汇总）
  4. 定时清理（cron + maxExecutions + retry）
  5. 通知发送（HTTP + idempotent + 重试）
  6. 延迟任务（countdown）
  7. 限流与并发控制（rateLimit + maxInstances）
  8. 持久化与崩溃恢复（daemon 重启恢复）

#### Scenario: 四维度 100% 通过
- **WHEN** 运行全部 Rust 单元测试 + tp 生产模拟测试
- **THEN** 代码质量通过率 100%
- **AND** 功能完善度通过率 100%
- **AND** bug 率通过率 100%（无残留 bug）
- **AND** 错误率通过率 100%（无运行时错误）

## MODIFIED Requirements

### Requirement: tp 项目演示能力
tp 项目 SHALL 在原有 `XhjobTask` Controller 基础上，新增生产模拟测试 Controller 与路由，提供 HTTP 触发入口与 CLI 测试入口，覆盖真实生产业务场景。
