# Tasks

- [x] Task 1: 切换主分支并准备环境
  - [x] SubTask 1.1: `git checkout main && git pull origin main`，确保在干净 main 基础上实施（起始 commit 9b8e8ea）
  - [x] SubTask 1.2: 确认工作树干净，记录起始 commit

- [x] Task 2: 更细致的 Rust 内核审查与修复（`src/`）
  - [x] SubTask 2.1: 执行 `cargo build --release --features persist` 编译，0 警告
  - [x] SubTask 2.2: 执行 `cargo test --features persist`，139 passed / 0 failed
  - [x] SubTask 2.3: 逐函数审查 `src/lib.rs` 中 27 个 `xhjob_*` PHP 导出函数（注：xhjob_countdown 实为 TaskBuilder 方法，非顶层函数）
  - [x] SubTask 2.4: 深入审查 daemon/ipc/executor/pool/scheduler/store 各模块的并发与错误处理
  - [x] SubTask 2.5: 修复审查中发现的 bug（xhjob_modify 未注册到 get_module，已补 1 行）

- [x] Task 3: 更细致的 PHP 集成包审查与修复（`releases/xhjob-thinkphp8-extend/Xhjob/`）
  - [x] SubTask 3.1: 逐方法审查 TaskBuilder(38)/TaskManager(24)/XhjobService(8)/Client(15) 的实现正确性
  - [x] SubTask 3.2: 审查 ServiceProvider/facade/helper/Exception
  - [x] SubTask 3.3: 修复审查中发现的 bug（facade 补齐 reportProgress/pullEvents/inspect 3 个 @method 注解）

- [x] Task 4: 更细致的 tp 项目审查与修复（`tp/`）
  - [x] SubTask 4.1: 审查 `tp/app/controller/XhjobTask.php`、配置、路由、service.php
  - [x] SubTask 4.2: 审查现有测试脚本
  - [x] SubTask 4.3: 修复审查中发现的 bug（XhjobTask (string)$id 强转、multi_service 失效断言、debug_p0_2c 标注）

- [x] Task 5: 安装集成包到 tp 项目
  - [x] SubTask 5.1: 将 `releases/xhjob-thinkphp8-extend/Xhjob/` 复制到 `tp/extend/Xhjob/`
  - [x] SubTask 5.2: 执行 `composer install`，验证命名空间加载（TaskManager 实例化成功）

- [x] Task 6: 编写服务启动器 `tp/xhjob_server.php`
  - [x] SubTask 6.1: 实现 daemon 启动逻辑（XhjobService::start + 健康检查）
  - [x] SubTask 6.2: 打印 READY 后退出，验证 daemon 跨进程存活（server 退出后 daemon PPID=1 独立运行）

- [x] Task 7: 编写客户端功能测试器 `tp/xhjob_client_test.php`
  - [x] SubTask 7.1: 实现 step 测试框架 + 连接已运行 daemon
  - [x] SubTask 7.2: 测试生命周期函数（start/stop/restart/status）
  - [x] SubTask 7.3: 测试任务 CRUD 函数（dispatch/state/result/get/list/remove）
  - [x] SubTask 7.4: 测试任务控制函数（pause/resume/cancel/requeue/reschedule）
  - [x] SubTask 7.5: 测试编排函数（chain/group/chord/countdown + state 查询）
  - [x] SubTask 7.6: 测试可观测函数（events/report_progress/pull_events/inspect）
  - [x] SubTask 7.7: 测试 TaskBuilder 链式 API 全部方法
  - [x] SubTask 7.8: 测试复杂生产场景（17 类，见 spec）
  - [x] SubTask 7.9: 实际运行测试器，修复至 100% PASS（43 PASS / 0 FAIL / 2 SKIP）

- [ ] Task 8: 端到端跨进程验证
  - [ ] SubTask 8.1: 运行 `php -d extension=<so> tp/xhjob_server.php` 启动 daemon
  - [ ] SubTask 8.2: 运行 `php -d extension=<so> tp/xhjob_client_test.php` 连接测试
  - [ ] SubTask 8.3: 验证 server 退出后 daemon 仍存活，client 测试全部 PASS
  - [ ] SubTask 8.4: 运行 `cargo test --features persist` 确认 Rust 单元测试 100% 通过
  - [ ] SubTask 8.5: 汇总四维度通过率：代码质量 / 功能完善度 / bug 率 / 错误率 均达 100%

- [ ] Task 9: 编译并提交到远程主分支
  - [ ] SubTask 9.1: `cargo build --release --features persist` 产出最终 .so
  - [ ] SubTask 9.2: `git add` 所有修改代码 + `.so` + 新增文件
  - [ ] SubTask 9.3: `git commit` 创建提交（含详细提交信息）
  - [ ] SubTask 9.4: `git push origin main` 推送到远程主分支
  - [ ] SubTask 9.5: 验证推送成功，记录最终 commit hash

# Task Dependencies
- Task 1 必须最先执行（切换到 main）
- Task 2、Task 3、Task 4 可在 Task 1 后并行（不同代码层）
- Task 5 依赖 Task 1（在 main 上安装集成包）
- Task 6 依赖 Task 5（集成包安装后才能启动 daemon）
- Task 7 依赖 Task 6（client 连接 server 启动的 daemon）
- Task 8 依赖 Task 2、Task 7（全部代码就绪后端到端验证）
- Task 9 依赖 Task 8（验证全部通过后提交推送）
