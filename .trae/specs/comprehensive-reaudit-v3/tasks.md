# Tasks

## 阶段一：Rust 核心层 clippy 修复（阻塞后续编译验证）
- [x] Task 1: 修复 src/scheduler/cron.rs:939 double_parens clippy 错误
  - [x] SubTask 1.1: 将 `fresh.next_fire = Some(((now + 100)));` 改为 `fresh.next_fire = Some(now + 100);`
  - [x] SubTask 1.2: 运行 `cargo clippy --lib -- -D warnings` 确认该文件无警告
- [x] Task 2: 修复 src/retry/mod.rs 11 处 field_reassign_with_default clippy 错误
  - [x] SubTask 2.1: 将所有 `let mut x = TaskResult::default(); x.field = val;` 改为 `let x = TaskResult { field: val, ..Default::default() };`（涉及行 247-248, 252-253, 257-258, 272-273, 300-301, 314-315, 331-332, 350-351, 367-368, 380-381）
  - [x] SubTask 2.2: 运行 `cargo clippy --all-targets -- -D warnings` 确认零警告
  - [x] SubTask 2.3: 运行 `cargo build --release` 确认编译成功

## 阶段二：execution_lease bug 修复（worker_pid 写入时机）
- [x] Task 3: TaskStore trait 新增 update_worker_pid 方法
  - [x] SubTask 3.1: 在 src/store/mod.rs 的 TaskStore trait 中新增 `async fn update_worker_pid(&self, task_id: &str, worker_pid: u32, starttime: u64) -> Result<()>;`
  - [x] SubTask 3.2: 在 src/store/in_memory.rs 实现该方法（更新 Task.worker_pid 与新增 worker_starttime 字段）
  - [x] SubTask 3.3: 在 src/store/sqlite.rs 实现该方法（UPDATE tasks SET worker_pid=?, worker_starttime=? WHERE id=?）
  - [x] SubTask 3.4: SQLite schema 迁移：tasks 表新增 worker_pid INTEGER 与 worker_starttime INTEGER 列（CREATE TABLE IF NOT EXISTS 已含则跳过；既有库 ALTER TABLE ADD COLUMN）
- [x] Task 4: Task 结构体新增 worker_starttime 字段
  - [x] SubTask 4.1: src/store/mod.rs 的 Task 结构体新增 `#[serde(default)] pub worker_starttime: Option<u64>`（与 worker_pid 配对用于 PID 复用防护）
  - [x] SubTask 4.2: 确认 serde 反序列化兼容旧数据（default 属性）
- [x] Task 5: executor::dispatch 返回 worker_pid + starttime
  - [x] SubTask 5.1: 检查 src/executor/mod.rs 的 dispatch 函数返回值，确保包含 worker_pid
  - [x] SubTask 5.2: 检查 src/executor/shell.rs 与 http.rs 是否透传子进程 pid（HTTP 任务无子进程，worker_pid=None）
- [x] Task 6: scheduler/queue.rs process_one 在 spawn 后立即写入 worker_pid
  - [x] SubTask 6.1: 在 process_one 中 dispatch_task 完成后，立即调用 `store.update_worker_pid(&task.id, child_pid, starttime).await?`
  - [x] SubTask 6.2: 确认 worker_pid 在任务状态变为 Running 之后、executor 实际执行之前写入
  - [x] SubTask 6.3: 编写 Rust 单元测试验证 worker_pid 在 spawn 后非 None

## 阶段三：daemon 崩溃恢复 + PID 复用防护
- [ ] Task 7: 验证 src/daemon/mod.rs 的 process_starttime 与 is_process_alive_with_starttime 函数
  - [ ] SubTask 7.1: 读取 daemon/mod.rs 确认 process_starttime(pid) 读取 /proc/<pid>/stat 的 starttime 字段
  - [ ] SubTask 7.2: 确认 is_process_alive_with_starttime 同时校验 pid 存活 + starttime 匹配
  - [ ] SubTask 7.3: 编写 Rust 单元测试：活进程返回 true，死进程返回 false，PID 复用（starttime 不匹配）返回 false
