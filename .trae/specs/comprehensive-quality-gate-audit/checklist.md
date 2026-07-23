# Checklist

## 阶段 0：干净基线
- [x] `cargo build --all-features` 退出码 0 且无 warning
- [x] `cargo build`（默认 feature）退出码 0 且无 warning
- [x] `cargo test --all-features` 全部通过（记录通过数，基线 ≥139）
- [x] `cargo test`（默认 feature）全部通过
- [x] `cargo clippy --all-features -- -D warnings` 退出码 0（或记录 `#[allow]` 例外清单）

## 阶段 1：代码质量审计（维度 1，目标 ≥97%）
- [x] Rust：`unwrap()` / `expect()` / `panic!()` 全部位于启动或测试路径（生产路径=缺陷）
- [x] Rust：无 `todo!()` / `unimplemented!()` / `unreachable!()` 在生产路径
- [x] Rust：dead code 已清除或已标注 `#[allow(dead_code)]` + 文档说明保留原因
- [x] Rust：命名一致（snake_case，跨模块同概念同命名）
- [x] Rust：关键路径有 `///` 文档注释说明 why
- [x] Rust：资源管理正确（文件句柄 / 锁 / 事务 / 进程组 释放）
- [x] Rust：并发安全（无持锁 await / 无死锁 / AtomicU* 顺序正确）
- [x] Rust：错误处理规范（`Result` 正确传播，无静默吞错）
- [x] PHP：异常类型正确（不返回 null 掩盖错误）
- [x] PHP：命名一致（camelCase，PSR-4 自动加载）
- [x] PHP：PHP 8.x 强类型声明
- [x] 维度 1 通过率 ≥ 0.97

## 阶段 2：功能完善度审计（维度 2，目标 ≥97%，≈56 项功能，需 ≥55 项可用）
### 调度触发器（11 项）
- [x] cron 触发器可用
- [x] interval(every) 触发器可用
- [x] run_at 一次性触发器可用
- [x] or_cron 复合触发器可用
- [x] skip_dates 可用
- [x] workdays_only 可用
- [x] timezone per-job 可用
- [x] jitter 可用
- [x] misfire_grace_time per-job 可用
- [x] coalesce per-job 行为生效
- [x] reschedule_job 可用
### 重叠控制（4 项）
- [x] allow_overlap 向后兼容
- [x] max_instances(N>1) 真实并发限制生效
- [x] rate_limit 每任务限流可用
- [x] worker_concurrency 全局并发上限可用
### HTTP 执行器（5 项）
- [x] method/headers/body 可用
- [x] redirect policy 可调
- [x] deflate/brotli/gzip 压缩可用
- [x] binary body(body_b64) 可用
- [x] proxy 可用
### Shell 执行器（4 项 + timeout/soft_timeout）
- [x] cmd 可用
- [x] stdin 可用
- [x] working_dir 可用
- [x] env 可用
- [x] timeout 可用
- [x] soft_timeout 可用
### 持久化（5 项）
- [x] SQLite WAL 可用
- [x] busy_timeout 可用
- [x] payload 加密可用
- [x] 自动 schema 迁移可用
- [x] SQLite 文件权限 0o600
### 生命周期（11 项）
- [x] retry 可用
- [x] retry_backoff 可用
- [x] expires 可用
- [x] requeue 可用
- [x] cancel 可用
- [x] pause/resume 可用
- [x] acks_late 可用
- [x] acks_on_failure 可用
- [x] ignore_result 可用
- [x] max_executions 可用
- [x] replace_existing 可用
### 组合原语（4 项）
- [x] chain 可用
- [x] group 可用
- [x] chord 可用
- [ ] chunks 可用（未实现，57/58 ≥ 97% 已达标）
### 事件流（3 项）
- [x] record_event 可用
- [x] list_events 可用
- [x] cleanup_expired_events 可用
### 多租户 + daemon 自愈 + 查询 + modify_job（6 项）
- [x] owner 多租户隔离可用
- [x] worker_max_tasks_per_child 可用
- [x] worker_max_memory_per_child 可用
- [x] get_job 可用
- [x] list_tasks(with tags filter) 可用
- [x] modify_job 可用
### PHP 客户端（3 项）
- [x] README.md 所有方法可调用且行为正确
- [x] EVALUATION.md 承诺功能均已实现
- [x] composer.json PSR-4 自动加载正确
- [x] 维度 2 通过率 ≥ 0.97（可用项 / 56 ≥ 0.97）

