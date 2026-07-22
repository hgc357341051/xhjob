# 生产级 PHP-FPM 可控长驻定时任务框架 Spec

## Why

xhjob 扩展已具备完整的底层能力（daemon 启停、任务调度、持久化、IPC），但**缺少站在 PHP 使用者角度的生产级框架代码**：PHP-FPM 进程如何控制 daemon 生命周期、如何在 Web 请求中管理任务（CRUD + 启停 + 日志）、以及独立环境连接框架服务的可用性验证。现有 `tests/business/fpm_sim/` 只覆盖单 worker HTTP 模拟，未形成可复用的框架层代码与端到端验证。

## What Changes

- 新增 `framework/` 目录，提供 PHP-FPM 可直接使用的长驻定时任务框架层代码：
  - `framework/XhjobService.php`：daemon 生命周期管理（start/stop/restart/status/wait、健康检查、PID 文件管理）
  - `framework/TaskManager.php`：任务管理门面（CRUD、启停、重排、列表、详情、事件查询、chain/group）
  - `framework/TaskBuilder.php`：PHP 端 fluent builder，封装 Xhjob 类所有配置项，支持 shell/http/chain/group 四种任务类型
  - `framework/HttpApi.php`：可选的 HTTP API 层（部署到 php-fpm，通过 HTTP 暴露全部 TaskManager 操作，含鉴权中间件）
  - `framework/Client.php`：独立环境连接框架服务的客户端（CLI 或 FPM 跨进程调用，支持超时、重连、批量）
  - `framework/exceptions.php`：框架异常体系（ServiceNotRunning / TaskNotFound / InvalidTaskConfig / IpcError）
- 新增 `web/` 目录，提供 PHP-FPM 部署的 HTTP API 入口：
  - `web/index.php`：HTTP API 路由入口（GET/POST/DELETE 映射到 TaskManager 方法）
  - `web/.htaccess`：Apache rewrite 规则
- 新增 `bin/xhjobctl`：CLI 控制脚本（替代 PHP-FPM 启动场景的命令行入口，支持 `start|stop|restart|status|list|run|stop-task|logs` 子命令）
- 新增端到端测试，覆盖 PHP-FPM 与 CLI 两种环境：
  - `tests/framework/fpm_framework_test.php`：PHP-FPM 环境完整流程测试（10 步）
  - `tests/framework/cli_client_test.php`：CLI 客户端连接框架服务测试（10 步）
  - `tests/framework/cross_env_test.php`：PHP-FPM 启动 daemon → CLI 客户端连接 → 操作任务 → FPM 验证结果（跨环境可用性）
- 新增 `examples/daemon_framework.php`：演示生产环境部署模式（fpm 启动 → 管理 → cli 连接 → 清理）

## Impact

- Affected specs: audit-and-polish-round2（底层能力已就绪，本 spec 构建其上的框架层）
- Affected code: 新增 framework/ web/ bin/ 目录，不修改现有 src/ Rust 代码（仅消费现有 PHP 扩展 API）

## ADDED Requirements

### Requirement: Daemon 生命周期管理（XhjobService）

系统 SHALL 提供 `XhjobService` 类，封装 daemon 的 start/stop/restart/status/wait/healthCheck 操作。

#### Scenario: FPM 进程启动 daemon
- **WHEN** PHP-FPM worker 调用 `XhjobService::start($name, $dataDir)`
- **THEN** daemon 启动并写入 PID 文件
- **AND** `wait()` 方法轮询直到 IPC socket 可连接（最多 10 秒）
- **AND** 返回 daemon PID

#### Scenario: 重启 daemon
- **WHEN** 调用 `restart()`
- **THEN** 旧 daemon 收到 SIGTERM 并退出
- **AND** 清理旧 PID/socket 文件
- **AND** 新 daemon 启动并就绪

#### Scenario: 健康检查
- **WHEN** 调用 `healthCheck()`
- **THEN** 返回 `['healthy' => bool, 'pid' => int|null, 'uptime' => int|null, 'stats' => array]`
- **AND** 通过 IPC `ping` 验证 daemon 响应

### Requirement: 任务管理门面（TaskManager）

系统 SHALL 提供 `TaskManager` 类，作为任务 CRUD + 启停 + 查询的统一入口。

#### Scenario: 新增 shell 任务
- **WHEN** 调用 `TaskManager::create(TaskBuilder::shell('echo hello')->cron('*/1 * * * *'))`
- **THEN** 任务持久化到 daemon store
- **AND** 返回 task_id

#### Scenario: 编辑任务（替换定义）
- **WHEN** 调用 `TaskManager::update($taskId, TaskBuilder::shell('echo new')->cron('*/5 * * * *'))`
- **THEN** 旧任务被 remove
- **AND** 新任务以相同 ID 插入（replaceExisting=true）
- **AND** execution_count 重置为 0

#### Scenario: 停止运行中的任务
- **WHEN** 调用 `TaskManager::stop($taskId)`
- **THEN** Pending 任务进入 Cancelled 终态
- **AND** Running 任务置 cancel_requested，执行完成后不再重试

#### Scenario: 重启任务
- **WHEN** 调用 `TaskManager::restart($taskId)`
- **THEN** 终态任务被 requeue 为 Pending
- **AND** next_fire 设为 now，立即触发

