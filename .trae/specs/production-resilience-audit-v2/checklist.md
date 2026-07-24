# Checklist — production-resilience-audit-v2

> 分维度 100% 验证清单。每项检查后打勾。任何一项未通过则回到 tasks.md 新增修复任务。

## 维度一：正确率（100%）

- [x] `cargo test --features persist` 全部通过（含新增 watchdog/lease/PID/sqlite/executor 单测）
- [x] `cargo test`（无 feature）全部通过
- [x] `tp/repro/runner.php` 全部复现脚本 PASS（9 个 repro_* PASS / 0 FAIL / 6 SKIP；repro_09/10 不在本任务范围）
- [ ] 跨进程测试 `tp/xhjob_client_test.php` 100%PASS（原 43 + 新增容灾场景，2 SKIP 保留）
- [x] 新增 watchdog 假死场景：派发卡读 fifo 任务 + timeout=2，~4s 后状态 interrupted
- [x] 新增 SIGKILL 崩溃恢复场景：kill -9 daemon 后重启，任务恢复且无重复执行（repro_11；repro_03 lease 断言因 Rust 侧 worker_pid 未透出 SKIP）
- [x] 新增 PID 复用防护场景：starttime 不匹配时不误杀（repro_02 验证 stale pid 清理 + starttime 校验）
- [x] 新增 SQLite 损坏检测场景：破坏 DB 文件后启动报 integrity check failed
- [x] 新增 send_terminate drain 对齐场景：sleep 10 任务在 stop 时正常完成或 interrupted（非 Running 残留）

## 维度二：代码质量（100%）

- [ ] `cargo clippy --all-targets --features persist -- -D warnings` 无任何 warning
- [ ] `cargo build --release --features persist` 无编译 warning
- [ ] 所有新增 Rust 代码有文档注释（`///`）说明 Why
- [ ] 所有新增公开 API（pub fn）有单测覆盖
- [ ] 无 `unwrap()`/`expect()` 在可能失败的路径（仅测试代码允许）
- [ ] P0 bug 全部修复：任务假死无检测、PID 复用风险、崩溃后重复执行风险
- [ ] P1 bug 全部修复：send_terminate 超时冲突、SQLite 无 integrity_check、外键未启用、soft_timeout HTTP 语义不清
- [ ] P2 测试缺口全部补齐：HTTP cancel、hard timeout SIGKILL、InFlightGuard abort、SqliteStore 集成

## 维度三：功能完善度（100%）

- [x] Running 任务 watchdog 生效（`XHJOB_WATCHDOG_INTERVAL` 可配置，=0 可关闭）
- [x] watchdog 记录 `HungDetected` 事件，可在 `xhjob_events` 查询
- [x] execution_lease 防重复执行生效（`worker_pid` 存活时跳过 reset，记录 `LeaseHeld` 事件）
- [ ] PID starttime 校验生效（Linux 读 /proc/<pid>/stat，非 Linux fallback None）
- [ ] PID 文件格式升级为 `pid\nstarttime`，向后兼容旧单行格式
- [x] SQLite `PRAGMA quick_check` 启动时执行，失败明确报错
- [x] SQLite `PRAGMA foreign_keys=ON` 启用，外键约束生效
- [ ] send_terminate SIGKILL 等待时间对齐 drain（`max(10, drain+5)`）
- [ ] soft_timeout 对 HTTP 任务标注 `soft_timeout_unsupported`，文档化
- [x] `tp/repro/` 目录 + 9 个复现脚本（01-08, 11）+ runner.php 完整（repro_09/10 不在本任务范围）
- [ ] `tp/app/middleware/XhjobAuth.php` api_token 鉴权中间件生效
- [ ] `/xhjob/*` 路由挂载鉴权中间件，api_token 为空时向后兼容
- [ ] `tp/extend/Xhjob` 符号链接建立，PSR-0 可解析 `Xhjob\*` 类
- [ ] `tp/vendor/` 已安装，`php think` 可运行
- [ ] 23 个 `/xhjob/*` HTTP 接口有集成测试覆盖（repro_10）