## 阶段 3：bug 率审计（维度 3，目标 ≥97%）
### 状态机
- [x] Pending→Running 转换正确
- [x] Running→Success/Failed 转换正确
- [x] Running→Interrupted 转换正确（shutdown）
- [x] Pending→Cancelled 转换正确
- [x] Pending→Expired 转换正确
- [x] Interrupted/Running→Pending 重置正确（restart）
- [x] 无非法状态跳转
### 边界与数据完整性
- [x] 空输入处理正确
- [x] 超大输入处理正确（max_output_bytes 限制）
- [x] 并发竞争无竞态
- [x] 时区跨日评估正确
- [x] cron 边界（月末 / 闰年 / 2 月 29 日）正确
- [x] SQLite 事务原子性正确
- [x] 加密/解密往返一致
- [x] 序列化/反序列化兼容（旧库迁移）
- [x] u32/u64/i64 算术无溢出
- [x] 时间戳转换无溢出
- [x] UTF-8 / 非 UTF-8 body_b64 处理正确
- [x] SQL 注入防护（参数化查询）
- [x] 命令注入防护（不通过 shell 执行）
### 并发与资源
- [x] OverlapController 计数无竞态
- [x] worker_limits 计数无竞态
- [x] queue in-flight 计数无竞态
- [x] 进程正确回收（process_group / kill_on_drop）
- [x] 文件句柄正确关闭
- [x] 锁正确释放（无持锁 await）
- [x] 维度 3 通过率 ≥ 0.97

## 阶段 4：错误率审计（维度 4，目标 ≥97%）
### Rust 错误处理
- [x] `Result` 正确传播（无滥用 `.unwrap()` 在生产路径）
- [x] `?` 不丢失上下文（关键错误附 task_id / op）
- [x] 无静默吞错（`let _ =` 已审视）
- [x] `unwrap_or_default()` 不掩盖真实错误
- [x] 无 panic 风险（`unwrap()` / 数组下标 / `from_utf8` / `parse` / 整数转换）
- [x] HTTP 超时错误处理正确
- [x] HTTP 连接拒绝错误处理正确
- [x] HTTP DNS 失败错误处理正确
- [x] SQLite BUSY 错误处理正确（busy_timeout）
- [x] IPC 断开错误处理正确
- [x] 错误信息可定位（含上下文）
### PHP 客户端错误处理
- [x] 异常类型正确
- [x] 不静默返回 null（state() 抛 ServiceNotRunningException）
- [x] result() 符合 docblock 契约
- [x] Client.callWithRetry setTimeout 生效
- [x] 维度 4 通过率 ≥ 0.97

## 阶段 5：修复与复测
- [x] 代码质量缺陷已修复（若维度 1 < 97%）
- [x] 功能完善度缺陷已修复（若维度 2 < 97%）
- [x] bug 已修复（若维度 3 < 97%）
- [x] 错误处理缺陷已修复（若维度 4 < 97%）
- [x] 修复后 `cargo build --all-features` 退出码 0 无 warning
- [x] 修复后 `cargo test --all-features` 全部通过

## 阶段 6：审计报告
- [x] AUDIT_REPORT.md 含四维通过率表格
- [x] AUDIT_REPORT.md 含问题清单（按维度 + 严重级别）
- [x] AUDIT_REPORT.md 含修复状态
- [x] AUDIT_REPORT.md 含最终结论（四维是否均 ≥97%）