- [ ] Task 8: 验证 reset_running_to_pending 崩溃恢复逻辑
  - [ ] SubTask 8.1: 读取 daemon 启动时 reset_running_to_pending 逻辑，确认读取 worker_pid + starttime 校验
  - [ ] SubTask 8.2: 确认 acks_late=true 的任务在孤儿进程检测后重置为 Pending
  - [ ] SubTask 8.3: 确认 acks_late=false 的任务保持 Running（不自动重置）
  - [ ] SubTask 8.4: 确认仍存活的子进程记录 LeaseHeld 事件且不重复派发

## 阶段四：watchdog 假死检测验证
- [ ] Task 9: 验证 src/scheduler/watchdog.rs 扫描逻辑
  - [ ] SubTask 9.1: 读取 watchdog.rs 确认 scan_once 遍历 load_active_tasks
  - [ ] SubTask 9.2: 确认对 Running + timeout>0 + started_at 存在的任务计算 elapsed
  - [ ] SubTask 9.3: 确认 elapsed > timeout * factor 时调用 signal_cancel + 状态置 Interrupted + 记录 HungDetected 事件
  - [ ] SubTask 9.4: 确认 watchdog 在 daemon 启动时被 spawn 为周期 task（默认 5s 间隔）
  - [ ] SubTask 9.5: 确认 daemon 关闭时 watchdog.stop() 被调用

## 阶段五：tp/repro/ 复现脚本（PHP + Rust 混合）
- [ ] Task 10: 复现脚本 repro_01 任务假死 watchdog 检测
  - [ ] SubTask 10.1: 派发 `sleep 100` shell 任务 timeout=2
  - [ ] SubTask 10.2: 等待 4s+（timeout * factor）
  - [ ] SubTask 10.3: 验证 state=interrupted
  - [ ] SubTask 10.4: 验证事件流含 HungDetected
- [ ] Task 11: 复现脚本 repro_02 PID 复用安全
  - [ ] SubTask 11.1: 派发任务记录 worker_pid + starttime
  - [ ] SubTask 11.2: 模拟 PID 复用（启动一个短命进程占用相同 pid 后退出，再启动新进程）
  - [ ] SubTask 11.3: 重启 daemon 验证 starttime 不匹配时不杀无害进程
- [ ] Task 12: 复现脚本 repro_03 崩溃不重复派发（execution_lease）
  - [ ] SubTask 12.1: 派发 `sleep 30` 长任务，验证 worker_pid 在 spawn 后立即写入 store
  - [ ] SubTask 12.2: kill -9 daemon，子进程被 init 收养仍存活
  - [ ] SubTask 12.3: 重启 daemon 验证 LeaseHeld 事件 + 不重复派发
  - [ ] SubTask 12.4: 验证 acks_late=true 任务在子进程死后才 reset_running_to_pending
- [ ] Task 13: 复现脚本 repro_04 SQLite 完整性检查
  - [ ] SubTask 13.1: 启动 daemon 写入若干任务
  - [ ] SubTask 13.2: 停止 daemon，手动破坏 SQLite 文件（截断）
  - [ ] SubTask 13.3: 重启 daemon 验证 PRAGMA quick_check 检测损坏并拒绝启动
- [ ] Task 14: 复现脚本 repro_05 send_terminate drain 对齐
  - [ ] SubTask 14.1: 派发多个任务，部分运行中
  - [ ] SubTask 14.2: 发送 SIGTERM，验证 wait_for_idle drain 完成后才退出
- [ ] Task 15: 复现脚本 repro_06 HTTP 任务取消
  - [ ] SubTask 15.1: 派发长时间 HTTP 任务
  - [ ] SubTask 15.2: 调用 xhjob_cancel
  - [ ] SubTask 15.3: 验证任务被取消
- [ ] Task 16: 复现脚本 repro_07 硬超时 SIGKILL
  - [ ] SubTask 16.1: 派发 `sleep 100` timeout=2 soft_timeout=None
  - [ ] SubTask 16.2: 验证硬超时后 SIGKILL 终止子进程
- [ ] Task 17: 复现脚本 repro_08 外键约束
  - [ ] SubTask 17.1: 验证 PRAGMA foreign_keys=ON
  - [ ] SubTask 17.2: 验证 results 表外键引用 tasks(id)
- [ ] Task 18: 复现脚本 repro_09 API token 鉴权
  - [ ] SubTask 18.1: 配置 xhjob.api_token
  - [ ] SubTask 18.2: 验证无 token 请求 401
  - [ ] SubTask 18.3: 验证正确 token 请求 200
  - [ ] SubTask 18.4: 验证 hash_equals 防时序攻击
