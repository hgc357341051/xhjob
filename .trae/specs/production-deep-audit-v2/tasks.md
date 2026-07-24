# Tasks

- [ ] Task 1: 环境基线确认与代码盘点
  - [ ] SubTask 1.1: 确认当前 HEAD（7a71a59）、工作树状态、Rust/PHP 代码量
  - [ ] SubTask 1.2: 确认 .so 已构建（target/release/libxhjob.so）与 releases/ 下 .so 一致性
  - [ ] SubTask 1.3: 确认 tp/extend/Xhjob/ 集成包已安装且命名空间可解析

- [ ] Task 2: Rust 内核深度审查（4 组并行）
  - [ ] SubTask 2.1: 审查组 A：`daemon/`（mod/unix/windows）+ `ipc/`（mod/unix_socket/named_pipe）+ `pool/`（mod/coroutine_pool/thread_pool）—— 进程生命周期、IPC 协议、并发池
  - [ ] SubTask 2.2: 审查组 B：`executor/`（mod/http/shell）+ `scheduler/`（mod/queue/overlap/rate_limit/cron/chain/group/chord/events）—— 执行器、调度器、编排原语
  - [ ] SubTask 2.3: 审查组 C：`store/`（mod/in_memory/sqlite/crypto）+ `task/` —— 持久化、AES-GCM 加密、任务模型、SQL、状态机
  - [ ] SubTask 2.4: 审查组 D：`outcome/` + `retry/` + `service/` + `utils/`（mod/limits/memory/metrics）+ `config.rs` + `errors.rs` + `lib.rs` + `daemon_main.rs` —— 支撑模块与 PHP 导出层
  - [ ] SubTask 2.5: 汇总四组审查结果，区分真实 bug / 误报 / 不确定项；对不确定项先询问用户

- [ ] Task 3: PHP 集成包与 tp 项目深度审查（并行）
  - [ ] SubTask 3.1: 逐方法审查 `releases/xhjob-thinkphp8-extend/Xhjob/` 全部类（TaskBuilder/TaskManager/XhjobService/Client/ServiceProvider/facade/helper/Exception）
  - [ ] SubTask 3.2: 审查 `tp/app/controller/XhjobTask.php`、配置、路由、service.php
  - [ ] SubTask 3.3: 审查现有测试脚本（test_xhjob*.php、xhjob_server.php、xhjob_client_test.php）的覆盖盲区
  - [ ] SubTask 3.4: 汇总 PHP 层问题，区分真实 bug / 误报 / 不确定项

- [ ] Task 4: 编写生产业务模拟测试套件 `tp/xhjob_production_business.php`
  - [ ] SubTask 4.1: 实现测试框架（启动 daemon → 10 个业务场景 → 清理）
  - [ ] SubTask 4.2: 场景1 订单异步处理 + 场景2 库存扣减 chain + 回滚
  - [ ] SubTask 4.3: 场景3 消息批量推送 group + 场景4 定时报表 cron + 文件验证
  - [ ] SubTask 4.4: 场景5 定时对账 interval + 场景6 失败补偿指数退避
  - [ ] SubTask 4.5: 场景7 软超时熔断 + 场景8 限流保护 + 场景9 幂等去重
  - [ ] SubTask 4.6: 场景10 崩溃恢复（acks_late + daemon 重启）
  - [ ] SubTask 4.7: 实际运行套件，修复至 100% PASS

- [ ] Task 5: 编写 Bug 复现套件 `tp/xhjob_bug_repro_suite.php`
  - [ ] SubTask 5.1: 实现套件框架（启动 daemon → 按 bug id 分 case → 清理）
  - [ ] SubTask 5.2: 为上一轮 11 个已修复 bug 编写回归 case（crypto/kill/RateLimiter/load_result/shell drain/service_name/outcome/modify_job/状态复活/溢出/未知 type）
  - [ ] SubTask 5.3: 为本轮新发现的 bug 编写复现 case（先复现 bug 行为，再验证修复后正确）
  - [ ] SubTask 5.4: 实际运行套件，修复至 100% PASS

- [ ] Task 6: 修复本轮新确认的真实 bug
  - [ ] SubTask 6.1: 对每个确认的真实 bug 做最小化针对性修复
  - [ ] SubTask 6.2: 对误报明确排除（记录原因）
  - [ ] SubTask 6.3: 对不确定项询问用户后处理

- [ ] Task 7: 端到端四维度 100% 验证
  - [ ] SubTask 7.1: `cargo test --features persist` 100% 通过
  - [ ] SubTask 7.2: `cargo build --release --features persist` 0 error 0 warning
  - [ ] SubTask 7.3: 跨进程 client_test 100% PASS（43+ PASS / 0 FAIL）
  - [ ] SubTask 7.4: 生产业务模拟测试 100% PASS（10 场景）
  - [ ] SubTask 7.5: bug 复现套件 100% PASS
  - [ ] SubTask 7.6: 汇总四维度通过率：正确率 / 代码质量 / 功能完善度 / bug 率与错误率 均达 100%

- [ ] Task 8: 编译并本地提交
  - [ ] SubTask 8.1: `cargo build --release --features persist` 产出最终 .so
  - [ ] SubTask 8.2: 复制 .so 到 `releases/xhjob-php8.2-linux-x86_64.so`
  - [ ] SubTask 8.3: `git add` 所有修改代码 + `.so` + 新增文件
  - [ ] SubTask 8.4: `git commit` 创建提交到本地 main（含详细提交信息）
  - [ ] SubTask 8.5: 验证提交成功，记录最终 commit hash（push 待用户提供凭据）

# Task Dependencies
- Task 1 必须最先执行（确认基线）
- Task 2 与 Task 3 可在 Task 1 后并行（不同代码层）
- Task 4 与 Task 5 可在 Task 2/3 审查初步完成后并行（测试套件编写）
- Task 6 依赖 Task 2/3（审查发现问题后修复）
- Task 5 的 SubTask 5.3 依赖 Task 6（新 bug 修复后才能写复现 case）
- Task 7 依赖 Task 4、Task 5、Task 6（全部就绪后端到端验证）
- Task 8 依赖 Task 7（验证全部通过后提交）
