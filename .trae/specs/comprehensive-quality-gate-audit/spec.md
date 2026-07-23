# 全面代码质量门审计 Spec

## Why

经过多轮功能补全与 bug 修复（已交付 max_instances 并发限制、Interrupted 状态、HTTP 二进制响应、Shell stdin/cwd/env、PHP 客户端修复等 12+ 项修复），代码库已趋于稳定。用户现要求对**全部代码**进行最终质量门审计，确保四个维度各自通过率 **≥97%**：

1. **代码质量**（Code Quality）—— 可读性、可维护性、错误处理规范、资源管理、并发安全、命名一致性
2. **功能完善度**（Feature Completeness）—— 对比 spec / README / EVALUATION.md 中承诺的功能，实际实现是否完整可用
3. **bug 率**（Bug Rate）—— 逻辑错误、边界条件错误、数据损坏、状态机错误、并发竞态；通过率 = (已检查项 - 发现的 bug) / 已检查项
4. **错误率**（Error Rate）—— 错误处理缺失、错误吞没、错误信息误导、panic 风险、unwrap/expect 滥用；通过率 = (已检查项 - 发现的错误处理缺陷) / 已检查项

若任一维度通过率 < 97%，需修复缺陷直至达标；若已达 97%，输出审计报告即可。

## What Changes

### 审计范围（不修改功能，仅评估 + 必要修复）

- **Rust 核心库**（`src/`，~14,375 LOC，~30 个 .rs 文件）：
  - `store/`（mod / in_memory / sqlite / crypto）
  - `scheduler/`（mod / cron / queue / overlap / events / chain / chord / group / rate_limit）
  - `executor/`（mod / http / shell）
  - `ipc/`、`daemon/`、`pool/`、`retry/`、`outcome/`、`task/`、`utils/`、`service/`、`config.rs`、`errors.rs`、`lib.rs`、`daemon_main.rs`
- **PHP ThinkPHP 8 扩展**（`releases/xhjob-thinkphp8-extend/`，~6,269 LOC）：
  - `Xhjob/`（Client / TaskBuilder / TaskManager / XhjobService / ServiceProvider / facade / Exception / helper.php）
  - `config/`、`controller/`、`route/`
  - `README.md` / `EVALUATION.md` / `composer.json` / 测试脚本
- **构建与测试**：`Cargo.toml`、`build.rs`、`README.md`、`AUDIT_REPORT.md`

### 四维审计（每维通过率目标 ≥97%）

#### 维度 1：代码质量审计
- 编译无 warning（`cargo build --all-features` + `cargo build`）
- `cargo clippy --all-features -- -D warnings` 通过（允许 `#[allow]` 标注的明确例外）
- 无 `unwrap()` / `expect()` / `panic!()` 在非启动路径滥用
- 无 `todo!()` / `unimplemented!()` / `unreachable!()` 在生产路径
- 无 dead code（已标注 `#[allow(dead_code)]` 的未来扩展接口除外）
- 命名一致性（Rust snake_case、PHP camelCase，跨模块同概念同命名）
- 关键路径有文档注释（`///`）说明 why
- 资源管理：文件句柄 / 锁 / 事务 / 进程组 正确释放
- 并发安全：`Mutex` / `RwLock` / `AtomicU*` 使用正确，无死锁风险，无持锁 await
- 错误处理：`Result` 正确传播，无静默吞错（`let _ = err`）

#### 维度 2：功能完善度审计
对照 `implement-async-task-scheduler`、`audit-and-polish-round2` spec 与 `README.md` / `EVALUATION.md` 承诺的功能清单，逐项验证：
- 调度器：cron / interval / run_at / or_cron / skip_dates / workdays_only / timezone / jitter / misfire_grace_time / coalesce
- 重叠控制：allow_overlap / max_instances(N>1) / rate_limit / worker_concurrency
- 执行器：HTTP（method/headers/body/redirect/compression/binary body）/ Shell（cmd/stdin/cwd/env/timeout/soft_timeout）
- 持久化：SQLite WAL / busy_timeout / 加密 / 自动迁移 / 文件权限
- 生命周期：retry/backoff / expires / requeue / cancel / pause/resume / acks_late / acks_on_failure / ignore_result / max_executions / replace_existing
- 组合原语：chain / group / chord / chunks
- 事件流：record_event / list_events / cleanup
- 多租户：owner 隔离
- daemon 自愈：worker_max_tasks_per_child / worker_max_memory_per_child
- PHP 客户端：所有 README 列出的方法可调用且行为正确
- 测试覆盖：`cargo test --all-features` 全通过；PHP 测试脚本可运行

#### 维度 3：bug 率审计
逐模块审查逻辑正确性，重点：
- 状态机转换：Pending/Running/Success/Failed/Interrupted/Cancelled/Expired 转换路径无非法跳转
- 边界条件：空输入、超大输入、并发竞争、时区跨日、cron 边界
- 数据完整性：SQLite 事务原子性、加密/解密往返、序列化/反序列化兼容
- 资源泄漏：进程未回收、文件未关闭、锁未释放
- 并发竞态：OverlapController 计数、worker_limits 计数、queue in-flight 计数
- 数值溢出：u32/u64/i64 算术、时间戳转换
- 字符串处理：UTF-8 / 非 UTF-8（body_b64）/ SQL 注入 / 命令注入

