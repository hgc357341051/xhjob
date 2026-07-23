# 运行所有测试代码功能并提交主分支 Spec

## Why
用户要求审查所有代码，运行**测试代码里的所有功能**（真实使用而非伪代码），通过率必须 100%；测试通过后编译 .so，将所有修改代码与 .so 提交到远程 main 分支。

当前状态：工作在 `trae/agent-lIaMM4` 分支（领先 main 一个 commit `906b1db`，含上轮代码审查修复 + 生产模拟测试）。环境已重置：`target/`、`tp/vendor/`、`tp/extend/Xhjob/` 均不存在，需重新准备。tp/ 下有 10 个测试脚本（test_xhjob.php / test_xhjob_async_queue.php / test_xhjob_deep.php / test_xhjob_fixes.php / test_xhjob_multi_service.php / test_xhjob_p0_round2.php / test_xhjob_persist.php / test_xhjob_pool_diff.php / test_xhjob_pool_mode.php / test_xhjob_production.php），需逐一真实运行并确保 100% 通过。

## What Changes
- **重建构建环境**：`cargo build --release --features persist` 重新编译 .so；`composer install` 恢复 tp 依赖；复制集成包到 `tp/extend/Xhjob/`。
- **运行所有测试代码**：逐一真实运行 tp/ 下全部 10 个测试脚本（每个脚本以真实 daemon + 真实任务执行，非 mock/伪代码），记录每个脚本的 PASS/FAIL/SKIP，修复所有失败用例至 100% 通过。
- **编译 .so 并纳入版本管理**：重新编译 .so，更新 `releases/xhjob-php8.2-linux-x86_64.so`（releases/ 下 .so 已被 .gitignore 例外允许跟踪）。
- **提交到远程 main 分支**：将 `trae/agent-lIaMM4` 的全部修改 + 测试修复 + 更新后的 .so 合并/快进到 main，commit 并 push 到 `origin/main`。
- **BREAKING**：无。

## Impact
- 受影响代码：
  - `tp/test_xhjob_*.php`（如运行测试发现失败则修复测试代码或底层 bug）
  - `releases/xhjob-php8.2-linux-x86_64.so`（更新：重新编译产物）
  - `src/`、`releases/xhjob-thinkphp8-extend/Xhjob/`、`tp/`（如测试发现新 bug 则修复）
  - 远程 `origin/main`（push 目标）

## ADDED Requirements

### Requirement: 运行所有测试代码功能
系统 SHALL 真实运行 tp/ 下全部 10 个测试脚本，每个脚本均以真实 daemon + 真实任务执行（非 mock/stub/伪代码），通过率 100%。

#### Scenario: 全部测试脚本通过
- **WHEN** 逐一执行 tp/ 下 10 个测试脚本（`php -d extension=<xhjob.so> tp/test_xhjob_*.php`）
- **THEN** 每个脚本的 PASS+SKIP 占比达 100%（无 FAIL），或修复失败用例至 100% 通过

#### Scenario: 真实使用非伪代码
- **WHEN** 测试运行
- **THEN** 每个测试均通过真实 daemon IPC 调用 + 真实 shell/http 任务执行验证返回值，非 mock/stub

### Requirement: 编译并提交到远程 main
系统 SHALL 重新编译 .so，将所有修改代码与 .so 提交并推送到远程 main 分支。

#### Scenario: 编译产物就绪
- **WHEN** 执行 `cargo build --release --features persist`
- **THEN** 生成 `target/release/libxhjob.so`，并更新 `releases/xhjob-php8.2-linux-x86_64.so`

#### Scenario: 推送到远程 main
- **WHEN** 将修改合并到 main 并 push
- **THEN** `origin/main` 包含全部代码修改、测试修复、更新后的 .so
- **AND** `git log origin/main` 含本次提交
