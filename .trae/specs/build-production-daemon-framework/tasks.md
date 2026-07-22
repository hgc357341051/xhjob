# Tasks

## 阶段一：框架核心层（framework/）

- [x] Task 1: 创建 framework/ 目录骨架 + composer.json 占位
  - [x] SubTask 1.1: 创建 framework/exceptions.php（XhjobFrameworkException 基类 + ServiceNotRunning + TaskNotFound + InvalidTaskConfig + IpcError）
  - [x] SubTask 1.2: 创建 framework/autoload.php（spl_autoload_register 简易 PSR-4）

- [x] Task 2: 实现 framework/XhjobService.php（daemon 生命周期管理）
  - [x] SubTask 2.1: start($name, $dataDir) 调用 xhjob_start + wait() 轮询 IPC ping
  - [x] SubTask 2.2: stop() / restart() 调用 xhjob_stop / xhjob_restart + wait() 等待退出/就绪
  - [x] SubTask 2.3: status() 调用 xhjob_status 返回 DaemonStatus 对象
  - [x] SubTask 2.4: healthCheck() 通过 IPC ping 验证 daemon 响应 + 返回 stats（worker_limits snapshot）
  - [x] SubTask 2.5: wait($timeoutSec=10, $expectRunning=true) 轮询直到状态匹配或超时抛 ServiceNotRunning

- [x] Task 3: 实现 framework/TaskBuilder.php（PHP fluent builder）
  - [x] SubTask 3.1: 静态工厂 shell($cmd) / http($method, $url) / chain(array) / group(array)
  - [x] SubTask 3.2: 配置方法（cron/every/runAt/timeout/priority/maxInstances/withRetry/maxExecutions/withMeta/tags/...）映射到 Xhjob 类或 dispatch JSON
  - [x] SubTask 3.3: dispatch($service=null, $dataDir=null) 终结方法，调用 xhjob_dispatch 或 xhjob_chain/xhjob_group
  - [x] SubTask 3.4: toArray() 序列化为 task JSON（供 TaskManager::create/update 使用）

- [x] Task 4: 实现 framework/TaskManager.php（任务管理门面）
  - [x] SubTask 4.1: create(TaskBuilder $b) 调用 xhjob_dispatch，返回 task_id
  - [x] SubTask 4.2: update($id, TaskBuilder $b) 先 remove 旧任务再 create 新任务（replaceExisting 语义）
  - [x] SubTask 4.3: get($id) 调用 xhjob_get 返回完整 Task 配置
  - [x] SubTask 4.4: list($stateFilter=null, $tag=null) 调用 xhjob_list 返回数组
  - [x] SubTask 4.5: stop($id) 调用 xhjob_cancel（Pending → Cancelled；Running → cancel_requested）
  - [x] SubTask 4.6: restart($id) 调用 xhjob_requeue（终态 → Pending + next_fire=now）
  - [x] SubTask 4.7: pause($id) / resume($id) 调用 xhjob_pause / xhjob_resume
  - [x] SubTask 4.8: remove($id) 调用 xhjob_remove
  - [x] SubTask 4.9: state($id) / result($id) 调用 xhjob_state / xhjob_result
  - [x] SubTask 4.10: logs($id, $sinceTs=0) 调用 xhjob_events 返回事件列表
  - [x] SubTask 4.11: reschedule($id, $cron) 调用 xhjob_reschedule

- [x] Task 5: 实现 framework/Client.php（跨环境客户端）
  - [x] SubTask 5.1: 构造函数 Client($name, $dataDir=null) + $timeoutSec 属性
  - [x] SubTask 5.2: dispatch(TaskBuilder $b) 通过 IPC 发送 dispatch op
  - [x] SubTask 5.3: state($id) / result($id) / get($id) / list() / events() 等查询方法
  - [x] SubTask 5.4: cancel($id) / pause($id) / resume($id) / requeue($id) / reschedule($id, $cron) / remove($id) 控制方法
  - [x] SubTask 5.5: retry($n, $delayMs) 链式方法配置重连
  - [x] SubTask 5.6: 与 TaskManager 的接口对齐（Client 可作为 TaskManager 的后端替代）

## 阶段二：HTTP API 层 + CLI 控制脚本

- [x] Task 6: 实现 framework/HttpApi.php（HTTP API 层）
  - [x] SubTask 6.1: handle(Request $req) 路由分发：GET/POST/DELETE → TaskManager 方法
  - [x] 6.1.1: POST /tasks → create；GET /tasks → list；GET /tasks/{id} → get；DELETE /tasks/{id} → remove
  - [x] 6.1.2: POST /tasks/{id}/restart → restart；POST /tasks/{id}/stop → stop；POST /tasks/{id}/pause → pause；POST /tasks/{id}/resume → resume
  - [x] 6.1.3: GET /tasks/{id}/state → state；GET /tasks/{id}/result → result；GET /tasks/{id}/logs → logs
  - [x] 6.1.4: POST /service/start / POST /service/stop / POST /service/restart / GET /service/status / GET /service/health
  - [x] SubTask 6.2: 鉴权中间件（X-Xhjob-Token header 校验，token 从 XHJOB_API_TOKEN env 读取）
  - [x] SubTask 6.3: JSON 响应封装（统一 {ok, data, error} 格式 + HTTP 状态码映射）