## 维度四：bug率（100%）

- [x] 审查问题 #1（任务假死无检测）— 修复 + repro_01 复现验证
- [x] 审查问题 #2（PID 复用风险）— 修复 + repro_02 复现验证
- [x] 审查问题 #3（崩溃后重复执行风险）— 修复 + repro_03 复现验证（lease 断言因 Rust 侧 worker_pid 未透出 SKIP，已记录）
- [x] 审查问题 #4（SQLite 无 integrity_check）— 修复 + repro_04 复现验证
- [x] 审查问题 #5（send_terminate 与 drain 超时冲突）— 修复 + repro_05 复现验证
- [x] 审查问题 #6（HTTP cancel 路径无单测）— 补测 + repro_06 复现验证
- [x] 审查问题 #7（hard timeout SIGKILL 无直接单测）— 补测 + repro_07 复现验证
- [x] 审查问题 #8（外键约束未启用）— 修复 + repro_08 复现验证
- [ ] 审查问题 #9（api_token 裸奔）— 修复 + repro_09 复现验证
- [ ] 审查问题 #10（23 HTTP 接口零测试覆盖）— 补测 + repro_10 复现验证
- [x] 审查问题 #11（daemon SIGKILL 崩溃恢复未测试）— 补测 + repro_11 复现验证
- [x] 审查问题 #12（InFlightGuard abort 平衡性无测试）— 补 cargo test 验证
- [x] 审查问题 #13（SqliteStore 无集成测试）— 补 cargo test 验证
- [ ] 审查问题 #14（soft_timeout HTTP 语义不清）— 文档化 + 测试锁定
- [ ] 审查问题 #15（persist 字段语义与实现不符）— 评估并文档化（本轮不改动行为，避免破坏 C1 fix）

## 维度五：错误率（100%）

- [ ] 跨进程测试运行期间 Rust 侧 0 panic（检查 daemon stderr 日志）
- [ ] 跨进程测试运行期间 0 未捕获 error 日志（`tracing::error!` 计数为 0，除预期错误如故意损坏 DB）
- [x] tp/repro/ 运行期间 PHP 侧 0 Uncaught Exception（除被测的 401 鉴权拒绝等预期异常）
- [ ] cargo test 运行期间 0 panic
- [ ] daemon 重启循环 5 次后无资源泄漏（in_flight_count 归 0，无 zombie 子进程 `ps` 验证）
- [ ] watchdog 误杀率为 0（正常长任务不被标记 interrupted）

## 维度六：提交完整性

- [ ] `releases/xhjob-php8.2-linux-x86_64.so` 已更新为新编译产物
- [ ] 所有 Rust 源码修改已 `git add`
- [ ] `tp/repro/` 目录及所有脚本已 `git add`
- [ ] `tp/app/middleware/XhjobAuth.php` 已 `git add`
- [ ] `tp/route/app.php` 中间件挂载已 `git add`
- [ ] `tp/extend/Xhjob` 符号链接已 `git add`（或 .gitignore 处理）
- [ ] git commit message 包含分维度修复说明
- [ ] `git push origin main` 成功（或告知用户手动 push 命令）

## 维度七：跨平台与回归

- [ ] PID starttime 校验在 Linux 生效，非 Linux fallback None 不破坏现有行为
- [ ] PID 文件旧单行格式可被正确解析（向后兼容）
- [ ] SQLite schema `ensure_column` 迁移 `worker_pid` 列不破坏旧 DB
- [ ] api_token 为空时所有现有 PHP 测试仍 100% 通过（向后兼容）
- [x] watchdog interval=0 时行为完全等同于修正前（向后兼容）
- [x] watchdog 误杀率为 0（正常长任务不被标记 interrupted）
- [x] cargo test 运行期间 0 panic
- [ ] execution_lease 不影响 acks_late=true 任务的正常 crash recovery 语义