#### Scenario: 查询任务日志
- **WHEN** 调用 `TaskManager::logs($taskId, $sinceTs = 0)`
- **THEN** 返回该任务的全部事件记录（started/succeeded/failed/cancelled 等）
- **AND** 每条事件含 ts + event_type + payload

### Requirement: 任务构建器（TaskBuilder）

系统 SHALL 提供 PHP 端 fluent `TaskBuilder`，封装全部 35 项对齐配置项。

#### Scenario: 构建 shell cron 任务
- **WHEN** `TaskBuilder::shell('echo hi')->cron('*/1 * * * *')->withRetry(3, 1)->maxExecutions(10)->dispatch()`
- **THEN** 返回 task_id 字符串
- **AND** daemon store 中可见完整配置

#### Scenario: 构建 HTTP 任务
- **WHEN** `TaskBuilder::http('POST', 'https://api.example.com/webhook')->withBody('{"k":"v"}')->withHeaders(['X-Token: abc'])->dispatch()`
- **THEN** 任务以 HTTP 类型派发

#### Scenario: 构建 chain
- **WHEN** `TaskBuilder::chain([TaskBuilder::shell('step1'), TaskBuilder::shell('step2')])->dispatch()`
- **THEN** 返回 chain_id
- **AND** step1 完成后 stdout 作为 step2 的 XHJOB_CHAIN_INPUT

#### Scenario: 构建 group
- **WHEN** `TaskBuilder::group([TaskBuilder::shell('a'), TaskBuilder::shell('b'), TaskBuilder::shell('c')])->dispatch()`
- **THEN** 3 个任务并行派发
- **AND** group_state 返回 completed_count / total_count

### Requirement: HTTP API 层（HttpApi）

系统 SHALL 提供可选的 HTTP API 层，部署到 php-fpm 后通过 HTTP 暴露全部 TaskManager 操作。

#### Scenario: RESTful 任务管理
- **WHEN** `POST /tasks` body 含 task 配置 JSON
- **THEN** 创建任务并返回 201 + task_id
- **WHEN** `GET /tasks?state=PENDING&tag=billing`
- **THEN** 返回任务列表 JSON
- **WHEN** `DELETE /tasks/{id}`
- **THEN** 删除任务返回 204
- **WHEN** `POST /tasks/{id}/restart`
- **THEN** 重启任务
- **WHEN** `GET /tasks/{id}/logs?since_ts=0`
- **THEN** 返回任务事件流

#### Scenario: 鉴权中间件
- **WHEN** 请求未携带 `X-Xhjob-Token` header 或 token 不匹配
- **THEN** 返回 401 Unauthorized
- **AND** token 通过环境变量 `XHJOB_API_TOKEN` 配置

### Requirement: 跨环境客户端（Client）

系统 SHALL 提供 `Client` 类，供独立 PHP-CLI 或另一 PHP-FPM 环境连接框架服务。

#### Scenario: CLI 连接 daemon
- **WHEN** `$c = new Client('default', $dataDir); $c->dispatch(TaskBuilder::shell('echo hi'))`
- **THEN** 通过 IPC socket 发送 dispatch 请求
- **AND** 返回 task_id

#### Scenario: 超时与重连
- **WHEN** daemon 重启期间调用
- **THEN** Client 在超时（默认 5s）后抛 IpcError
- **AND** 提供 `retry($n, $delayMs)` 方法配置重连

### Requirement: CLI 控制脚本（xhjobctl）

系统 SHALL 提供 `bin/xhjobctl` CLI 脚本，支持以下子命令。

#### Scenario: 启动服务
- **WHEN** `php bin/xhjobctl start --name=cron-svc --data-dir=/var/lib/xhjob`
- **THEN** daemon 启动并输出 PID

#### Scenario: 列出任务
- **WHEN** `php bin/xhjobctl list --state=PENDING`
- **THEN** 表格输出任务列表

#### Scenario: 运行任务
- **WHEN** `php bin/xhjobctl run --id=abc123`
- **THEN** 任务被 requeue 并立即触发

#### Scenario: 查看日志
- **WHEN** `php bin/xhjobctl logs --id=abc123 --tail=50`
- **THEN** 输出最近 50 条事件

### Requirement: 端到端测试验证

系统 SHALL 提供端到端测试，覆盖 PHP-FPM 与 CLI 两种环境下的完整流程。

#### Scenario: FPM 框架完整流程
- **WHEN** 运行 `tests/framework/fpm_framework_test.php`
- **THEN** 10 步全部 PASS：启动 → 健康检查 → 新增 shell 任务 → 新增 HTTP 任务 → 新增 cron 任务 → 列表查询 → 编辑任务 → 停止任务 → 重启任务 → 查询日志 → 停止服务

#### Scenario: CLI 客户端连接
- **WHEN** 运行 `tests/framework/cli_client_test.php`
- **THEN** 10 步全部 PASS：CLI 启动 daemon → dispatch → 查询状态 → 查询结果 → pause → resume → list → events → restart daemon → 验证任务恢复

#### Scenario: 跨环境可用性
- **WHEN** 运行 `tests/framework/cross_env_test.php`
- **THEN** 验证 PHP-FPM 启动 daemon → CLI 客户端 dispatch 任务 → FPM 查询结果一致
- **AND** 验证 CLI 重启 daemon → FPM 任务状态仍可查询（persist 模式）
