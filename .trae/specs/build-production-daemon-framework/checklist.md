# Checklist

## framework/ 核心层
- [x] framework/ 目录已创建
- [x] framework/exceptions.php 含 XhjobFrameworkException 基类 + 4 个子异常
- [x] framework/autoload.php spl_autoload_register 正常加载 framework/ 下所有类
- [x] framework/XhjobService.php 实现 start/stop/restart/status/healthCheck/wait
- [x] XhjobService::start 调用 xhjob_start + wait 等待 IPC 就绪
- [x] XhjobService::stop 调用 xhjob_stop + wait 等待退出
- [x] XhjobService::restart 调用 xhjob_restart + wait 等待就绪
- [x] XhjobService::status 调用 xhjob_status 返回 running + pid
- [x] XhjobService::healthCheck 通过 IPC ping 验证响应 + 返回 stats
- [x] XhjobService::wait 轮询直到状态匹配或超时抛 ServiceNotRunning
- [x] framework/TaskBuilder.php 实现 shell/http/chain/group 静态工厂
- [x] TaskBuilder 配置方法（cron/every/runAt/timeout/priority/maxInstances/withRetry/maxExecutions/withMeta/tags/...）全部映射
- [x] TaskBuilder::dispatch 调用 xhjob_dispatch 或 xhjob_chain/xhjob_group
- [x] TaskBuilder::toArray 序列化为 task JSON
- [x] framework/TaskManager.php 实现 create/update/get/list/stop/restart/pause/resume/remove/state/result/logs/reschedule
- [x] TaskManager::create 调用 xhjob_dispatch 返回 task_id
- [x] TaskManager::update 先 remove 旧任务再 create 新任务
- [x] TaskManager::stop 调用 xhjob_cancel
- [x] TaskManager::restart 调用 xhjob_requeue
- [x] TaskManager::logs 调用 xhjob_events
- [x] framework/Client.php 实现 dispatch/state/result/get/list/events/cancel/pause/resume/requeue/reschedule/remove
- [x] Client 构造函数含 name + dataDir + timeoutSec
- [x] Client::retry 链式方法配置重连

## HTTP API 层 + CLI
- [x] framework/HttpApi.php 路由分发（GET/POST/DELETE → TaskManager 方法）
- [x] HttpApi 处理 POST /tasks → create
- [x] HttpApi 处理 GET /tasks → list（支持 state + tag 过滤）
- [x] HttpApi 处理 GET /tasks/{id} → get
- [x] HttpApi 处理 DELETE /tasks/{id} → remove
- [x] HttpApi 处理 POST /tasks/{id}/restart → restart
- [x] HttpApi 处理 POST /tasks/{id}/stop → stop
- [x] HttpApi 处理 POST /tasks/{id}/pause → pause
- [x] HttpApi 处理 POST /tasks/{id}/resume → resume
- [x] HttpApi 处理 GET /tasks/{id}/state → state
- [x] HttpApi 处理 GET /tasks/{id}/result → result
- [x] HttpApi 处理 GET /tasks/{id}/logs → logs
- [x] HttpApi 处理 POST /service/start|stop|restart + GET /service/status|health
- [x] HttpApi 鉴权中间件（X-Xhjob-Token header 校验）
- [x] HttpApi JSON 响应统一格式 {ok, data, error}
- [x] web/index.php 引入 autoload + HttpApi + 解析路由
- [x] web/.htaccess rewrite 规则正确
- [x] bin/xhjobctl start|stop|restart|status|health 子命令
- [x] bin/xhjobctl list|run|stop-task|restart-task|logs|get 子命令
- [x] bin/xhjobctl 全局选项 --name/--data-dir/--state/--tag/--tail/--id
- [x] bin/xhjobctl list 表格输出对齐

## 端到端测试
- [x] tests/framework/fpm_framework_test.php 已创建
- [x] 9.1 启动 daemon + healthCheck PASS
- [x] 9.2 新增 shell 任务 + 验证 stdout PASS
- [x] 9.3 新增 HTTP 任务 PASS（或 SKIP 无网络）
- [x] 9.4 新增 cron 任务 + maxExecutions(3) + 验证 execution_count=3 PASS
- [x] 9.5 list 查询 PASS
- [x] 9.6 编辑任务 + 验证 execution_count 重置 PASS
- [x] 9.7 stop 任务 + 验证 Cancelled 终态 PASS
- [x] 9.8 restart 任务 + 验证恢复 Pending PASS
- [x] 9.9 logs 查询 + 验证事件流 PASS
- [x] 9.10 停止 daemon + 验证清理 PASS
- [x] tests/framework/cli_client_test.php 已创建
- [x] 10.1 bin/xhjobctl start 启动 daemon PASS
- [x] 10.2 Client dispatch shell 任务 + 等待结果 PASS
- [x] 10.3 state / result 查询 PASS
- [x] 10.4 pause / resume PASS
- [x] 10.5 list 查询 PASS
- [x] 10.6 events 查询 PASS
- [x] 10.7 restart daemon + 验证 PID 变化 PASS
- [x] 10.8 persist 模式任务恢复 PASS
- [x] 10.9 stop-task / restart-task PASS
- [x] 10.10 停止 daemon PASS
- [x] tests/framework/cross_env_test.php 已创建
- [x] 11.1 PHP-FPM 启动 daemon PASS
- [x] 11.2 CLI 连接 + dispatch PASS
- [x] 11.3 PHP-FPM 查询状态一致 PASS
- [x] 11.4 CLI 重启 daemon PASS
- [x] 11.5 persist 模式任务恢复 PASS
- [x] 11.6 评估报告输出 PASS

## 示例 + 评估
- [x] examples/daemon_framework.php 已创建
- [x] 示例演示 XhjobService + TaskManager + TaskBuilder + Client 完整流程
- [x] 评估报告含通过率
- [x] 评估报告含生产环境可用性评分（可靠性 / 性能 / 易用性）
- [x] 评估报告输出到 stdout

## 回归验证
- [x] cargo build --release --features persist 成功（未修改 Rust 代码，仅确认未破坏）
- [x] cargo test --release --lib --features persist 118 passed
- [x] libxhjob.so 已部署
- [x] 现有 tests/*.php 测试无回归（抽样运行 boundary_cases + functional_verify）
- [x] examples/daemon_framework.php 可运行无 fatal error

## 提交与推送
- [x] git status 核对修改文件清单
- [x] git add framework/ web/ bin/ tests/framework/ examples/daemon_framework.php
- [x] git commit -m "feat: 生产级 PHP-FPM 可控长驻定时任务框架（XhjobService/TaskManager/TaskBuilder/Client/HttpApi/xhjobctl）+ 端到端测试 + 评估报告"
- [x] git push origin main
- [x] git log origin/main --oneline -3 确认远程已更新
