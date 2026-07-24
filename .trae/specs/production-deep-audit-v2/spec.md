# 生产级深度审查 v2 Spec

## Why
上一轮（commit 4b1ee5b）完成了首轮全 Rust 源文件审查并修复了 11 个生产可触发 bug，跨进程 client_test 达到 43 PASS / 0 FAIL。但用户要求**新一轮更深度**的审查评估，四个维度（正确率/代码质量/功能完善度/bug 率与错误率）通过率均要 100%，并且：
1. 必须在 `tp/` 项目里写代码**模拟真实生产环境业务逻辑**去测试效果（而非伪代码）；
2. 所有审查提出的问题必须**用代码复现出它在修正**（先触发 bug 行为，再验证修复后行为正确）；
3. 不理解或不确定的问题必须先询问用户，不得瞎猜。

本轮在本地最新 HEAD（7a71a59）基础上实施，不回退、不撤销已有工作。

## What Changes
- **新一轮全代码深度审查**：对 Rust 内核（`src/` 37 文件 ~15410 行）、PHP 集成包（`releases/xhjob-thinkphp8-extend/Xhjob/`）、tp 项目（`tp/`）做比上一轮更细致的逐函数/逐方法审查，覆盖正确性、并发安全、错误处理、资源泄漏、边界条件、API 契约一致性。
- **生产业务模拟测试套件**：在 `tp/` 下新增结构化 PHP 测试，模拟真实生产业务逻辑（如订单异步处理、库存扣减、消息推送、报表生成、定时对账等），用 xhjob 实现端到端业务流程并验证结果正确。
- **Bug 复现套件**：为本轮新发现的每个真实 bug 编写独立复现 case（先复现 bug 行为，再验证修复后行为正确）；对上一轮已修复的 11 个 bug 也补充显式复现 case，形成完整回归防护网。集中组织在 `tp/xhjob_bug_repro_suite.php`，按 bug id 分 case。
- **修复所有确认的真实问题**：对审查中确认的真实 bug 做最小化、针对性修复；对疑似问题先读代码验证，排除误报；对不确定的问题先询问用户再行动。
- **四维度 100% 验证**：
  - 正确率（正常使用而非伪代码）：跨进程 client_test + 生产业务测试 + bug 复现套件全部 PASS；
  - 代码质量：`cargo build` 0 error 0 warning、`cargo clippy` 无新增警告、PHP 代码风格一致、PHPDoc 完整；
  - 功能完善度：全部 25 个 `xhjob_*` 函数 + TaskBuilder 全部链式方法 + XhjobService 生命周期均有测试覆盖；
  - bug 率与错误率：审查发现的 bug 全部修复、运行时无 panic / 无未捕获异常 / 无永久阻塞 / 无 daemon crash。
- **编译并本地提交**：`cargo build --release --features persist` 产出新 .so，复制到 `releases/xhjob-php8.2-linux-x86_64.so`，`git add` + `git commit` 到本地 main（push 需 GitHub 凭据，本轮默认本地提交，如用户后续提供 token 再 push）。
- **BREAKING**：无（仅新增测试代码与必要的 bug 修复，不改变公开 API 契约）。

## Impact
- 受影响代码：
  - `src/`（Rust 内核，必要时修复新发现的 bug）
  - `releases/xhjob-thinkphp8-extend/Xhjob/`（PHP 集成包，必要时修复）
  - `tp/extend/Xhjob/`（运行时安装，受 .gitignore 排除，规范源在 releases/）
  - `tp/app/controller/`（必要时修复）
  - `tp/xhjob_production_business.php`（新增：生产业务模拟测试套件）
  - `tp/xhjob_bug_repro_suite.php`（新增：bug 复现套件，按 bug id 分 case）
  - `tp/xhjob_server.php`（保留，可能增强）
  - `tp/xhjob_client_test.php`（保留，可能增强）
  - `releases/xhjob-php8.2-linux-x86_64.so`（重新编译产物）
  - `target/release/libxhjob.so`（编译产物，纳入提交）
- 受影响能力：全部 25 个 `xhjob_*` 函数 + TaskBuilder 全部链式方法 + XhjobService 生命周期 + 编排原语（chain/group/chord）+ 重试/限流/重叠控制 + 持久化与崩溃恢复。

## ADDED Requirements

### Requirement: 新一轮全代码深度审查
系统 SHALL 对全部 Rust 与 PHP 代码做比上一轮更细致的逐函数/逐方法审查，覆盖正确性、并发安全、错误处理、资源泄漏、边界条件、API 契约一致性，并区分真实 bug 与误报。

#### Scenario: 审查覆盖全部模块
- **WHEN** 完成新一轮审查
- **THEN** Rust 37 个源文件（daemon/ipc/pool/executor/scheduler/store/task/utils/service/outcome/retry/config/errors/lib/daemon_main）全部逐函数审查
- **AND** PHP 集成包全部类（TaskBuilder/TaskManager/XhjobService/Client/ServiceProvider/facade/helper/Exception）全部逐方法审查
- **AND** tp 项目 Controller/配置/路由/测试脚本审查
- **AND** 每个疑似问题通过读实际代码验证，排除误报或确认真实

