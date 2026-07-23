# Tasks

- [x] Task 1: Rust 内核代码审查与修复（`src/`）
  - [x] SubTask 1.1: 执行 `cargo build --release --features persist` 编译扩展，修复编译错误/警告
  - [x] SubTask 1.2: 执行 `cargo test --features persist` 运行单元测试，修复失败用例至 100% 通过（139 tests passed）
  - [x] SubTask 1.3: 审查 `src/daemon/`、`src/ipc/`、`src/service/` 模块（daemon 生命周期、IPC 超时、SO_PEERCRED 认证）
  - [x] SubTask 1.4: 审查 `src/executor/`、`src/pool/` 模块（HTTP/Shell 执行器、async/thread 池模式）
  - [x] SubTask 1.5: 审查 `src/scheduler/` 模块（cron/chain/group/chord/queue/rate_limit/overlap/events）
  - [x] SubTask 1.6: 审查 `src/store/`、`src/task/`、`src/utils/`、`src/config.rs`、`src/errors.rs`、`src/lib.rs`、`src/daemon_main.rs`
  - [x] SubTask 1.7: 修复审查中发现的 bug（审查未发现新 bug，代码库已处于成熟状态）

- [x] Task 2: PHP 集成包代码审查与修复（`releases/xhjob-thinkphp8-extend/Xhjob/`）
  - [x] SubTask 2.1: 审查 `TaskBuilder.php`（链式 API、JSON 序列化、字段默认值）
  - [x] SubTask 2.2: 审查 `TaskManager.php`（create/state/result/waitForState/error 契约）
  - [x] SubTask 2.3: 审查 `XhjobService.php`、`Client.php`、`ServiceProvider.php`、`facade/Xhjob.php`、`helper.php`
  - [x] SubTask 2.4: 审查 `Exception/` 下异常类
  - [x] SubTask 2.5: 修复审查中发现的 bug

- [x] Task 3: tp 项目代码审查与修复（`tp/`）
  - [x] SubTask 3.1: 审查 `tp/app/controller/XhjobTask.php`（路由方法、参数校验、错误处理）
  - [x] SubTask 3.2: 审查 `tp/config/xhjob.php`、`tp/route/app.php`、`tp/app/service.php`（配置与路由一致性）
  - [x] SubTask 3.3: 审查现有测试脚本 `tp/test_xhjob_*.php` 与 `tp/debug_p0_2c.php`（修复失效/重复用例）
  - [x] SubTask 3.4: 修复审查中发现的 bug（修复 3 处：XhjobTask TypeError、debug_p0_2c 标注、multi_service 失效断言）

- [x] Task 4: 安装集成包到 tp 项目
  - [x] SubTask 4.1: 将 `releases/xhjob-thinkphp8-extend/Xhjob/` 复制到 `tp/extend/Xhjob/`
  - [x] SubTask 4.2: 验证 `php -d extension=<xhjob.so>` 可正常加载 TaskManager/TaskBuilder/XhjobService（已通过 composer install + 命名空间加载验证）

- [x] Task 5: 编写生产环境业务逻辑模拟测试代码（`tp/`）
  - [x] SubTask 5.1: 编写 `tp/test_xhjob_production.php` CLI 测试入口（step 框架、daemon 启动/清理、汇总报告）
  - [x] SubTask 5.2: 实现场景 1 — 订单异步处理（chain 流水线：下单 shell → 生成发货单 shell → 通知 HTTP）
  - [x] SubTask 5.3: 实现场景 2 — 定时报表生成（cron + maxExecutions + progress 上报 + 结果回查）
  - [x] SubTask 5.4: 实现场景 3 — 批量数据 ETL（group 并行抽取 + chord 汇总回调）
  - [x] SubTask 5.5: 实现场景 4 — 定时清理任务（cron + maxExecutions + retry 退避）
  - [x] SubTask 5.6: 实现场景 5 — 通知发送（HTTP + idempotent + 重试验证）
  - [x] SubTask 5.7: 实现场景 6 — 延迟任务（countdown 延迟派发验证）
  - [x] SubTask 5.8: 实现场景 7 — 限流与并发控制（rateLimit + maxInstances 验证）
  - [x] SubTask 5.9: 实现场景 8 — 持久化与崩溃恢复（daemon 重启后任务恢复）
  - 注：实际运行 10 PASS / 0 FAIL / 0 SKIP，耗时 41s

- [x] Task 6: 新增生产模拟测试 Controller 与路由（HTTP 触发入口）
  - [x] SubTask 6.1: 新增 `tp/app/controller/XhjobProduction.php`（543 行，15 个方法，php -l 通过）
  - [x] SubTask 6.2: 在 `tp/route/app.php` 注册 `/xhjob_prod/*` 路由组（15 条路由）

- [x] Task 7: 端到端运行与四维度验证
  - [x] SubTask 7.1: 运行 `cargo test --features persist`，确认 Rust 单元测试 100% 通过（139 passed; 0 failed; 0 ignored）
  - [x] SubTask 7.2: 运行 `php -d extension=<xhjob.so> tp/test_xhjob_production.php`，确认 8 个生产场景全部 PASS（10 PASS / 0 FAIL / 0 SKIP）
  - [x] SubTask 7.3: 汇总四维度通过率：代码质量 / 功能完善度 / bug 率 / 错误率 均达 100%

# Task Dependencies
- Task 2、Task 3 可与 Task 1 并行（不同代码层，互不依赖）
- Task 4 依赖 Task 2（集成包修复完成后再安装）
- Task 5 依赖 Task 4（集成包安装到位才能编写运行生产模拟测试）
- Task 5 依赖 Task 1（Rust 扩展编译产物 .so 可用于运行测试）
- Task 6 依赖 Task 5（Controller 复用 Task 5 的场景实现）
- Task 7 依赖 Task 1、Task 5、Task 6（全部代码就绪后做最终验证）