#### 维度 4：错误率审计
逐模块审查错误处理，重点：
- `Result` 是否正确传播（无 `.unwrap()` / `?` 滥用）
- 错误信息是否可定位（含上下文：task_id / op / cause）
- 是否有静默吞错（`let _ = ` / `unwrap_or_default()` 掩盖真实错误）
- panic 风险：`unwrap()` / 数组下标 / `from_utf8` / `parse` / 整数转换
- 外部边界错误处理：HTTP 超时 / 连接拒绝 / DNS 失败 / SQLite BUSY / IPC 断开
- PHP 客户端：异常类型正确、不静默返回 null

### 产出

- `AUDIT_REPORT.md`：四维审计报告，含每维通过率计算、发现的问题清单、修复状态
- 必要时修复发现的 bug / 错误处理缺陷（目标：四维均 ≥97%）

## Impact

- Affected specs: `audit-and-polish-from-user-perspective`、`audit-and-polish-round2`、`audit-rust-extension-codebase`（已完成的审计，本次为最终质量门复核）
- Affected code: `src/`（全部 Rust 模块）、`releases/xhjob-thinkphp8-extend/`（全部 PHP 模块）、`AUDIT_REPORT.md`（新建/更新）

## ADDED Requirements

### Requirement: 四维质量门审计
系统 SHALL 对全部 Rust + PHP 代码进行四维审计（代码质量 / 功能完善度 / bug 率 / 错误率），每维通过率 ≥97%。功能完善度维度（用户特别强调"功能"）需对照下方完整功能清单逐项验证可用性，不允许仅靠文档承诺计数。

#### 功能完善度完整清单（维度 2 的检查项，共 N 项，通过率 = 可用项 / N）
**调度触发器（11 项）**：cron、interval(every)、run_at、or_cron、skip_dates、workdays_only、timezone per-job、jitter、misfire_grace_time per-job、coalesce per-job、reschedule_job
**重叠控制（4 项）**：allow_overlap、max_instances(N>1)、rate_limit、worker_concurrency
**HTTP 执行器（5 项）**：method/headers/body、redirect policy 可调、deflate/brotli/gzip 压缩、binary body(body_b64)、proxy
**Shell 执行器（4 项）**：cmd、stdin、working_dir、env；timeout、soft_timeout
**持久化（5 项）**：SQLite WAL、busy_timeout、payload 加密、自动 schema 迁移、文件权限 0o600
**生命周期（11 项）**：retry、retry_backoff、expires、requeue、cancel、pause/resume、acks_late、acks_on_failure、ignore_result、max_executions、replace_existing
**组合原语（4 项）**：chain、group、chord、chunks
**事件流（3 项）**：record_event、list_events、cleanup_expired_events
**多租户（1 项）**：owner 隔离
**daemon 自愈（2 项）**：worker_max_tasks_per_child、worker_max_memory_per_child
**查询能力（2 项）**：get_job、list_tasks(with tags filter)
**modify_job（1 项）**：任意字段在线修改
**PHP 客户端（3 项）**：README 所有方法可调用、EVALUATION 承诺已实现、composer.json 自动加载
**功能总数 ≈ 56 项**，要求可用 ≥55 项（97%）

#### Scenario: 编译与测试基线干净
- **WHEN** 执行 `cargo build --all-features` + `cargo build` + `cargo test --all-features`
- **THEN** 全部退出码 0，无 warning，全部测试通过

#### Scenario: clippy 无 warning
- **WHEN** 执行 `cargo clippy --all-features -- -D warnings`
- **THEN** 退出码 0（允许明确 `#[allow]` 标注的例外）

#### Scenario: 代码质量通过率 ≥97%
- **WHEN** 逐模块审查代码质量（unwrap/panic/dead code/命名/文档/资源/并发/错误处理）
- **THEN** 通过项数 / 已检查项数 ≥ 0.97

#### Scenario: 功能完善度通过率 ≥97%
- **WHEN** 对照上述功能清单（≈56 项）逐项验证实现可用性
- **THEN** 可用功能数 / 功能总数 ≥ 0.97（即 ≥55 项可用）

#### Scenario: bug 率通过率 ≥97%
- **WHEN** 逐模块审查逻辑正确性、边界、竞态、数据完整性
- **THEN** (已检查项 - 发现 bug) / 已检查项 ≥ 0.97

#### Scenario: 错误率通过率 ≥97%
- **WHEN** 逐模块审查错误处理（传播/信息/吞错/panic/外部边界）
- **THEN** (已检查项 - 错误处理缺陷) / 已检查项 ≥ 0.97

#### Scenario: 修复后达标
- **WHEN** 某维度通过率 < 97%
- **THEN** 修复发现的缺陷并重新审计，直至四维均 ≥97%

### Requirement: 审计报告
系统 SHALL 产出 `AUDIT_REPORT.md`，含四维通过率、问题清单、修复状态。

## MODIFIED Requirements

### Requirement: 审计报告文档
`AUDIT_REPORT.md` 更新为最终质量门审计结果，含四维通过率表格与结论。
