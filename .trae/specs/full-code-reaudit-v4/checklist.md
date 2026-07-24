# Checklist

## 阶段一：基线建立
- [ ] `cargo build --all-features` 通过
- [ ] `cargo test --all-features` 全部通过（记录基线通过数）
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` 零警告
- [ ] `cargo fmt --all -- --check` 干净
- [ ] `cargo build --release --all-features` 产出 libxhjob.so
- [ ] `php tp/repro/runner.php` 跑完全部 11 个脚本，记录基线 PASS/FAIL/CRASH/SKIP

## 阶段二：Rust 核心逐模块审查（独立于 v3）
- [ ] store/mod.rs — Task 字段完整性、状态机迁移合法、序列化兼容
- [ ] store/in_memory.rs — 无跨 await 持锁、reset_running_to_pending lease 正确、requeue/reschedule 边界正确
- [ ] store/sqlite.rs — schema 迁移完整、PRAGMA 正确（WAL/busy_timeout/foreign_keys/quick_check）、加密读写、worker_pid/starttime 持久化
- [ ] store/crypto.rs — 加密/解密正确、key 管理安全、错误处理
- [ ] scheduler/queue.rs — 派发流程正确、cancel_flag 传递、lease 写入时机正确、并发安全
- [ ] scheduler/cron.rs — cron 解析/next_fire/时区/or_cron/skip_dates/workdays_only/jitter 正确
- [ ] scheduler/watchdog.rs — 扫描周期合理、timeout*factor 判定正确、取消状态标记正确、与 lease 交互无冲突
- [ ] scheduler/overlap.rs + rate_limit.rs + max_instances — 去重与限流逻辑正确
- [ ] scheduler/chain.rs + group.rs + chord.rs — 编排状态机正确、回调触发正确、失败传播正确
- [ ] executor/shell.rs — execute_with_lease spawn 时机正确、starttime 捕获正确、cancel/timeout/SIGKILL 正确、编码处理正确
- [ ] executor/http.rs — 请求/代理/headers/body 正确、超时与取消正确、二进制 body 正确
- [ ] executor/mod.rs — 执行器选择正确、TaskResult 组装正确、错误路径正确
- [ ] daemon/mod.rs — process_starttime 读取正确、is_process_alive_with_starttime 正确、信号处理正确、drain 正确
- [ ] daemon/unix.rs + windows.rs — 平台 fork/setsid/信号差异处理正确
- [ ] daemon_main.rs — 启动/恢复/reset_running_to_pending 正确、PID 复用防护有效
- [ ] pool/ — 容量/背压/shutdown 正确
- [ ] ipc/ — 请求响应/超时/IO 死锁防护正确
- [ ] retry/mod.rs — 重试/退避正确、TaskResult 初始化正确、无整数溢出
- [ ] utils/ — 资源限制与度量正确
- [ ] lib.rs — PHP 导出函数参数校验正确、IPC 超时包裹到位

## 阶段三：tp 业务层审查
- [ ] app/controller/XhjobTask.php + app/middleware/XhjobAuth.php — API 端点/token 鉴权/参数校验正确
- [ ] config/xhjob.php — 配置项完整、默认值合理
- [ ] xhjob_server.php — daemon 起停/ini scan dir/健康检查正确
- [ ] xhjob_client_test.php + test_xhjob*.php — 客户端调用与测试脚本有效

## 阶段四：问题复现
- [ ] 每个发现的问题有对应 repro 脚本（PHP）或 cargo test（Rust）
- [ ] 复现脚本/test 在未修复状态下稳定复现（断言 FAIL 或 panic）
- [ ] 现有失效 repro 脚本已修复（阶段一标记项）
- [ ] 新增 repro 脚本纳入 runner.php 自动扫描

## 阶段五：源码修正
- [ ] 每个问题已根因修正（非压制症状）
- [ ] 每个修正后对应 repro/test 转为 PASS
- [ ] 修正未引入新 clippy 警告 / fmt 不一致
- [ ] 全量 repro 脚本与 cargo test 回归通过

## 阶段六：5 维度 100% 验证
- [ ] 正确率：`php tp/repro/runner.php` 0 FAIL/0 CRASH
- [ ] 正确率：`cargo test --all-features` 全通过
- [ ] 代码质量：`cargo clippy --all-targets --all-features -- -D warnings` 零警告
- [ ] 代码质量：`cargo fmt --all -- --check` 干净
- [ ] 功能完善度：cron/interval/runAt 端到端跑通
- [ ] 功能完善度：or_cron/skip_dates/workdays_only 端到端跑通
- [ ] 功能完善度：retry/timeout/cancel 端到端跑通
- [ ] 功能完善度：chain/group/chord 端到端跑通
- [ ] 功能完善度：rate_limit/overlap/max_instances 端到端跑通
- [ ] 功能完善度：persist+加密端到端跑通
- [ ] 功能完善度：watchdog 假死检测端到端跑通
- [ ] 功能完善度：execution_lease + crash recovery 端到端跑通
- [ ] 功能完善度：progress 端到端跑通
- [ ] 功能完善度：multi-tenant owner 端到端跑通
- [ ] 功能完善度：token 鉴权端到端跑通
- [ ] bug 率：已知 bug = 0（发现数 = 复现+修正数）
- [ ] 错误率：全量执行零 panic
- [ ] 错误率：全量执行零未捕获异常
- [ ] 错误率：全量执行零进程残留（daemon/zombie 清理干净）