- [ ] Task 19: 复现脚本 repro_10 HTTP 控制器集成
  - [ ] SubTask 19.1: 启动 PHP 内置 server 模拟 /xhjob/* 路由
  - [ ] SubTask 19.2: 验证 create/list/state/result 完整链路
- [ ] Task 20: 复现脚本 repro_11 daemon SIGKILL 恢复（与 repro_03 互补）
  - [ ] SubTask 20.1: 派发任务后 kill -9 daemon
  - [ ] SubTask 20.2: 重启验证 Running 任务恢复策略

## 阶段六：tp 业务层复审
- [ ] Task 21: 审查 app/middleware/XhjobAuth.php
  - [ ] SubTask 21.1: 验证 X-Xhjob-Token header 与 ?token= query 两种来源
  - [ ] SubTask 21.2: 验证 empty(api_token) 时直接放行（未配置鉴权）
  - [ ] SubTask 21.3: 验证 hash_equals 防时序攻击
- [ ] Task 22: 审查 app/controller/XhjobTask.php
  - [ ] SubTask 22.1: 验证所有路由方法对应 TaskManager/TaskBuilder/XhjobService 调用正确
  - [ ] SubTask 22.2: 验证 stop/restart 的 id 参数二义性处理
  - [ ] SubTask 22.3: 验证 create 接收 raw JSON body
- [ ] Task 23: 审查 releases/xhjob-thinkphp8-extend/ 下 Client.php / XhjobService.php / TaskBuilder.php / TaskManager.php / ServiceProvider.php / helper.php
  - [ ] SubTask 23.1: 验证 XhjobService.start/stop/restart/status/healthCheck/wait/ensureStopped 生命周期管理
  - [ ] SubTask 23.2: 验证 TaskBuilder 链式 API 与 Rust TaskBuilder 字段一一对应
  - [ ] SubTask 23.3: 验证 TaskManager.create/state/result/list/cancel/pause/resume 转发正确
  - [ ] SubTask 23.4: 验证 Client.php IPC 封装与错误处理
- [ ] Task 24: 审查 route/app.php 与 config/xhjob.php
  - [ ] SubTask 24.1: 验证 /xhjob 路由组挂载 XhjobAuth 中间件
  - [ ] SubTask 24.2: 验证 config 字段 service_name/data_dir/api_token/pool_mode

## 阶段七：全量编译与分维度验证
- [ ] Task 25: cargo clippy + build 验证
  - [ ] SubTask 25.1: `cargo clippy --all-targets -- -D warnings` 零警告
  - [ ] SubTask 25.2: `cargo build --release` 成功生成 libxhjob.so
- [ ] Task 26: tp/repro/runner.php 全量执行
  - [ ] SubTask 26.1: `php tp/repro/runner.php` 执行所有 repro_*.php
  - [ ] SubTask 26.2: 汇总所有脚本的 PASS/FAIL/SKIP，FAIL 必须为 0
  - [ ] SubTask 26.3: SKIP 需逐一注明原因并经用户确认合理
- [ ] Task 27: 分维度 100% 通过判定
  - [ ] SubTask 27.1: 正确率 100% — 所有 repro 中「正常使用」场景 PASS
  - [ ] SubTask 27.2: 代码质量 100% — clippy 零警告 + panic 边界安全
  - [ ] SubTask 27.3: 功能完善度 100% — lib.rs 所有 PHP API 有 tp 封装 + repro 覆盖
  - [ ] SubTask 27.4: bug 率 0% — 所有已发现问题有 repro 证明修复
  - [ ] SubTask 27.5: 错误率 0% — runner.php 汇总 FAIL=0

# Task Dependencies
- Task 2 依赖 Task 1（同属 clippy 修复，可并行但建议顺序）
- Task 3-6 依赖 Task 2（store/executor/queue 修改需先编译通过）
- Task 7-9 依赖 Task 6（崩溃恢复与 watchdog 依赖 worker_pid 写入）
- Task 10-20 依赖 Task 7-9（repro 脚本验证修复后的代码）
- Task 21-24 可与 Task 10-20 并行（tp 业务层审查不依赖 Rust 修改）
- Task 25-27 依赖所有前置任务完成
