# Tasks

- [ ] Task 1: 探索代码库结构并建立审查基线
  - [ ] SubTask 1.1: 读取 Cargo.toml 了解依赖与 features
  - [ ] SubTask 1.2: 读取 src/lib.rs 了解 PHP 扩展导出函数清单与模块组织
  - [ ] SubTask 1.3: 统计各模块代码行数、依赖关系、外部 crate 使用情况

- [ ] Task 2: 审查 daemon 模块（进程管理 + 跨平台）
  - [ ] SubTask 2.1: 审查 src/daemon/mod.rs（daemon 生命周期、状态机）
  - [ ] SubTask 2.2: 审查 src/daemon/unix.rs（double-fork、信号处理、setsid）
  - [ ] SubTask 2.3: 审查 src/daemon/windows.rs（CreateProcessW、Named Pipe）
  - [ ] SubTask 2.4: 审查 src/daemon_main.rs（主循环、op 分发、scan_once）

- [ ] Task 3: 审查 pool 模块（两种执行池）
  - [ ] SubTask 3.1: 审查 src/pool/coroutine_pool.rs（tokio runtime、Semaphore）
  - [ ] SubTask 3.2: 审查 src/pool/thread_pool.rs（std::thread、crossbeam-channel）
  - [ ] SubTask 3.3: 审查 src/scheduler/queue.rs 中两种池的切换逻辑与任务派发

- [ ] Task 4: 审查 scheduler 模块（调度与编排）
  - [ ] SubTask 4.1: 审查 src/scheduler/cron.rs（cron 表达式解析、next_fire）
  - [ ] SubTask 4.2: 审查 src/scheduler/overlap.rs（max_instances、coalesce、misfire）
  - [ ] SubTask 4.3: 审查 src/scheduler/rate_limit.rs（滑动窗口限流）
  - [ ] SubTask 4.4: 审查 src/scheduler/chain.rs / chord.rs / group.rs（编排原语）
  - [ ] SubTask 4.5: 审查 src/scheduler/events.rs（事件流）

- [ ] Task 5: 审查 executor 模块（HTTP / Shell 执行器）
  - [ ] SubTask 5.1: 审查 src/executor/http.rs（HTTP 客户端、代理、超时、重试）
  - [ ] SubTask 5.2: 审查 src/executor/shell.rs（子进程、信号、编码转换、软超时）

- [ ] Task 6: 审查 store 与 ipc 模块（持久化与通信）
  - [ ] SubTask 6.1: 审查 src/store/mod.rs（TaskStore trait、Task 结构、TaskState）
  - [ ] SubTask 6.2: 审查 src/store/in_memory.rs 与 src/store/sqlite.rs（存储后端）
  - [ ] SubTask 6.3: 审查 src/ipc/mod.rs + unix_socket.rs + named_pipe.rs（IPC 协议）

- [ ] Task 7: 审查 task / utils / service / outcome / retry 模块
  - [ ] SubTask 7.1: 审查 src/task/mod.rs（TaskBuilder、序列化、字段默认值）
  - [ ] SubTask 7.2: 审查 src/utils/memory.rs + limits.rs（RSS 读取、限制）
  - [ ] SubTask 7.3: 审查 src/retry/mod.rs（重试策略、指数退避）
  - [ ] SubTask 7.4: 审查 src/service/mod.rs 与 src/outcome/mod.rs

- [ ] Task 8: 审查 lib.rs（PHP 扩展入口与导出函数）
  - [ ] SubTask 8.1: 审查 src/lib.rs 全部导出函数的签名、参数校验、错误返回
  - [ ] SubTask 8.2: 审查 JSON 契约、error: 字符串约定、向后兼容性

- [ ] Task 9: 交叉维度审查（7 大维度横向扫描）
  - [ ] SubTask 9.1: 错误处理维度（unwrap/expect/panic、Result 传播链）
  - [ ] SubTask 9.2: 资源管理维度（文件/socket/子进程/SQLite 释放）
  - [ ] SubTask 9.3: 安全性维度（命令注入、路径遍历、反序列化、credential）
  - [ ] SubTask 9.4: 并发安全维度（锁竞争、死锁、Send/Sync、block_on 嵌套）

- [ ] Task 10: 产出审查报告 AUDIT_REPORT.md
  - [ ] SubTask 10.1: 撰写优点清单（值得保留的好功能与设计）
  - [ ] SubTask 10.2: 撰写缺点清单（按 P0/P1/P2 优先级排序）
  - [ ] SubTask 10.3: 撰写待完善功能清单（TODO/stub/增强建议）
  - [ ] SubTask 10.4: 撰写按模块的审查结论（含文件:行号引用）

# Task Dependencies

- Task 1 是所有后续任务的前置（建立基线）
- Task 2-8 可并行执行（不同模块独立审查）
- Task 9 依赖 Task 2-8（需要先读取各模块代码才能做横向扫描）
- Task 10 依赖 Task 2-9（汇总所有发现）
