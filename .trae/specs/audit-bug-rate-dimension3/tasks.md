# Tasks

- [x] Task 1: 状态机正确性审计（项 1-7）
  - [x] 1.1 验证 Pending→Running 转换（queue.rs process_one）
  - [x] 1.2 验证 Running→Success/Failed 转换（queue.rs lines 419-591）
  - [x] 1.3 验证 Running→Interrupted 转换（daemon_main.rs mark_running_as_interrupted）
  - [x] 1.4 验证 Pending→Cancelled 转换（in_memory.rs cancel_task / sqlite.rs cancel_task）
  - [x] 1.5 验证 Pending→Expired 转换（cron.rs expires 检查）
  - [x] 1.6 验证 Interrupted|Running→Pending 重置（reset_running_to_pending 两端实现）
  - [x] 1.7 验证无非法状态跳转（终态守卫 is_terminal）

- [x] Task 2: 边界与数据完整性审计（项 8-20）
  - [x] 2.1 空输入（TaskBuilder 校验 / executor 处理）
  - [x] 2.2 超大输入（shell.rs MAX_OUTPUT_BYTES / http.rs MAX_BODY_BYTES）
  - [x] 2.3 OverlapController 并发竞争（Mutex<HashMap>）
  - [x] 2.4 时区跨天（cron.rs tz 处理）
  - [x] 2.5 cron 边界 月末/闰年/2月29（cron 表达式解析）
  - [x] 2.6 SQLite 事务原子性（**FAIL**：delete_task/remove_task/cancel_task 未用事务）
  - [x] 2.7 加解密往返（crypto.rs AES-GCM + OsRng nonce）
  - [x] 2.8 serde 兼容性（#[serde(default)]）
  - [x] 2.9 u32/u64/i64 算术溢出（saturating_mul / attempts>=63 守卫）
  - [x] 2.10 时间戳溢出（now_ts saturating）
  - [x] 2.11 UTF-8/非UTF-8 body_b64（http.rs base64 编码）
  - [x] 2.12 SQL 注入（params![] 宏）
  - [x] 2.13 命令注入（Command::new arg 分离）

- [x] Task 3: 并发与资源审计（项 21-26）
  - [x] 3.1 OverlapController 计数竞争（Mutex 原子）
  - [x] 3.2 worker_limits 计数竞争（AtomicU64 fetch_add）
  - [x] 3.3 queue in-flight 计数竞争（InFlightGuard RAII）
  - [x] 3.4 进程回收（start_kill + wait reap）
  - [x] 3.5 文件句柄关闭（tokio Drop 语义）
  - [x] 3.6 锁释放持锁 await（mark_running_as_interrupted 先放锁再记录事件）

- [x] Task 4: 重点潜在 bug 审计（项 27-35）
  - [x] 4.1 scan_once cron next_fire 推进（cron.rs next_fire 滚动）
  - [x] 4.2 max_executions 终态时序（先 increment 再判 max）
  - [x] 4.3 retry_backoff 指数计算（backoff_delay 2^attempts + cap）
  - [x] 4.4 acks_on_failure=false 无限重试（u32::MAX 覆盖）
  - [x] 4.5 ignore_result=true 状态转换（仍用 dispatch 结果判成败）
  - [x] 4.6 replace_existing=true 旧结果清理（delete_task 清理任务+结果）
  - [x] 4.7 chord 回调携带全部结果（refresh_state 收集 results 到 meta）
  - [x] 4.8 chain 失败早终止（mark_failed 设 failed 终态，advance 返回 None）
  - [x] 4.9 group partial_failed 状态（refresh_state 计算 partial_failed）

- [x] Task 5: 生成结构化审计报告
  - [x] 5.1 汇总 35 项 PASS/FAIL 与证据
  - [x] 5.2 FAIL 项按严重级别分类（MEDIUM）
  - [x] 5.3 计算通过率 = 34/35 = 0.9714 ≥ 0.97

# Task Dependencies
- Task 2.6（SQLite 事务原子性 FAIL）为独立发现，不阻塞其它项
- 所有 Task 并行完成后汇入 Task 5 报告
