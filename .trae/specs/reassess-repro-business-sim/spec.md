# 二次审查 + 生产业务模拟 + 复现驱动修复 Spec

## Why
上一轮已完成全量 Rust 审查并修复 11 个真实 bug（本地 commit 4b1ee5b），通过 143 单测 + 43 跨进程测试。但上一轮修复是"直接改"，未对每个 bug 先写代码复现；且现有测试（`tp/xhjob_client_test.php`）是 API 函数级测试，未模拟真实生产业务流。用户要求本轮：(1) 对已修改的代码重新做五维度评估（正确率/代码质量/功能完善度/bug率/错误率，各 100%）；(2) 在 tp/ 写生产环境业务逻辑模拟（电商订单全链路）测试真实效果；(3) 对发现的每个问题必须先用代码复现再修复，并对上一轮 11 个已修复 bug 补写复现脚本作为回归锁定。

> 默认假设（用户跳过澄清问题，按推荐项推进，可在审阅时修正）：
> 1. 业务域 = 电商订单全链路（单域覆盖最广）。
> 2. 复现形式 = 每个问题一个独立 `tp/repro/bug_<编号>_<简述>.php`，修复前 FAIL / 修复后 PASS；Rust 侧 bug 同时补 cargo test 回归用例。
> 3. 审查范围 = 聚焦找本轮新问题 + 对上一轮 11 个已修复 bug 补写回归复现脚本。
> 4. 完善度定义 = 现有/文档承诺功能全可用 + 补齐明显缺口，不新增未规划功能。

## What Changes
- **重新审查全部代码**：对 37 个 Rust 源文件 + PHP 集成包 + tp 项目做二次审查，聚焦上一轮修复后是否引入新问题、是否遗漏边界。
- **电商订单全链路业务模拟**：在 `tp/business/order_pipeline.php` 以"下单→支付回调→发货→通知→对账"全链路串联 chain/group/chord/cron/HTTP/Shell/retry/rateLimit/maxInstances/encoding 全部能力，真实业务调用（非伪代码）。
- **复现驱动修复**：对每个新发现问题，先在 `tp/repro/bug_<编号>_<简述>.php` 写独立复现脚本（修复前 FAIL / 修复后 PASS），再修复；Rust 侧 bug 同时补 cargo test 回归用例。
- **11 个已修复 bug 回归复现**：对上一轮 11 个 bug（crypto 密钥缓存、daemon kill 截断、RateLimiter 泄漏、shell drain 误判、service_name 注入、modify_job 清空、未知状态复活、溢出安全等）补写 `tp/repro/bug_001~011` 复现脚本，锁定回归。
- **五维度 100% 通过率评估**：正确率/代码质量/功能完善度/bug率/错误率各自 100%，结果落到 checklist。
- **编译并提交**：`cargo build --release --features persist` 产出 .so，复制到 `releases/`，`git commit` + `git push origin main`。
- **BREAKING**：无（仅新增业务模拟/复现脚本与必要的 bug 修复，不改变公开 API 契约）。

## Impact
- 受影响代码：
  - `src/`（Rust 内核，按审查结果修复；Rust 侧 bug 补 cargo test 回归用例）
  - `releases/xhjob-thinkphp8-extend/Xhjob/`（PHP 集成包，必要时修复）
  - `tp/business/order_pipeline.php`（新增：电商订单全链路业务模拟）
  - `tp/repro/`（新增：复现脚本目录，bug_001~011 回归 + 本轮新发现 bug_<N>_<desc>）
  - `tp/extend/Xhjob/`（从 releases 同步更新）
  - `target/release/libxhjob.so` + `releases/xhjob-php8.2-linux-x86_64.so`（编译产物，纳入提交）
- 受影响能力：全部 27 个 `xhjob_*` 函数 + TaskBuilder 链式 API + XhjobService 生命周期 + chain/group/chord 编排。

## ADDED Requirements

### Requirement: 电商订单全链路业务模拟
系统 SHALL 在 `tp/business/order_pipeline.php` 提供以电商订单全链路为载体的生产业务模拟脚本，用真实业务调用（非伪代码）串联 xhjob 全部核心能力。

#### Scenario: 订单全链路串联多类任务
- **WHEN** 运行 `php -d extension=<so> tp/business/order_pipeline.php`
- **THEN** 覆盖：下单(Shell 生成订单文件) → 支付回调(HTTP) → 发货(chain 三步流水线) → 通知(group 并行 邮件/短信/推送) → 对账(chord header 汇总 + callback 生成报表)
- **AND** 期间触发 cron 夜间结算、retry 支付失败重试、rateLimit 通知限流、maxInstances 防重复发货、encoding GBK 发票
- **AND** 全链路断言通过，输出 PASS

#### Scenario: 业务模拟基于独立 daemon
- **WHEN** 业务模拟运行
- **THEN** 连接独立运行 daemon（跨进程），非单进程内嵌
- **AND** daemon 在业务模拟脚本退出后仍独立存活

### Requirement: 复现驱动修复
系统 SHALL 对审查发现的每个新问题，先写独立复现脚本暴露问题，再修复；复现脚本修复前 FAIL、修复后 PASS。

#### Scenario: 新发现问题先复现再修复
- **WHEN** 审查发现一个新 bug
- **THEN** 先在 `tp/repro/bug_<编号>_<简述>.php` 写复现脚本
- **AND** 运行复现脚本确认当前代码触发 bug（输出 FAIL / 错误现象）
- **AND** 修复后重跑复现脚本输出 PASS
- **AND** Rust 侧 bug 同时在 `src/` 对应模块补 cargo test 回归用例

### Requirement: 11 个已修复 bug 回归复现
系统 SHALL 对上一轮 11 个已修复 bug 补写复现脚本，作为回归锁定。

#### Scenario: 已修复 bug 回归锁定
- **WHEN** 补写 `tp/repro/bug_001~011` 复现脚本
- **THEN** 每个脚本针对该 bug 的触发条件构造输入
- **AND** 在当前已修复代码上运行均输出 PASS（证明未回归）
- **AND** 覆盖：bug_001 crypto 密钥缓存、bug_002 daemon kill 截断、bug_003 RateLimiter 泄漏、bug_004 shell drain 误判、bug_005 service_name 注入、bug_006 outcome 路径校验、bug_007 modify_job 清空 trigger、bug_008 未知状态复活、bug_009 未知 task_type、bug_010 next_fire 溢出、bug_011 attempts/next_retry 溢出

### Requirement: 五维度 100% 通过率评估
系统 SHALL 对正确率/代码质量/功能完善度/bug率/错误率五个维度各自评估并达 100%。

#### Scenario: 五维度全通过
- **WHEN** 完成二次审查 + 业务模拟 + 复现驱动修复
- **THEN** 正确率 100%（全部功能真实业务调用行为正确）
- **AND** 代码质量 100%（cargo build 0 warning、风格一致、关键路径有注释、PHP strict_types + PHPDoc）
- **AND** 功能完善度 100%（现有/文档承诺功能全可用，明显缺口已补齐）
- **AND** bug率 100%（0 已知未修复 bug，全部经复现脚本锁定）
- **AND** 错误率 100%（0 runtime panic/fatal/uncaught，错误路径有处理）

## MODIFIED Requirements

### Requirement: tp 项目测试能力
tp 项目 SHALL 在 API 级 `xhjob_client_test.php` 基础上，新增业务级全链路模拟（`tp/business/`）与复现驱动修复脚本目录（`tp/repro/`），提供生产业务视角的端到端验证与 bug 回归锁定。
