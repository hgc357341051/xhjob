# Tasks

## 阶段一：基线建立（当前状态快照）
- [ ] Task 1: 建立代码质量基线
  - [ ] SubTask 1.1: `cargo build --all-features` 记录是否通过
  - [ ] SubTask 1.2: `cargo test --all-features` 记录通过/失败数
  - [ ] SubTask 1.3: `cargo clippy --all-targets --all-features -- -D warnings` 记录警告数
  - [ ] SubTask 1.4: `cargo fmt --all -- --check` 记录是否干净
- [ ] Task 2: 跑现有 repro 基线
  - [ ] SubTask 2.1: 编译 release 扩展 `cargo build --release --all-features`
  - [ ] SubTask 2.2: `php tp/repro/runner.php` 跑全部 11 个脚本，记录 PASS/FAIL/CRASH/SKIP 分布
  - [ ] SubTask 2.3: 标记失效/断言不严谨的 repro 脚本清单

## 阶段二：Rust 核心逐模块深度审查（独立于 v3 结论）
- [ ] Task 3: 审查 `src/store/`（mod / in_memory / sqlite / crypto）
  - [ ] SubTask 3.1: store/mod.rs — Task 结构体字段完整性、状态机迁移合法性、序列化兼容性
  - [ ] SubTask 3.2: in_memory.rs — 锁顺序（跨 await 持锁）、reset_running_to_pending lease 逻辑、requeue/reschedule 边界
  - [ ] SubTask 3.3: sqlite.rs — schema 迁移、PRAGMA（WAL/busy_timeout/foreign_keys/quick_check）、加密读写、worker_pid/starttime 持久化
  - [ ] SubTask 3.4: crypto.rs — 加密/解密、key 管理、错误处理
- [ ] Task 4: 审查 `src/scheduler/`（mod / queue / cron / events / overlap / rate_limit / chain / group / chord / watchdog）
  - [ ] SubTask 4.1: queue.rs — 派发流程、cancel_flag 传递、execution_lease 写入时机、并发安全
  - [ ] SubTask 4.2: cron.rs — cron 解析、next_fire、时区、or_cron/skip_dates/workdays_only、jitter
  - [ ] SubTask 4.3: watchdog.rs — 假死扫描周期、timeout*factor 判定、取消与状态标记、与 lease 的交互
  - [ ] SubTask 4.4: overlap.rs / rate_limit.rs / max_instances — 去重与限流逻辑
  - [ ] SubTask 4.5: chain.rs / group.rs / chord.rs — 编排状态机、回调触发、失败传播
- [ ] Task 5: 审查 `src/executor/`（mod / http / shell）
  - [ ] SubTask 5.1: shell.rs — execute_with_lease spawn 时机、starttime 捕获、cancel/timeout/SIGKILL、编码处理
  - [ ] SubTask 5.2: http.rs — 请求/代理/headers/body、超时与取消、二进制 body
  - [ ] SubTask 5.3: mod.rs — 执行器选择、TaskResult 组装、错误路径
- [ ] Task 6: 审查 `src/daemon/`（mod / unix / windows / daemon_main）
  - [ ] SubTask 6.1: mod.rs — process_starttime 读取 /proc/<pid>/stat、is_process_alive_with_starttime、信号处理、drain
  - [ ] SubTask 6.2: unix.rs / windows.rs — 平台 fork/setsid/信号差异
  - [ ] SubTask 6.3: daemon_main.rs — 启动/恢复/崩溃恢复 reset_running_to_pending、PID 复用防护
- [ ] Task 7: 审查 `src/pool/`、`src/ipc/`、`src/retry/`、`src/utils/`、`src/config.rs`、`src/errors.rs`、`src/service/`、`src/task/`、`src/outcome/`、`lib.rs`
  - [ ] SubTask 7.1: pool/ — 协程池/线程池容量、背压、shutdown
  - [ ] SubTask 7.2: ipc/ — unix_socket/named_pipe 请求响应、超时、IO 死锁防护
  - [ ] SubTask 7.3: retry/mod.rs — 重试/退避、TaskResult 初始化、整数溢出
  - [ ] SubTask 7.4: utils/ — limits/memory/metrics 资源限制与度量
  - [ ] SubTask 7.5: lib.rs — PHP 导出函数（xhjob_start/stop/submit/state/get/list 等）参数校验、IPC 超时包裹

## 阶段三：tp 业务层审查
- [ ] Task 8: 审查 `tp/` PHP 业务层
  - [ ] SubTask 8.1: app/controller/XhjobTask.php + app/middleware/XhjobAuth.php — API 端点、token 鉴权、参数校验
  - [ ] SubTask 8.2: config/xhjob.php — 配置项完整性、默认值合理性
  - [ ] SubTask 8.3: xhjob_server.php — daemon 起停、ini scan dir、健康检查
  - [ ] SubTask 8.4: xhjob_client_test.php + test_xhjob*.php — 客户端调用与测试脚本有效性

## 阶段四：问题复现（PHP repro + Rust test）
- [ ] Task 9: 汇总阶段二/三发现的问题清单，为每个问题分配复现方式
  - [ ] SubTask 9.1: 端到端问题 → tp/repro/ 新增/修复脚本（编号顺延）
  - [ ] SubTask 9.2: Rust 内部问题 → cargo test（`repro_` 前缀）复现
- [ ] Task 10: 逐个编写复现脚本/test，在未修复状态下稳定复现（断言 FAIL 或 panic）
- [ ] Task 11: 修复现有失效 repro 脚本（阶段一 SubTask 2.3 标记项）

## 阶段五：源码修正
- [ ] Task 12: 逐个修正问题（根因修复，非压制症状）
  - [ ] SubTask 12.1: 每个修正对应一个 repro/test 转为 PASS
  - [ ] SubTask 12.2: 修正过程不引入新 clippy 警告 / fmt 不一致
- [ ] Task 13: 修正后回归所有 repro 脚本与 cargo test

## 阶段六：5 维度 100% 验证
- [ ] Task 14: 正确率验证 — `php tp/repro/runner.php` 0 FAIL/0 CRASH + `cargo test --all-features` 全通过
- [ ] Task 15: 代码质量验证 — clippy 零警告 + fmt 干净
- [ ] Task 16: 功能完善度验证 — 逐功能端到端跑通（cron/interval/runAt/or_cron/skip_dates/workdays_only/retry/timeout/cancel/chain/group/chord/rate_limit/overlap/max_instances/persist+加密/watchdog/lease/crash recovery/progress/owner/token鉴权）
- [ ] Task 17: bug 率验证 — 已知 bug=0（发现数=复现+修正数）
- [ ] Task 18: 错误率验证 — 全量执行零 panic/零未捕获异常/零进程残留

# Task Dependencies
- Task 2 依赖 Task 1（需 release 扩展编译产物）
- Task 9 依赖 Task 3-8（审查发现问题）
- Task 10-11 依赖 Task 9
- Task 12 依赖 Task 10-11（先复现再修正）
- Task 13 依赖 Task 12
- Task 14-18 依赖 Task 13
- Task 3-8 可并行（不同模块/层）
