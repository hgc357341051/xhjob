# 站在使用者视角的代码审查与完善 Spec

## Why

项目从骨架到多服务实例、data_dir 参数一路快速演进，已具备完整功能但缺乏一次"以使用者身份"的端到端审视：用户拿到 `releases/xhjob-php8.2-linux-x86_64.so` 后，按 README 步骤能否跑通？API 命名是否直观？错误返回是否友好？示例与文档是否自洽？测试是否覆盖真实使用路径？需要在交付前系统性审查并补齐遗漏。

## What Changes

- 拉取并核对远程主分支代码，确认工作树与 `origin/main` 一致（不回滚任何用户改动）
- 站在 PHP 使用者视角审查 `src/lib.rs` 暴露的所有函数与 `Xhjob` 类 API：命名、参数顺序、返回值、错误信息、向后兼容性
- 审查 `examples/*.php` 与 `README.md`：用户照搬示例能否直接运行，文档与代码是否一致
- 审查 `tests/` 全量测试（.phpt + business + data_dir_smoke）：覆盖度、断言强度、跳过条件是否合理
- 编译验证：`cargo build --release` 与 `--features persist` 均零警告通过
- 单独执行测试验证每个功能模块的运行结果正确性（含 cron / retry / overlap / persist / 多服务 / data_dir / 代理跳过 / 编码跳过）
- 修复审查中发现的真实问题（API 不一致、文档错漏、示例不可运行、测试断言不严等），不做过度工程
- 将修复提交到本地 main 分支并 push 到 `origin/main`

## Impact

- Affected specs: `implement-async-task-scheduler`（上一轮的产物，本次做交付前打磨）
- Affected code: `src/lib.rs`、`src/task/mod.rs`、`src/daemon/*.rs`、`src/ipc/*.rs`、`src/outcome/mod.rs`、`src/store/mod.rs`、`src/scheduler/*.rs`、`src/executor/*.rs`、`examples/*.php`、`tests/*.phpt`、`tests/business/**`、`README.md`
- 不引入新功能，不删除现有 API；仅修复审查中发现的真实缺陷

## ADDED Requirements

### Requirement: 使用者视角的端到端验证
The system SHALL 在交付前通过"用户照搬 README + examples 能否跑通"的端到端验证。

#### Scenario: 用户加载预编译扩展
- **WHEN** 用户从 `releases/` 下载 `.so` 并在 php.ini 加载
- **THEN** `php -m` 列出 `xhjob`，`function_exists('xhjob_start')` 返回 true

#### Scenario: 用户照搬 README 基础示例
- **WHEN** 用户复制 README"基础用法"代码块运行
- **THEN** daemon 启动、HTTP/Shell 任务 dispatch、state/result 查询、stop 全部正常工作

#### Scenario: 用户使用 data_dir 参数
- **WHEN** 用户调用 `xhjob_start('svc', '/var/lib/xhjob')` 并投递持久化任务
- **THEN** pid/sock/db/log 全部落在 `/var/lib/xhjob`，stop 后 db 保留可用于恢复

### Requirement: 文档与代码一致性
The system SHALL 保证 README、examples、spec 文档与实际代码行为一致。

#### Scenario: API 签名一致
- **WHEN** 用户对照 README 函数签名表与实际 `ReflectionFunction`
- **THEN** 参数名、参数顺序、可选性、返回类型完全一致

#### Scenario: 示例可运行
- **WHEN** 用户执行 `examples/*.php` 任一脚本
- **THEN** 脚本不因扩展 API 缺失或签名不符而 fatal error（依赖外部网络的 HTTP 示例可降级为 SKIP）

## MODIFIED Requirements

### Requirement: 测试覆盖度
在原有测试基础上，要求：
- 全量测试在本机 PHP 8.2 环境实际执行（非仅代码审查）
- 每个功能模块至少有一个测试能独立验证其执行结果正确性
- 跳过条件（SKIP）必须真实反映环境限制（无网络/Windows-only），而非掩盖失败

## REMOVED Requirements

（无移除项——本次为审查与打磨，不删除任何现有能力）
