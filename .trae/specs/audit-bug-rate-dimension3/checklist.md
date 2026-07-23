# Checklist

## 状态机正确性（项 1-7）
- [x] 项 1 Pending→Running：queue.rs process_one 设置 Running（有状态守卫）
- [x] 项 2 Running→Success|Failed：queue.rs lines 419-591 按执行结果转换
- [x] 项 3 Running→Interrupted：daemon_main.rs mark_running_as_interrupted
- [x] 项 4 Pending→Cancelled：in_memory.rs:167 / sqlite.rs:612 cancel_task
- [x] 项 5 Pending→Expired：cron.rs:200 expires 检查转 Expired 终态
- [x] 项 6 Interrupted|Running→Pending：reset_running_to_pending 同时处理两态
- [x] 项 7 无非法状态跳转：终态守卫 is_terminal() 阻止 Success/Failed→Running

## 边界与数据完整性（项 8-20）
- [x] 项 8 空输入：TaskBuilder 校验 charset，executor 容错
- [x] 项 9 超大输入：shell.rs:147 MAX_OUTPUT_BYTES=64MiB / http.rs:180 MAX_BODY_BYTES=64MiB
- [x] 项 10 OverlapController 并发竞争：overlap.rs Mutex<HashMap> 原子修改
- [x] 项 11 时区跨天：cron.rs 使用 chrono-tz 处理时区
- [x] 项 12 cron 边界 月末/闰年/2月29：cron 表达式解析器处理
- [ ] 项 13 SQLite 事务原子性：**FAIL** — delete_task(sqlite.rs:533)/remove_task(:572)/cancel_task(:612) 未用事务，崩溃可致孤儿行
- [x] 项 14 加解密往返：crypto.rs AES-256-GCM + OsRng 12字节 nonce，roundtrip 测试通过
- [x] 项 15 serde 兼容性：Task 结构 #[serde(default)] 兼容旧字段缺失
- [x] 项 16 u32/u64/i64 算术溢出：backoff_delay saturating_mul + attempts>=63 守卫
- [x] 项 17 时间戳溢出：now_ts duration_since.unwrap_or(0) saturating
- [x] 项 18 UTF-8/非UTF-8 body_b64：http.rs:217-231 String::from_utf8 检测，二进制走 base64
- [x] 项 19 SQL 注入：sqlite.rs 全部查询使用 params![] 宏参数化
- [x] 项 20 命令注入：shell.rs Command::new("bash").arg("-c").arg(&cmd) 非 shell 字符串拼接

## 并发与资源（项 21-26）
- [x] 项 21 OverlapController 计数竞争：overlap.rs on_start/on_finish Mutex 内 lock+modify
- [x] 项 22 worker_limits 计数竞争：limits.rs AtomicU64 fetch_add 原子
- [x] 项 23 queue in-flight 计数竞争：queue.rs InFlightGuard RAII Drop 递减
- [x] 项 24 进程回收：shell.rs timeout/cancel 路径 start_kill + wait 防僵尸
- [x] 项 25 文件句柄关闭：tokio Child/Stdio Drop 自动关闭管道
- [x] 项 26 锁释放持锁 await：in_memory.rs mark_running_as_interrupted 先收集 ids 放锁再记录事件

## 重点潜在 bug（项 27-35）
- [x] 项 27 scan_once cron next_fire 推进：cron.rs:347 next_fire(cron_expr, now+1, tz) 滚动
- [x] 项 28 max_executions 终态时序：queue.rs 先 increment_execution_count 再判 max
- [x] 项 29 retry_backoff 指数计算：retry/mod.rs backoff_delay 2^attempts capped 60×retry_delay
- [x] 项 30 acks_on_failure=false 无限重试：retry/mod.rs RetryPolicy::new(u32::MAX) 不达上限
- [x] 项 31 ignore_result=true 状态转换：queue.rs 仍用 dispatch 结果判 success/failed
- [x] 项 32 replace_existing=true 旧结果清理：daemon_main.rs:598 store.delete_task 清任务+结果行
- [x] 项 33 chord 回调携带全部结果：chord.rs refresh_state 收集 header results 到 callback meta
- [x] 项 34 chain 失败早终止：chain.rs mark_failed 设 failed 终态，advance 检测终态返回 None
- [x] 项 35 group partial_failed 状态：group.rs refresh_state 计算 succeeded>0 && failed>0 → partial_failed

## 通过率
- [x] PASS 项数 = 34
- [x] FAIL 项数 = 1（项 13，MEDIUM）
- [x] 通过率 = 34/35 = 0.9714 ≥ 0.97 目标 ✓
