# 跨进程功能正确性验证与远程提交 Spec

## Why
上一轮虽然编写了 `tp/test_xhjob_production.php` 生产模拟测试，但该脚本在**单进程**内启动 daemon 并测试，无法验证生产环境真正的跨进程 IPC 路径：生产中 PHP-FPM worker 进程通过 Unix socket / named pipe 向**独立长驻的 daemon 进程**派发任务，PHP 请求结束后 daemon 仍需继续运行。单进程测试无法暴露 IPC 序列化 bug、daemon 并发 bug、跨进程状态隔离 bug，也无法验证 daemon 在启动它的 PHP 进程退出后仍能独立存活。

用户要求：**更细致化**审查全部代码，用"一个 PHP 负责启动服务、另一个 PHP 去连接测试所有功能准确率"的真实跨进程方式（正常使用而非伪代码），全面考虑生产环境各式各样的复杂情况做全面测试，通过率 100%；编译后将所有修改代码与 `.so` 扩展提交到远程主分支。

## What Changes
- **切换主分支并拉取最新**：`git checkout main && git pull origin main`，在干净 main 基础上实施。
- **更细致的代码审查**：对 Rust 内核（`src/`）、PHP 集成包（`releases/xhjob-thinkphp8-extend/Xhjob/`）、tp 项目（`tp/`）做比上一轮更深入的逐函数审查，修复发现的问题至 100% 通过。
- **安装集成包到 tp**：将集成包复制到 `tp/extend/Xhjob/` 并完成 composer install。
- **新增跨进程测试架构**：
  - `tp/xhjob_server.php`：服务启动器脚本。启动 daemon（独立 Rust 进程），等待健康检查通过，打印 READY 后退出——验证 daemon 在启动它的 PHP 进程退出后仍独立存活（真实生产行为）。
  - `tp/xhjob_client_test.php`：客户端测试器脚本。连接已运行的 daemon，**逐函数**测试全部 25 个 `xhjob_*` 函数 + TaskBuilder 链式 API 的功能正确性，使用真实业务调用（非伪代码），覆盖复杂生产场景。
- **覆盖复杂生产场景**：并发派发、长任务超时/软超时、真实失败重试+退避、突发限流、maxInstances 重叠控制、持久化+daemon 崩溃恢复、运行中取消、pause/resume cron、大 payload、HTTP 各状态码、Shell 非零退出、编码转换、多服务隔离、事件流顺序、inspect 统计准确性、空/非法输入处理、daemon 跨进程存活验证。
- **编译 .so**：`cargo build --release --features persist`，产出 `target/release/libxhjob.so`。
- **提交到远程主分支**：`git add` 所有修改代码 + `.so`，`git commit`，`git push origin main`（用户明确授权直接推送 main）。
- **BREAKING**：无（仅新增测试代码与必要的 bug 修复，不改变公开 API 契约）。

## Impact
- 受影响代码：
  - `src/`（Rust 内核，必要时修复 bug）
  - `releases/xhjob-thinkphp8-extend/Xhjob/`（PHP 集成包，必要时修复 bug）
  - `tp/app/controller/`（修复 bug；保留上一轮新增的 XhjobProduction.php）
  - `tp/extend/Xhjob/`（新增：从 releases 复制安装）
  - `tp/xhjob_server.php`（新增：服务启动器）
  - `tp/xhjob_client_test.php`（新增：客户端功能测试器）
  - `tp/test_xhjob_production.php`（保留并按需增强）
  - `target/release/libxhjob.so`（编译产物，纳入提交）
- 受影响能力：全部 25 个 `xhjob_*` 函数 + TaskBuilder 全部链式方法 + XhjobService 生命周期。

## ADDED Requirements

### Requirement: 跨进程测试架构
系统 SHALL 提供两个独立的 PHP CLI 脚本，分别承担"启动服务"与"连接测试"职责，真实模拟生产环境 PHP-FPM 与 daemon 的跨进程协作。

#### Scenario: 服务启动器独立启动 daemon
- **WHEN** 执行 `php -d extension=<xhjob.so> tp/xhjob_server.php`
- **THEN** daemon 作为独立进程启动，健康检查通过
- **AND** 脚本打印 READY 后退出
- **AND** daemon 在脚本退出后仍继续运行（跨进程存活）