- [x] Task 7: 实现 web/index.php（FPM 入口）+ web/.htaccess
  - [x] SubTask 7.1: index.php 引入 autoload + HttpApi + 解析 PATH_INFO / REQUEST_METHOD 路由
  - [x] SubTask 7.2: .htaccess rewrite 规则（RewriteRule ^ index.php [L]）

- [x] Task 8: 实现 bin/xhjobctl（CLI 控制脚本）
  - [x] SubTask 8.1: 子命令 start|stop|restart|status|health 解析 + 调用 XhjobService
  - [x] SubTask 8.2: 子命令 list|run|stop-task|restart-task|logs|get 调用 TaskManager
  - [x] SubTask 8.3: --name / --data-dir / --state / --tag / --tail / --id 全局选项解析
  - [x] SubTask 8.4: 表格输出（list 命令用 sprintf 对齐）

## 阶段三：端到端测试

- [x] Task 9: tests/framework/fpm_framework_test.php（FPM 完整流程）
  - [x] 9.1: 启动 daemon + 等待就绪 + healthCheck
  - [x] 9.2: 新增 shell 任务（echo hello）+ 等待 SUCCESS + 验证 stdout
  - [x] 9.3: 新增 HTTP 任务（如可用则测试，否则 SKIP）
  - [x] 9.4: 新增 cron 任务（*/1 * * * * *）+ maxExecutions(3) + 等待 execution_count=3
  - [x] 9.5: list 查询 + 验证返回包含 3 个任务
  - [x] 9.6: 编辑任务（update: cron 改为 */2 + 验证 execution_count 重置）
  - [x] 9.7: stop 任务（cancel + 验证 Cancelled 终态）
  - [x] 9.8: restart 任务（requeue + 验证恢复 Pending）
  - [x] 9.9: logs 查询（events + 验证含 started/succeeded/cancelled）
  - [x] 9.10: 停止 daemon + 验证 PID 文件清理

- [x] Task 10: tests/framework/cli_client_test.php（CLI 客户端完整流程）
  - [x] 10.1: 用 bin/xhjobctl start 启动 daemon
  - [x] 10.2: 用 Client dispatch shell 任务 + 等待结果
  - [x] 10.3: state / result 查询
  - [x] 10.4: pause / resume
  - [x] 10.5: list 查询
  - [x] 10.6: events 查询
  - [x] 10.7: restart daemon（bin/xhjobctl restart）+ 验证 PID 变化
  - [x] 10.8: persist 模式下任务恢复（重启后 state 仍可查）
  - [x] 10.9: stop-task / restart-task 操作
  - [x] 10.10: 停止 daemon（bin/xhjobctl stop）

- [x] Task 11: tests/framework/cross_env_test.php（跨环境可用性）
  - [x] 11.1: PHP-FPM 进程（模拟）启动 daemon
  - [x] 11.2: CLI 进程用 Client 连接 daemon + dispatch 任务
  - [x] 11.3: PHP-FPM 进程查询任务状态（应一致）
  - [x] 11.4: CLI 重启 daemon
  - [x] 11.5: PHP-FPM 进程查询任务状态（persist 模式应可查）
  - [x] 11.6: 评估报告输出（通过率 + 性能指标 + 可用性评分）

## 阶段四：示例 + 评估

- [x] Task 12: examples/daemon_framework.php
  - [x] 12.1: 演示 XhjobService::start + TaskManager::create + TaskBuilder::shell/http/chain/group
  - [x] 12.2: 演示 Client 跨进程连接
  - [x] 12.3: 演示 stop / restart / logs

- [x] Task 13: 评估报告
  - [x] 13.1: 运行全部 framework 测试 + 记录通过率
  - [x] 13.2: 评估生产环境可用性（可靠性 / 性能 / 易用性 / 文档完整度）
  - [x] 13.3: 输出评估摘要到 stdout

# Task Dependencies

- Task 1 → Task 2/3/4/5（异常基类和 autoload 是其他类的基础）
- Task 2/3 → Task 4（TaskManager 依赖 XhjobService 和 TaskBuilder）
- Task 4 → Task 6（HttpApi 依赖 TaskManager）
- Task 4 → Task 8（xhjobctl 依赖 TaskManager）
- Task 2-8 → Task 9/10/11（测试依赖框架代码）
- Task 9-11 → Task 13（评估报告依赖测试结果）
