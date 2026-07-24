# 全代码库重新审查与生产级修复 Spec（full-code-reaudit-v4）

## Why
前期已对代码做过一轮修改（watchdog / execution_lease / PID starttime / SQLite 完整性检查等），但用户要求**不参考原 spec**，对当前代码状态做一次独立的、从零开始的全面审查。目标是验证 Rust 核心扩展与 tp 业务层在**真实生产业务逻辑**下达到 5 个维度各 100% 通过率：正确率、代码质量、功能完善度、bug 率、错误率。所有发现的问题必须用代码复现后再修正，杜绝伪代码验收。

## What Changes
- 以**当前代码状态**为审查基线（已含 v3 修复），独立重新审查 `src/` Rust 核心（37 个源文件）与 `tp/` PHP 业务层，不沿用 v3 spec 的结论。
- 按 5 个维度建立可验证的硬标准，逐模块审查并记录问题清单。
- 所有发现的问题在 `tp/repro/` 用 PHP+Rust 混合方式复现（端到端走 PHP，Rust 内部逻辑走 cargo test），复现稳定后修正源码，复现脚本转为回归测试。
- 修复后重新跑全量复现 + cargo test + clippy + fmt，确保 5 维度 100% 通过。
- 复用现有 `tp/repro/_bootstrap.php` / `runner.php` 基础设施；对失效或断言不严谨的现有 repro 脚本进行修复，并按新发现问题新增脚本。

## Impact
- Affected code: `src/`（config / daemon / errors / executor / ipc / outcome / pool / retry / scheduler / service / store / task / utils / lib.rs）；`tp/`（app/controller、app/middleware、config/xhjob.php、xhjob_server.php、xhjob_client_test.php、repro/）。
- Affected tests: `tp/repro/repro_*.php`、`src` 内 `#[cfg(test)]` 模块、cargo test 套件。
- 不修改 `releases/xhjob-thinkphp8-extend/` 发布包（本次范围外）。

## ADDED Requirements

### Requirement: 5 维度 100% 通过的硬标准
系统 SHALL 在审查结束时满足以下可验证判定，每维度独立计 100% 通过率：

- **正确率（正常使用而非伪代码）**：`tp/repro/runner.php` 全量执行所有 repro 脚本 0 FAIL / 0 CRASH；`cargo test --all-features` 全部通过；每个 repro 脚本的断言基于真实 daemon 行为（真实起停、真实任务派发、真实状态读取），禁止 mock 伪代码。
- **代码质量**：`cargo clippy --all-targets --all-features -- -D warnings` 零警告；`cargo fmt --all -- --check` 干净；PHP 业务层无语法/逻辑坏味道（通过审查清单确认）。
- **功能完善度**：spec 中已定义的所有功能（cron/interval/runAt/or_cron/skip_dates/workdays_only、retry/timeout/cancel、chain/group/chord、rate_limit/overlap/max_instances、persist+加密、watchdog、execution_lease、crash recovery、progress、multi-tenant owner、token 鉴权）在生产模拟中可端到端跑通并产出正确状态。
- **bug 率**：审查发现的 bug 数 = 已用代码复现并修正的 bug 数（即残留已知 bug = 0）。每个 bug 有对应 repro 脚本或 cargo test 复现。
- **错误率**：全量 repro + cargo test 执行期间零 panic、零未捕获异常、零非预期进程残留（daemon/zombie 清理干净）。

### Requirement: 生产环境业务逻辑模拟测试
系统 SHALL 在 `tp/repro/` 中通过真实 daemon 子进程模拟生产业务逻辑进行测试：

- 每个 repro 脚本通过 `_bootstrap.php` 的 `start_daemon()` 起真实 daemon，调用 `xhjob_*` 扩展 API 提交/查询任务，断言基于 `xhjob_state` / `xhjob_get` / `xhjob_list` / 进程 `/proc` 读取等真实信号。
- 涉及崩溃恢复的场景通过 `posix_kill` 发送 SIGKILL 模拟 daemon 崩溃，重启后验证不重复执行、Running 任务被正确回收。
- 涉及任务假死的场景通过派发卡死子进程（如 `sleep` 超过 timeout）验证 watchdog 取消与状态标记。
- 测试结束 `stop_daemon()` + `cleanup_data_dir()` 确保无残留。

### Requirement: 问题复现-修正闭环
对审查中发现的每个问题，系统 SHALL 执行以下闭环：

1. **复现**：编写 repro 脚本（PHP 端到端）或 cargo test（Rust 内部），在**未修复**状态下能稳定复现问题（断言 FAIL 或 panic）。
2. **修正**：修改源码修复根因，而非压制症状。
3. **回归**：修复后同一脚本/test 断言转 PASS；脚本纳入 `runner.php` 持续回归。
4. **记录**：在 `tasks.md` 标注复现脚本路径与修正 commit 位置。

### Requirement: Rust 内部问题复现方式
对 PHP 层无法直接触发的问题（整数溢出、状态机非法迁移、并发锁竞态、内部 panic 路径），系统 SHALL 通过 cargo test（`#[cfg(test)]` 模块或 `tests/` 集成测试）复现并修正：

- 测试需能在 `cargo test --all-features` 下稳定复现（必要时加 `#[ignore]` 并在 CI 显式触发）。
- 复现测试命名以 `repro_` 前缀标识，与 PHP repro 脚本一一对应或独立成项。

## MODIFIED Requirements

### Requirement: 复现脚本基础设施复用
现有 `tp/repro/_bootstrap.php`（start/stop_daemon、repro_step/repro_summary）与 `runner.php`（批量执行+汇总）SHALL 被复用：

- 逐个验证现有 11 个 repro 脚本（01-11）的有效性：能稳定 PASS 的保留；断言不严谨或已失效的修复；对应问题已变化的更新。
- 新发现问题按编号顺延新增 repro 脚本，纳入 `runner.php` 自动扫描。
- `runner.php` 退出码：全 PASS=0，有 FAIL/CRASH=1。

## REMOVED Requirements

### Requirement: 依赖 v3 spec 结论
**Reason**: 用户明确要求不参考原 spec，本次审查独立进行，以当前代码状态为基线重新评估。
**Migration**: v3 中已落地的修复（watchdog/lease/starttime/SQLite 完整性）视为代码现状的一部分，由本次审查重新验证其有效性，而非直接采信。