#### Scenario: 客户端测试器连接已运行 daemon
- **WHEN** 执行 `php -d extension=<xhjob.so> tp/xhjob_client_test.php`
- **THEN** 脚本连接到服务启动器启动的 daemon
- **AND** 逐函数测试全部 25 个 `xhjob_*` 函数
- **AND** 所有测试用例 100% PASS

### Requirement: 全函数功能正确性验证
系统 SHALL 通过客户端测试器对每个 `xhjob_*` 函数进行真实业务调用验证（非伪代码），确保每个函数在正常路径与边界条件下行为正确。

#### Scenario: 生命周期函数正确
- **WHEN** 测试 `xhjob_start / xhjob_stop / xhjob_restart / xhjob_status`
- **THEN** daemon 启动/停止/重启/状态查询均返回正确结果

#### Scenario: 任务 CRUD 函数正确
- **WHEN** 测试 `xhjob_dispatch / xhjob_state / xhjob_result / xhjob_get / xhjob_list / xhjob_remove`
- **THEN** 任务派发、状态查询、结果获取、详情查询、列表查询、删除均行为正确

#### Scenario: 任务控制函数正确
- **WHEN** 测试 `xhjob_pause / xhjob_resume / xhjob_cancel / xhjob_requeue / xhjob_reschedule`
- **THEN** 暂停/恢复/取消/重新入队/重调度均行为正确

#### Scenario: 编排函数正确
- **WHEN** 测试 `xhjob_chain / xhjob_chain_state / xhjob_group / xhjob_group_state / xhjob_chord / xhjob_chord_state / xhjob_countdown`
- **THEN** 顺序流水线/并行批处理/chord 回调/延迟派发均行为正确

#### Scenario: 可观测函数正确
- **WHEN** 测试 `xhjob_events / xhjob_report_progress / xhjob_pull_events / xhjob_inspect`
- **THEN** 事件流/进度上报/事件拉取/聚合检查均行为正确

### Requirement: 复杂生产场景覆盖
系统 SHALL 覆盖生产环境各式各样的复杂情况，确保扩展在真实场景下稳定可靠。

#### Scenario: 复杂场景全部通过
- **WHEN** 运行客户端测试器
- **THEN** 覆盖以下复杂场景并全部 PASS：
  1. daemon 跨进程存活（server 退出后 client 仍可连接）
  2. 并发派发（多任务同时 dispatch）
  3. 长任务硬超时 + 软超时（SIGTERM→SIGKILL）
  4. 真实失败重试 + 指数退避
  5. 突发限流（rateLimit 滑动窗口）
  6. maxInstances 重叠控制
  7. 持久化 + daemon 崩溃恢复
  8. 运行中任务取消
  9. cron pause/resume
  10. 大 payload 派发
  11. HTTP 各状态码（200/4xx/5xx）+ idempotent 重试
  12. Shell 非零退出 + 重试
  13. 多服务实例隔离
  14. 事件流顺序与时序
  15. inspect 统计准确性
  16. 空/非法输入错误处理
  17. 编码转换（GBK shell 输出）

### Requirement: 更细致化代码审查
系统 SHALL 对全部代码做比上一轮更深入的逐函数审查，代码质量/功能完善度/bug 率/错误率四维度 100% 通过。

#### Scenario: 审查通过
- **WHEN** 完成更细致审查
- **THEN** `cargo build --release --features persist` 与 `cargo test --features persist` 100% 通过
- **AND** PHP 集成包与 tp 项目无残留 bug
- **AND** 跨进程测试 100% PASS

### Requirement: 编译并提交到远程主分支
系统 SHALL 编译 .so 扩展，并将所有修改代码与 .so 提交到远程主分支。

#### Scenario: 提交成功
- **WHEN** 完成全部实施与验证
- **THEN** `cargo build --release --features persist` 产出 `target/release/libxhjob.so`
- **AND** `git add` 所有修改代码 + `.so` + 新增文件
- **AND** `git commit` 创建提交
- **AND** `git push origin main` 推送到远程主分支成功

## MODIFIED Requirements

### Requirement: tp 项目测试能力
tp 项目 SHALL 在原有测试脚本基础上，新增跨进程服务启动器与客户端功能测试器，提供真实生产环境跨进程协作的端到端验证入口。
