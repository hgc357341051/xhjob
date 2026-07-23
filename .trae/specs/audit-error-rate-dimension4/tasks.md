# Tasks

- [x] Task 1: Rust Result 传播 + 错误信息可定位审计（项 1-12）
  - [x] 1.1 验证 errors.rs XhjobError 枚举设计（thiserror + #[source] 链）
  - [x] 1.2 验证 From<String>/From<io::Error> 支持 ? 传播
  - [x] 1.3 验证 errors.rs 类型化构造器 ipc/store/exec/config
  - [x] 1.4 验证 lib.rs ipc_request timeout 包裹 + ? 传播
  - [x] 1.5 验证 lib.rs dispatch() 错误返回 "error:" 前缀
  - [x] 1.6 验证 ipc/mod.rs write_frame/read_frame map_err 映射
  - [x] 1.7 验证 sqlite.rs/crypto.rs/cron.rs/http.rs/shell.rs 用 map_err + ?
  - [x] 1.8 验证 errors.rs 变体含 context 字符串
  - [x] 1.9 验证 ipc/mod.rs 错误含 op 上下文
  - [x] 1.10 验证 sqlite.rs map_err 含方法名上下文
  - [x] 1.11 验证 lib.rs ipc_request 错误含 op + timeout
  - [x] 1.12 验证 executor/http.rs 错误含操作描述

- [x] Task 2: Rust 静默吞错审计（项 13-22）
  - [x] 2.1 验证 ipc fire-and-forget tx.send 合理
  - [x] 2.2 验证 daemon let _ = remove_file 最佳努力清理合理
  - [x] 2.3 验证 shell let _ = child kill/wait 合理
  - [x] 2.4 验证 queue let _ = shutdown_tx.send 合理
  - [x] 2.5 验证 in_memory let _ = record_event 合理
  - [x] 2.6 审计 sqlite.rs:633 let _ = conn.execute 事件 INSERT（**FAIL** D-2）
  - [x] 2.7 验证 sqlite.rs maybe_decrypt 回退明文（文档化兼容）
  - [x] 2.8 审计 sqlite.rs:256,259 payload/state unwrap_or 吞损坏（**FAIL** D-3）
  - [x] 2.9 审计 sqlite.rs:306,926,944 tags/EventType unwrap_or（**FAIL** D-5）
  - [x] 2.10 审计 sqlite.rs:1013+ chain/group/chord list unwrap_or_default（**FAIL** D-4）

- [x] Task 3: Rust panic 风险 + 外部边界审计（项 23-35）
  - [x] 3.1 审计 sqlite.rs conn.lock().unwrap() x39 中毒 panic（**FAIL** D-1 HIGH）
  - [x] 3.2 审计 ipc/mod.rs:121 json.len() as u32 截断（**FAIL** D-6）
  - [x] 3.3 验证 daemon_main.rs expect SIGTERM/SIGINT（启动期合理）
  - [x] 3.4 验证 pool expect runtime/spawn（启动期合理）
  - [x] 3.5 验证 #[cfg(test)] expect/unwrap（测试代码）
  - [x] 3.6 验证 http.rs String::from_utf8 检测（无 lossy）
  - [x] 3.7 验证 shell.rs/cron.rs 安全 unwrap_or 回退
  - [x] 3.8 验证 HTTP 边界（timeout/redirect/body/binary/cancel/proxy）
  - [x] 3.9 验证 Shell 边界（soft_timeout/kill_on_drop/env_clear/output cap）
  - [x] 3.10 验证 SQLite 边界（WAL/busy_timeout/参数化）
  - [x] 3.11 验证 IPC 边界（8MB cap/fchmod/SO_PEERCRED）
  - [x] 3.12 验证 lib.rs ipc_request timeout 5s + daemon_main 连接 timeout 15s
  - [x] 3.13 验证 daemon_main XHJOB_MAX_CONNECTIONS 信号量 + ownership_check

- [x] Task 4: PHP 客户端错误处理审计（项 36-55）
  - [x] 4.1 验证 XhjobException 层级（基类 + 3 子类）
  - [x] 4.2 验证异常类型语义正确
  - [x] 4.3 验证 facade @method 标注一致
  - [x] 4.4 验证 create/chain/group/chord dispatch 抛 InvalidTaskConfigException
  - [x] 4.5 验证 get/list/state/logs 抛 ServiceNotRunningException
  - [x] 4.6 审计 result() 不抛异常（**FAIL** D-7）
  - [x] 4.7 审计 chainState/groupState/chordState 静默返回 null（**FAIL** D-8）
  - [x] 4.8 审计 pullEvents/inspect 静默返回 []（**FAIL** D-9）
  - [x] 4.9 验证 remove() 抛 TaskNotFoundException
  - [x] 4.10 验证 waitForState/waitForResult 轮询容错
  - [x] 4.11 验证 parseResponse 抛 InvalidTaskConfigException
  - [x] 4.12 验证 callWithRetry 业务错误透传 + setTimeout 软约束
  - [x] 4.13 审计 callWithRetry fallback 吞异常无日志（**FAIL** D-10）
  - [x] 4.14 审计 callWithRetry func_num_args() < 2 逻辑 bug（**FAIL** D-11）
  - [x] 4.15 验证 dispatch() 无 fallback 重试耗尽抛出
  - [x] 4.16 验证各方法 fallback 值（容错客户端设计）
  - [x] 4.17 验证 TaskManager @throws 与行为一致
  - [x] 4.18 验证 TaskBuilder @throws 与行为一致
  - [x] 4.19 验证 XhjobService @throws 与行为一致
  - [x] 4.20 验证 Client @throws 与行为一致

- [x] Task 5: 生成结构化审计报告
  - [x] 5.1 汇总 55 项 PASS/FAIL 与证据
  - [x] 5.2 FAIL 项按严重级别分类（1 HIGH + 7 MEDIUM + 3 LOW）
  - [x] 5.3 计算通过率 = 44/55 = 80.0% < 97% 目标（未达标）
  - [x] 5.4 输出缺陷清单表（D-1 至 D-11）+ 文件:行号 + 修复建议
  - [x] 5.5 提供达标路径建议

# Task Dependencies
- Task 2.6/2.8/2.9/2.10（SQLite 静默吞错 FAIL）为独立发现，集中在 store/sqlite.rs
- Task 3.1（SQLite Mutex unwrap HIGH）为最大 panic 风险源，独立发现
- Task 4.6/4.7/4.8（PHP TaskManager 错误路径不一致）属同一根因（错误路径未统一）
- Task 4.13/4.14（Client fallback 静默吞异常）属容错客户端设计但缺日志
- 所有 Task 并行完成后汇入 Task 5 报告
- 不重复维度 1/3 已发现问题（M-1 事务原子性、M-2 持锁 await）