#### Scenario: 不确定问题先询问
- **WHEN** 审查中发现语义不明或设计意图不确定的问题
- **THEN** 必须先询问用户确认意图，不得瞎猜
- **AND** 在得到答复前不擅自修改相关代码

### Requirement: 生产业务模拟测试套件
系统 SHALL 在 `tp/xhjob_production_business.php` 中模拟真实生产环境业务逻辑，用 xhjob 实现端到端业务流程并验证结果正确（非伪代码）。

#### Scenario: 真实业务场景覆盖
- **WHEN** 运行 `php -d extension=<so> tp/xhjob_production_business.php`
- **THEN** 覆盖以下真实业务场景并全部 PASS：
  1. 订单异步处理（dispatch 订单任务 → 状态轮询 → 结果验证）
  2. 库存扣减（chain：预占 → 扣减 → 释放，失败回滚）
  3. 消息批量推送（group 并行发送 N 条消息 → 汇总结果）
  4. 定时报表生成（cron 触发 → 生成报表文件 → 验证文件存在）
  5. 定时对账（interval 触发 → 比对数据 → 记录差异）
  6. 失败补偿（任务失败 → 指数退避重试 → 最终成功或告警）
  7. 超时熔断（长任务软超时 → SIGTERM → 验证优雅退出）
  8. 限流保护（高并发 dispatch → rateLimit 限流 → 验证被限流任务延迟执行）
  9. 幂等去重（相同 id 任务重复 dispatch → 验证 replace_existing 行为）
  10. 崩溃恢复（持久化任务 → daemon 重启 → 验证 acks_late 任务重新执行）

#### Scenario: 业务测试真实可运行
- **WHEN** 执行生产业务测试套件
- **THEN** 每个场景使用真实 shell 命令 / 真实文件 IO / 真实状态轮询（非 mock、非伪代码）
- **AND** 测试结束后清理产生的临时文件与 daemon

### Requirement: Bug 复现套件
系统 SHALL 在 `tp/xhjob_bug_repro_suite.php` 中为本轮新发现的真实 bug 与上一轮已修复的 11 个 bug 各编写独立复现 case，先复现 bug 行为，再验证修复后行为正确。

#### Scenario: 每个真实 bug 有复现 case
- **WHEN** 运行 `php -d extension=<so> tp/xhjob_bug_repro_suite.php`
- **THEN** 每个已修复 bug 都有独立 case
- **AND** case 先构造触发条件复现 bug 行为（或在注释中说明修复前会如何表现）
- **AND** case 验证修复后行为正确
- **AND** 全部 case PASS

#### Scenario: 上一轮 11 个 bug 显式回归
- **WHEN** 运行 bug 复现套件
- **THEN** 覆盖上一轮修复的 11 个 bug（crypto 无效密钥、kill 截断、RateLimiter 泄漏、load_result 失败误标、shell drain 超时、service_name 注入、outcome 校验、modify_job 清空、未知状态复活、溢出安全、未知 task_type）的回归 case

### Requirement: 四维度 100% 通过率
系统 SHALL 在正确率、代码质量、功能完善度、bug 率与错误率四个维度均达到 100% 通过率。

#### Scenario: 正确率 100%
- **WHEN** 运行全部测试
- **THEN** `cargo test --features persist` 100% 通过
- **AND** 跨进程 client_test 100% PASS
- **AND** 生产业务模拟测试 100% PASS
- **AND** bug 复现套件 100% PASS

#### Scenario: 代码质量 100%
- **WHEN** 检查代码质量
- **THEN** `cargo build --release --features persist` 0 error 0 warning
- **AND** `cargo clippy --features persist -- -D warnings` 无新增警告（或基线内）
- **AND** PHP 代码风格一致、PHPDoc 完整

#### Scenario: 功能完善度 100%
- **WHEN** 检查功能覆盖
- **THEN** 全部 25 个 `xhjob_*` 函数有测试覆盖
- **AND** TaskBuilder 全部链式方法有测试覆盖
- **AND** XhjobService 生命周期方法有测试覆盖
- **AND** 编排/重试/限流/重叠/持久化/崩溃恢复均有测试覆盖

#### Scenario: bug 率与错误率 100%
- **WHEN** 检查 bug 与错误
- **THEN** 审查发现的 bug 全部修复
- **AND** 运行时无 panic / 无未捕获异常 / 无永久阻塞 / 无 daemon crash

### Requirement: 编译并本地提交
系统 SHALL 编译 .so 扩展，并将所有修改代码与 .so 提交到本地 main 分支。

#### Scenario: 本地提交成功
- **WHEN** 完成全部实施与验证
- **THEN** `cargo build --release --features persist` 产出 `target/release/libxhjob.so`
- **AND** 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- **AND** `git add` 所有修改代码 + `.so` + 新增文件
- **AND** `git commit` 创建提交到本地 main

## MODIFIED Requirements

### Requirement: tp 项目测试能力
tp 项目 SHALL 在原有跨进程测试脚本基础上，新增生产业务模拟测试套件与 bug 复现套件，提供真实生产环境业务逻辑端到端验证与 bug 回归防护。

## REMOVED Requirements
无（本轮不移除任何已有能力）。
