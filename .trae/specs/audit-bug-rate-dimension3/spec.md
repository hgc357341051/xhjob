# 维度 3 Bug 率深度审计 Spec

## Why
对 /workspace Rust 项目（src/ 全部 .rs 文件）及 PHP 扩展（releases/xhjob-thinkphp8-extend/）进行 35 项深度 bug 率审计，验证状态机正确性、边界与数据完整性、并发与资源、以及重点潜在 bug 是否达标（通过率 ≥ 0.97，即 ≤ 1 项 FAIL）。

## What Changes
- 执行只读审计：逐项验证 35 个检查项（状态机 7 项 + 边界/数据完整性 13 项 + 并发/资源 6 项 + 重点潜在 bug 9 项）
- 每项给出 PASS/FAIL + 证据（文件:行号 + 简述）
- 发现 1 项 MEDIUM 级别 bug（SQLite delete_task/remove_task 未使用事务，崩溃可致孤儿行），因非 CRITICAL 不当场修复，仅记录并建议后续修复
- 不修改任何代码（只读审计任务）

## Impact
- Affected specs: 无（只读审计，不改变现有能力）
- Affected code: 审计覆盖 src/*.rs 与 releases/xhjob-thinkphp8-extend/*.php，但不改动代码
- 审计结论：34/35 PASS，通过率 0.9714 ≥ 0.97 目标

## ADDED Requirements
### Requirement: 35 项 Bug 率审计验证
系统 SHALL 通过逐项代码审查验证以下 35 个检查项，每项给出 PASS/FAIL 与文件:行号证据。

#### Scenario: 状态机正确性（项 1-7）
- **WHEN** 审查 Pending→Running / Running→Success|Failed / Running→Interrupted / Pending→Cancelled / Pending→Expired / Interrupted|Running→Pending 重置 / 非法状态跳转
- **THEN** 全部 7 项 PASS（状态转换均有状态守卫）

#### Scenario: 边界与数据完整性（项 8-20）
- **WHEN** 审查空输入/超大输入/并发竞争/时区跨天/cron边界/SQLite事务原子性/加解密/serde/u32溢出/时间戳溢出/UTF-8/SQL注入/命令注入
- **THEN** 12 项 PASS，1 项 FAIL（项 13：delete_task/remove_task/cancel_task 未使用事务）

#### Scenario: 并发与资源（项 21-26）
- **WHEN** 审查 OverlapController/worker_limits/in-flight计数/进程回收/文件句柄/锁释放
- **THEN** 全部 6 项 PASS

#### Scenario: 重点潜在 bug（项 27-35）
- **WHEN** 审查 scan_once推进/max_executions时序/retry_backoff/acks_on_failure/ignore_result/replace_existing/chord回调/chain终止/group partial_failed
- **THEN** 全部 9 项 PASS

## MODIFIED Requirements
无（只读审计，不修改任何现有需求）。

## REMOVED Requirements
无。
