# Checklist

## 阶段一：Rust clippy 修复
- [x] src/scheduler/cron.rs:939 的 `Some(((now + 100)))` 已改为 `Some(now + 100)`
- [x] src/retry/mod.rs 所有 `let mut x = TaskResult::default(); x.field = val;` 已改为 `let x = TaskResult { field: val, ..Default::default() };`
- [x] `cargo clippy --all-targets -- -D warnings` 退出码 0 且无任何输出
- [x] `cargo build --release` 成功生成 target/release/libxhjob.so

## 阶段二：execution_lease bug 修复
- [x] TaskStore trait 新增 `update_worker_pid(task_id, worker_pid, starttime)` 方法
- [x] InMemoryStore 实现 update_worker_pid（更新 Task.worker_pid 与 worker_starttime）
- [x] SqliteStore 实现 update_worker_pid（UPDATE tasks SET worker_pid=?, worker_starttime=?）
- [x] SQLite tasks 表 schema 含 worker_pid INTEGER 与 worker_starttime INTEGER 列
- [x] 既有 SQLite 库通过 ALTER TABLE ADD COLUMN 兼容迁移
- [x] Task 结构体新增 `worker_starttime: Option<u64>` 字段（serde default 兼容旧数据）
- [x] executor::dispatch 返回值含 worker_pid（HTTP 任务为 None）
- [x] scheduler/queue.rs process_one 在 dispatch 后立即调用 store.update_worker_pid
- [x] Rust 单元测试验证 worker_pid 在 spawn 后非 None

## 阶段三：daemon 崩溃恢复 + PID 复用防护
- [ ] daemon/mod.rs process_starttime(pid) 正确读取 /proc/<pid>/stat starttime 字段
- [ ] is_process_alive_with_starttime 同时校验 pid 存活 + starttime 匹配
- [ ] reset_running_to_pending 读取 worker_pid + starttime 校验孤儿进程
- [ ] acks_late=true 任务在孤儿进程检测后重置为 Pending
- [ ] acks_late=false 任务保持 Running（不自动重置）
- [ ] 仍存活子进程记录 LeaseHeld 事件且不重复派发
- [ ] Rust 单元测试覆盖：活进程/死进程/PID 复用三种场景

## 阶段四：watchdog 假死检测
- [ ] watchdog.rs scan_once 遍历 load_active_tasks
- [ ] 对 Running + timeout>0 + started_at 存在的任务计算 elapsed
- [ ] elapsed > timeout * factor 时调用 signal_cancel + 状态置 Interrupted + 记录 HungDetected
- [ ] watchdog 在 daemon 启动时被 spawn 为周期 task（默认 5s）
- [ ] daemon 关闭时 watchdog.stop() 被调用

## 阶段五：tp/repro/ 复现脚本
- [ ] repro_01 任务假死 watchdog 检测 PASS
- [ ] repro_02 PID 复用安全 PASS
- [ ] repro_03 崩溃不重复派发（execution_lease）PASS
- [ ] repro_04 SQLite 完整性检查 PASS
- [ ] repro_05 send_terminate drain 对齐 PASS
- [ ] repro_06 HTTP 任务取消 PASS
- [ ] repro_07 硬超时 SIGKILL PASS
- [ ] repro_08 外键约束 PASS
- [ ] repro_09 API token 鉴权 PASS
- [ ] repro_10 HTTP 控制器集成 PASS
- [ ] repro_11 daemon SIGKILL 恢复 PASS
- [ ] 所有 repro 脚本文件名匹配 `repro_NN_*.php`
- [ ] 所有 repro 脚本顶部 require _bootstrap.php
- [ ] 所有 repro 脚本调用 repro_header / repro_step / repro_summary
- [ ] 所有 repro 脚本结束时 stop_daemon + cleanup_data_dir
- [ ] 所有 SKIP 在 detail 中注明原因

## 阶段六：tp 业务层复审
- [ ] XhjobAuth.php 验证 X-Xhjob-Token header 与 ?token= query
- [ ] XhjobAuth.php empty(api_token) 时放行
- [ ] XhjobAuth.php 使用 hash_equals 防时序攻击
- [ ] XhjobTask.php 所有路由方法调用正确
- [ ] XhjobTask.php stop/restart 的 id 参数二义性处理正确
- [ ] XhjobService.php 生命周期管理（start/stop/restart/status/healthCheck/wait/ensureStopped）正确
- [ ] TaskBuilder.php 链式 API 与 Rust TaskBuilder 字段一一对应
- [ ] TaskManager.php create/state/result/list/cancel/pause/resume 转发正确
- [ ] Client.php IPC 封装与错误处理正确
- [ ] route/app.php /xhjob 路由组挂载 XhjobAuth 中间件
- [ ] config/xhjob.php 字段 service_name/data_dir/api_token/pool_mode 完整

## 阶段七：分维度 100% 通过判定
- [ ] 正确率 100% — 所有 repro 中「正常使用」场景 PASS
- [ ] 代码质量 100% — clippy 零警告 + panic 边界安全（无 extern "C" panic）
- [ ] 功能完善度 100% — lib.rs 所有 PHP API 有 tp 封装 + repro 覆盖
- [ ] bug 率 0% — 所有已发现问题有 repro 证明修复（修复前 FAIL，修复后 PASS）
- [ ] 错误率 0% — runner.php 汇总 FAIL=0
- [ ] `cargo clippy --all-targets -- -D warnings` 退出码 0
- [ ] `cargo build --release` 成功
- [ ] `php tp/repro/runner.php` 所有 repro FAIL=0
