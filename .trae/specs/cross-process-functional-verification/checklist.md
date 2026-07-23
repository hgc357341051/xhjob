# Checklist

## 环境准备
- [x] 已切换到 main 分支并 pull 到最新
- [x] 工作树干净，起始 commit 已记录

## 代码质量（更细致审查）
- [x] Rust 内核 `cargo build --release --features persist` 编译成功无 error
- [x] Rust 内核无显著 warning
- [x] Rust 27 个 `xhjob_*` PHP 导出函数逐个审查正确（修正：xhjob_modify 已补注册到 get_module；xhjob_countdown 实为 TaskBuilder 方法，非顶层函数）
- [x] Rust daemon/ipc/executor/pool/scheduler/store 并发与错误处理审查正确
- [x] PHP 集成包所有类含 `declare(strict_types=1)`
- [x] PHP 集成包公开方法含 PHPDoc（Facade 补齐 reportProgress/pullEvents/inspect 三个 @method）
- [x] tp 项目 Controller/配置/路由代码风格一致（XhjobTask 4 处 (string)$id 强转修复）
- [x] 集成包已安装到 `tp/extend/Xhjob/`，命名空间可解析（受 tp/extend/.gitignore 排除，规范源在 releases/）

## 功能完善度
- [x] Rust 单元测试 `cargo test --features persist` 100% 通过（139 passed; 0 failed）
- [x] PHP 集成包 TaskBuilder 链式方法可正常调用（client_test step 27 验证）
- [x] PHP 集成包 TaskManager 全部方法可正常调用（client_test CRUD/控制/编排覆盖）
- [x] XhjobService 生命周期方法可正常调用（client_test step 1-4 验证）
- [x] tp/xhjob_server.php 可启动 daemon 并打印 READY
- [x] tp/xhjob_client_test.php 可连接已运行 daemon

## bug 率
- [x] Rust 内核审查发现的 bug 全部修复（xhjob_modify 未注册）
- [x] PHP 集成包审查发现的 bug 全部修复（Facade @method 缺失）
- [x] tp 项目审查发现的 bug 全部修复（strict_types 下 null id TypeError）
- [x] 无残留 P0/P1 级别 bug

## 错误率
- [x] Rust 扩展运行时无 panic
- [x] PHP 端调用 xhjob 函数无未捕获异常
- [x] IPC 请求无永久阻塞（ipc_request 默认 5s 超时兜底）
- [x] daemon 启动/停止/重启流程无错误
- [x] 跨进程测试运行期间无 daemon crash / 无 PHP fatal error

## 跨进程功能正确性（27 个 xhjob_* 顶层函数）
- [x] xhjob_start / xhjob_stop / xhjob_restart / xhjob_status 正确（client_test step 2-4）
- [x] xhjob_dispatch / xhjob_state / xhjob_result / xhjob_get / xhjob_list / xhjob_remove 正确（step 5-10）
- [x] xhjob_pause / xhjob_resume / xhjob_cancel / xhjob_requeue / xhjob_reschedule / xhjob_modify 正确（step 11-15）
- [x] xhjob_chain / xhjob_chain_state / xhjob_group / xhjob_group_state / xhjob_chord / xhjob_chord_state 正确（step 16-22，countdown 经 TaskBuilder::countdown 验证）
- [x] xhjob_events / xhjob_report_progress / xhjob_pull_events / xhjob_inspect 正确（step 23-26）
- [x] TaskBuilder 链式 API 全部方法正确（step 27）

## 复杂生产场景覆盖
- [x] 场景 1：daemon 跨进程存活（server 退出后 client 仍可连接）PASS
- [x] 场景 2：并发派发 PASS
- [x] 场景 3：长任务硬超时 + 软超时 PASS
- [x] 场景 4：真实失败重试 + 指数退避 PASS
- [x] 场景 5：突发限流（rateLimit 滑动窗口）PASS
- [x] 场景 6：maxInstances 重叠控制 PASS
- [x] 场景 7：持久化 + daemon 崩溃恢复 PASS
- [x] 场景 8：运行中任务取消 PASS
- [x] 场景 9：cron pause/resume PASS（与 SubTask 7.4 合并，SKIP 重复）
- [x] 场景 10：大 payload 派发 PASS
- [x] 场景 11：HTTP 各状态码 + idempotent 重试 PASS
- [x] 场景 12：Shell 非零退出 + 重试 PASS（与场景 4 合并，SKIP 重复）
- [x] 场景 13：多服务实例隔离 PASS
- [x] 场景 14：事件流顺序与时序 PASS
- [x] 场景 15：inspect 统计准确性 PASS
- [x] 场景 16：空/非法输入错误处理 PASS
- [x] 场景 17：编码转换（GBK shell 输出）PASS

## 端到端跨进程验证
- [x] `tp/xhjob_server.php` 启动 daemon 后退出，daemon 仍存活（PPID=1）
- [x] `tp/xhjob_client_test.php` 连接 daemon 全部测试 100% PASS（43 PASS / 0 FAIL / 2 SKIP）
- [x] `cargo test --features persist` 100% 通过（139 passed; 0 failed; 0 ignored）

## 四维度 100% 通过率最终确认
- [x] 代码质量通过率 100%
- [x] 功能完善度通过率 100%
- [x] bug 率通过率 100%（已修复：xhjob_modify 注册、Facade @method、Controller strict_types 强转）
- [x] 错误率通过率 100%

## 编译并提交到远程主分支
- [x] `cargo build --release --features persist` 产出 `target/release/libxhjob.so`（11640256 字节）
- [x] `git add` 包含所有修改代码 + `.so` + 新增文件（11 files changed, 1458 insertions）
- [x] `git commit` 创建提交（commit hash: 5e79fdc）
- [ ] `git push origin main` 推送到远程主分支成功（阻塞：HTTPS 需 GitHub 凭据，当前环境无 credential helper / token / SSH key）
- [ ] 记录最终 commit hash
