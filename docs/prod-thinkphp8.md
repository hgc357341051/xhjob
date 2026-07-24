---
title: 生产实战：ThinkPHP 8 第三方类库集成
parent: 生产实战
nav_order: 54
---

# 生产实战：ThinkPHP 8 第三方类库集成

本篇演示如何把 xhjob 以**独立 composer 包**（`xhjob/thinkphp8-extend`）的形式集成进 ThinkPHP 8 应用：包提供 `ServiceProvider` 自动注册、`Xhjob` Facade、全局 helper 函数、配置文件与一组 HTTP 控制器路由，让你在 TP8 里既能以 PHP 代码调用任务 API，也能通过 HTTP 端点做运维管控。

包源码位于仓库 `releases/xhjob-thinkphp8-extend/`，命名空间为 `Xhjob\`。

## 安装

### 方式一：composer 安装（推荐）

包名 `xhjob/thinkphp8-extend`，依赖 `php >= 8.0.0` 与 `ext-xhjob`：

```bash
composer require xhjob/thinkphp8-extend
```

包的 `composer.json` 关键声明：

```json
{
    "name": "xhjob/thinkphp8-extend",
    "require": {
        "php": ">=8.0.0",
        "ext-xhjob": "*"
    },
    "autoload": {
        "psr-4": { "Xhjob\\": "Xhjob/" },
        "files": [ "Xhjob/helper.php" ]
    },
    "extra": {
        "think": {
            "services": [ "Xhjob\\ServiceProvider" ]
        }
    }
}
```

- `autoload.files` 中的 `Xhjob/helper.php` 会被 composer 自动引入，全局 helper 函数（`xhjob_manager()` 等）立即可用。
- `extra.think.services` 是 ThinkPHP 8 的**服务自动注册约定**：TP8 启动时会扫描已安装包的该字段并自动注册 `Xhjob\ServiceProvider`，无需手动改 `app/service.php`。

### 方式二：本地 path repository（开发 / 私有部署）

未发布到 Packagist 时，用 path repository 把本地包链接进来：

```json
// 你的 TP8 项目 composer.json
{
    "repositories": [
        { "type": "path", "url": "/path/to/releases/xhjob-thinkphp8-extend" }
    ],
    "require": {
        "xhjob/thinkphp8-extend": "*"
    }
}
```

然后 `composer update xhjob/thinkphp8-extend`，composer 会以 symlink 方式安装。

### 方式三：手动安装（无 composer）

无法用 composer 时，手动完成三步：

1. **复制类库**：把 `releases/xhjob-thinkphp8-extend/Xhjob/` 整个目录拷到 TP8 的 `extend/Xhjob/`（PSR-4 自动加载，命名空间 `Xhjob\` 对应 `extend/Xhjob/`）。
2. **复制配置**：把 `releases/xhjob-thinkphp8-extend/config/xhjob.php` 拷到 TP8 的 `config/xhjob.php`。
3. **注册服务**：在 `app/service.php` 追加 `\Xhjob\ServiceProvider::class`：

```php
// app/service.php
return [
    app\AppService::class,
    // Xhjob 定时任务扩展服务（注册 xhjob.service / xhjob.manager 到容器）
    \Xhjob\ServiceProvider::class,
];
```

4. **挂载路由与中间件**（可选，若需 HTTP 端点）：把 `route/app.php` 中的 `Route::group('xhjob', ...)` 复制到你的路由文件，并挂上 `XhjobAuth` 中间件。手动安装还需自行 `require` `Xhjob/helper.php`（或在入口引入），helper 函数才可用。

## ServiceProvider 自动注册

`Xhjob\ServiceProvider` 继承 `think\Service`，`boot()` 中向容器绑定两个单例：

```php
// Xhjob/ServiceProvider.php
class ServiceProvider extends Service
{
    public function boot()
    {
        // 注册 XhjobService（daemon 生命周期管理）
        $this->app->bind('xhjob.service', function (\think\App $app) {
            $name    = $app->config->get('xhjob.service_name', 'default');
            $dataDir = $app->config->get('xhjob.data_dir', null);
            return new XhjobService($name, $dataDir);
        });

        // 注册 TaskManager（任务 CRUD / 查询 / 控制）
        $this->app->bind('xhjob.manager', function (\think\App $app) {
            $name    = $app->config->get('xhjob.service_name', 'default');
            $dataDir = $app->config->get('xhjob.data_dir', null);
            return new TaskManager($name, $dataDir);
        });
    }
}
```

绑定关系：

| 容器标识 | 实例 | 用途 |
|---------|------|------|
| `xhjob.service` | `Xhjob\XhjobService` | daemon 生命周期：`start/stop/restart/status/healthCheck/ensureRunning/ensureStopped` |
| `xhjob.manager` | `Xhjob\TaskManager` | 任务操作：`create/state/result/list/pause/resume/remove/reschedule/...` |

两个实例都从 `config('xhjob.service_name')` 与 `config('xhjob.data_dir')` 读取默认服务名与数据目录，因此**改配置即切换服务**，无需改代码。

## Facade 用法

`Xhjob\facade\Xhjob` 把容器中的 `xhjob.manager`（`TaskManager` 实例）以静态方法形式暴露：

```php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

// 创建并派发任务
$id = Xhjob::create(TaskBuilder::shell('echo hi')->timeout(30));

// 查状态 / 取结果
$state  = Xhjob::state($id);
$result = Xhjob::result($id);

// 列表 / 暂停 / 恢复 / 删除
$list = Xhjob::list('pending');
Xhjob::pause($id);
Xhjob::resume($id);
Xhjob::remove($id);

// 编排
$chainId = Xhjob::createChain([
    TaskBuilder::shell('echo step1'),
    TaskBuilder::shell('echo step2'),
]);
```

Facade 通过 `getFacadeClass()` 绑定到容器标识 `xhjob.manager`：

```php
class Xhjob extends Facade
{
    protected static function getFacadeClass(): string
    {
        return 'xhjob.manager';
    }
}
```

### Facade 全方法列表（@method）

| 方法 | 签名 | 返回 |
|------|------|------|
| `create` | `create(TaskBuilder $b)` | `string` task_id |
| `createChain` | `createChain(array $builders)` | `string` chain_id |
| `createGroup` | `createGroup(array $builders)` | `string` group_id |
| `createChord` | `createChord(array $headerBuilders, TaskBuilder $callback)` | `string` chord_id |
| `update` | `update(string $id, TaskBuilder $b)` | `string` new task_id |
| `get` | `get(string $id)` | `array\|null` |
| `list` | `list(?string $stateFilter = null, ?string $tag = null)` | `array` |
| `state` | `state(string $id)` | `array` |
| `result` | `result(string $id)` | `array` |
| `stop` | `stop(string $id)` | `bool` |
| `restart` | `restart(string $id)` | `bool` |
| `pause` | `pause(string $id)` | `bool` |
| `resume` | `resume(string $id)` | `bool` |
| `remove` | `remove(string $id)` | `bool` |
| `logs` | `logs(string $id, int $sinceTs = 0)` | `array` |
| `reschedule` | `reschedule(string $id, string $cron)` | `bool` |
| `chainState` | `chainState(string $chainId)` | `array\|null` |
| `groupState` | `groupState(string $groupId)` | `array\|null` |
| `chordState` | `chordState(string $chordId)` | `array\|null` |
| `reportProgress` | `reportProgress(string $id, int $percent, ?string $meta = null)` | `bool` |
| `pullEvents` | `pullEvents(int $sinceTs = 0, ?string $eventType = null)` | `array` |
| `inspect` | `inspect(string $mode = 'stats')` | `array` |
| `waitForState` | `waitForState(string $id, string $expectedState, int $timeoutSec = 30)` | `bool` |
| `waitForResult` | `waitForResult(string $id, int $timeoutSec = 30)` | `array\|null` |

> `waitForState` / `waitForResult` 是**轮询阻塞**调用，仅用于 CLI worker / 测试，**绝不在 FPM 请求内使用**（会钉死 FPM worker）。

## helper 函数

`Xhjob/helper.php` 通过 composer `autoload.files` 全局引入，提供三个快捷函数：

```php
// 获取容器中的 TaskManager（需先注册 ServiceProvider）
$mgr = xhjob_manager();           // 等价于 app('xhjob.manager')

// 获取容器中的 XhjobService（daemon 生命周期管理）
$svc = xhjob_service();           // 等价于 app('xhjob.service')

// 一行派发 shell 任务（不传 service 时走容器默认服务）
$taskId = xhjob_task('php /app/jobs/report.php');
// 指定服务与数据目录
$taskId = xhjob_task('php /app/jobs/report.php', 'cron-svc', '/var/lib/xhjob');
```

| 函数 | 签名 | 返回 | 说明 |
|------|------|------|------|
| `xhjob_manager()` | `(): \Xhjob\TaskManager` | TaskManager | 从容器取默认 manager |
| `xhjob_service()` | `(): \Xhjob\XhjobService` | XhjobService | 从容器取默认 service |
| `xhjob_task(string $cmd, ?string $service = null, ?string $dataDir = null)` | `(): string` | task_id | 快捷创建并派发 shell 任务；`$service` 非 null 时新建 manager，否则用容器默认 |

## 配置文件

`config/xhjob.php` 全部键：

```php
return [
    // 服务名（多实例时通过该值区分 daemon 与 sock 文件）
    'service_name' => env('XHJOB_SERVICE', 'default'),
    // 数据目录（默认 null 由扩展内置）
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    // API Token（HTTP 网关鉴权，必须配置，否则中间件 fail-closed）
    'api_token'    => env('XHJOB_API_TOKEN', null),
    // 任务执行池模式：async（默认）/ thread / coroutine（async 别名）
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

| 配置键 | env 变量 | 默认 | 说明 |
|--------|---------|------|------|
| `service_name` | `XHJOB_SERVICE` | `default` | 服务名，决定 PID/sock/db/log 文件命名与多实例隔离 |
| `data_dir` | `XHJOB_DATA_DIR` | `null` | 统一数据目录；`null` 时由扩展按路径优先级解析（见[CLI 与 FPM 共用](prod-cli-fpm-share/)） |
| `api_token` | `XHJOB_API_TOKEN` | `null` | HTTP 网关鉴权 token，**必须配置**否则 `XhjobAuth` 中间件抛 500 |
| `pool_mode` | `XHJOB_POOL_MODE` | `async` | 线程池模式，详见[双线程池模式对比](../pool-modes/) |

> `pool_mode` 仅作为配置文档记录——它实际通过 env var `XHJOB_POOL_MODE` 在 daemon 启动时被读取（见 `src/scheduler/queue.rs`）。改 `pool_mode` 需重启 daemon（stop + start），运行期不可热切换。

## HTTP 端点

包提供一组以 `/xhjob` 为前缀的路由，对应 `app\controller\XhjobTask` 控制器。生产部署应挂上 `XhjobAuth` 中间件强制鉴权。

### 路由全表（25 个）

| # | 方法 | 路径 | 控制器动作 | 说明 |
|---|------|------|-----------|------|
| 1 | GET | `/xhjob/index` | `index` | 首页：daemon status + health |
| 2 | POST | `/xhjob/start` | `start` | 启动 daemon，返回 pid |
| 3 | POST | `/xhjob/stop` | `stop` | 停止 daemon（无 id）或取消任务（有 id） |
| 4 | POST | `/xhjob/restart` | `restart` | 重启 daemon（无 id）或重新入队任务（有 id） |
| 5 | GET | `/xhjob/status` | `status` | daemon 状态 |
| 6 | GET | `/xhjob/health` | `health` | 健康检查 |
| 7 | GET | `/xhjob/list` | `list` | 任务列表（可选 `state` / `tag` 过滤） |
| 8 | GET | `/xhjob/get?id=` | `get` | 任务详情 |
| 9 | POST | `/xhjob/create` | `create` | 创建任务（body = task JSON） |
| 10 | POST | `/xhjob/createShell` | `createShell` | 创建 shell 任务（`cmd` / `cron`） |
| 11 | POST | `/xhjob/createHttp` | `createHttp` | 创建 HTTP 任务（`method` / `url` / `body`） |
| 12 | POST | `/xhjob/createCron` | `createCron` | 创建 cron 任务（`cmd` / `cron` / `max_executions`） |
| 13 | POST | `/xhjob/createChain` | `createChain` | 创建任务链（`steps` 数组） |
| 14 | POST | `/xhjob/createGroup` | `createGroup` | 创建任务组（`tasks` 数组） |
| 15 | GET | `/xhjob/state?id=` | `state` | 任务状态 |
| 16 | GET | `/xhjob/result?id=` | `result` | 任务结果 |
| 17 | GET | `/xhjob/logs?id=&since_ts=` | `logs` | 任务事件流 |
| 18 | POST | `/xhjob/update?id=` | `update` | 编辑任务（重建） |
| 19 | POST | `/xhjob/pause?id=` | `pause` | 暂停任务 |
| 20 | POST | `/xhjob/resume?id=` | `resume` | 恢复任务 |
| 21 | POST | `/xhjob/reschedule?id=&cron=` | `reschedule` | 重新调度 |
| 22 | DELETE | `/xhjob/delete?id=` | `delete` | 删除任务 |
| 23 | GET | `/xhjob/chainState?id=` | `chainState` | 链状态 |
| 24 | GET | `/xhjob/groupState?id=` | `groupState` | 组状态 |
| 25 | POST | `/xhjob/demo` | `demo` | 综合演示（起停 daemon + 派发任务） |

> `stop` / `restart` 是**双重语义**：无 `id` 参数时操作 daemon，有 `id` 参数时操作单个任务。生产硬化版路由（`tp/route/app.php`）把 `demo` 设为 **POST**（避免 GET 触发爬虫 / 预检），并挂上 `XhjobAuth` 中间件：

```php
// tp/route/app.php（生产硬化版）
Route::group('xhjob', function () {
    Route::get('index', 'XhjobTask/index');
    Route::post('start', 'XhjobTask/start');
    // ... 其余 22 个路由 ...
    Route::post('demo', 'XhjobTask/demo');   // POST 防爬虫
})->middleware(\app\middleware\XhjobAuth::class);
```

## 鉴权中间件

`XhjobAuth` 中间件对 `/xhjob/*` 路由强制校验 `X-Xhjob-Token` header：

```php
// app/middleware/XhjobAuth.php
class XhjobAuth
{
    public function handle(Request $request, \Closure $next)
    {
        $expectedToken = config('xhjob.api_token');
        // 未配置 token 时 fail closed（拒绝所有请求），避免无鉴权 RCE
        if ($expectedToken === null || $expectedToken === '') {
            throw new HttpException(500, 'Xhjob API token not configured: set XHJOB_API_TOKEN env var');
        }
        // 仅从 header 读取 token（不接受 ?token= query，避免日志/Referer 泄露）
        $token = $request->header('X-Xhjob-Token', '');
        if (!hash_equals((string)$expectedToken, (string)$token)) {
            throw new HttpException(401, 'Unauthorized: invalid or missing Xhjob token');
        }
        return $next($request);
    }
}
```

设计要点：

- **未配置 token 时 fail-closed**：`api_token` 为 `null` 或空串时直接抛 **500**，拒绝所有请求（含 `createShell` 等 RCE 入口）。不用 `empty()`，因为字符串 `"0"` 是合法 token，`empty()` 会误判。
- **仅 header 鉴权**：token 只从 `X-Xhjob-Token` header 读取，不接受 `?token=` query——避免 token 泄露到 web server access logs / Referer / 浏览器历史。
- **恒定时间比较**：用 `hash_equals` 防 timing attack。

## 生产 controller 调用示例

下面演示在 TP8 controller 中完成"创建 cron 任务 → 查状态 → 取结果 → 取消"的完整流程：

```php
<?php
namespace app\controller;

use app\BaseController;
use think\Response;
use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\facade\Xhjob;

class Report extends BaseController
{
    /**
     * 创建每日报表任务
     */
    public function createDailyReport(): Response
    {
        // 方式一：Facade（走容器默认服务，读 config/xhjob.php）
        $taskId = Xhjob::create(
            TaskBuilder::shell('php /app/jobs/daily-report.php')
                ->cron('0 9 * * *')                 // 每天 09:00
                ->withTimezone('Asia/Shanghai')     // 时区
                ->persist(true)                     // 持久化
                ->acksLate(true)                    // 崩溃恢复
                ->maxExecutions(1000)               // 最多跑 1000 次
                ->withRetry(3, 60)                  // 重试 3 次
                ->retryBackoff(true)                // 指数退避
                ->timeout(1800)                     // 30 分钟硬超时
                ->tag('report')
        );

        return $this->json(['task_id' => $taskId], 201);
    }

    /**
     * 查任务状态
     */
    public function state(string $id): Response
    {
        return $this->json(Xhjob::state($id));
    }

    /**
     * 取任务结果
     */
    public function result(string $id): Response
    {
        return $this->json(Xhjob::result($id));
    }

    /**
     * 取消任务
     */
    public function cancel(string $id): Response
    {
        Xhjob::stop($id);       // stop = cancel
        return $this->json(['cancelled' => true]);
    }

    /**
     * 重新调度（改 cron）
     */
    public function reschedule(string $id): Response
    {
        $cron = $this->request->param('cron', '0 10 * * *');
        Xhjob::reschedule($id, $cron);
        return $this->json(['rescheduled' => true]);
    }

    protected function json($data, int $code = 200): Response
    {
        return Response::create($data, 'json', $code);
    }
}
```

### 直接用 TaskManager / XhjobService（不走容器默认）

需要切换服务时，直接 `new` 并传入 `service_name` + `data_dir`：

```php
<?php
// 用独立服务 'report-svc'，与默认服务隔离
$mgr = new TaskManager('report-svc', '/var/lib/xhjob');
$svc = new \Xhjob\XhjobService('report-svc', '/var/lib/xhjob');

$svc->ensureRunning();          // 确保 daemon 已启动

$taskId = $mgr->create(
    TaskBuilder::shell('php /app/jobs/daily-report.php')
        ->cron('0 9 * * *')
        ->persist(true)
);

// CLI worker 中查结果（FPM 内不要 waitForResult）
// $result = $mgr->waitForResult($taskId, 600);
```

## 通过 HTTP 端点管控

部署运维侧可用 curl 调 HTTP 端点（需带 `X-Xhjob-Token` header）：

```bash
TOKEN="your-secret-token"

# 启动 daemon
curl -X POST http://app.example.com/xhjob/start \
    -H "X-Xhjob-Token: $TOKEN"

# 创建 cron 任务
curl -X POST "http://app.example.com/xhjob/createCron" \
    -H "X-Xhjob-Token: $TOKEN" \
    -d 'cmd=php /app/jobs/cleanup.php' \
    -d 'cron=0 2 * * *' \
    -d 'max_executions=0'

# 查任务状态
curl "http://app.example.com/xhjob/state?id=TASK_ID" \
    -H "X-Xhjob-Token: $TOKEN"

# 查所有注册任务
curl "http://app.example.com/xhjob/list" \
    -H "X-Xhjob-Token: $TOKEN"

# 暂停 / 恢复 / 删除
curl -X POST "http://app.example.com/xhjob/pause?id=TASK_ID" -H "X-Xhjob-Token: $TOKEN"
curl -X POST "http://app.example.com/xhjob/resume?id=TASK_ID" -H "X-Xhjob-Token: $TOKEN"
curl -X DELETE "http://app.example.com/xhjob/delete?id=TASK_ID" -H "X-Xhjob-Token: $TOKEN"

# 停止 daemon（无 id）
curl -X POST http://app.example.com/xhjob/stop -H "X-Xhjob-Token: $TOKEN"
```

## 注意事项

| 关注点 | 说明 |
|------|------|
| **`api_token` 必须配置** | `XhjobAuth` 中间件 fail-closed：`api_token` 为 `null` 或空串时所有 `/xhjob/*` 请求抛 **500**。生产部署务必 `export XHJOB_API_TOKEN=<强随机串>`。字符串 `"0"` 是合法 token（不用 `empty()` 判断）。 |
| **`demo` 路由必须 POST** | `demo` 会起停 daemon 并派发任务，是状态变更操作。生产硬化版路由用 `POST`（防爬虫 / 预检触发），包内置的 `route/app.php` 用 GET 仅作演示，生产应替换为 POST 版本。 |
| **`stop` / `restart` 双重语义** | 无 `id` 参数 → 操作 daemon；有 `id` 参数 → 操作单个任务。调用时务必明确意图，避免误停 daemon。 |
| **Facade 走容器默认服务** | `Xhjob::xxx()` 读 `config('xhjob.service_name'/'data_dir')`。需切换服务时直接 `new TaskManager($name, $dataDir)`，不要依赖 Facade。 |
| **`waitForState/waitForResult` 仅 CLI** | 这两个方法轮询阻塞，FPM 请求内调用会钉死 worker。FPM 只做派发，等结果交给 CLI worker 或前端轮询。详见[后台任务队列](prod-background-queue/)。 |
| **扩展必须先加载** | 包依赖 `ext-xhjob`。若扩展未加载，调用 `xhjob_*` 函数会报 `Call to undefined function`。确认 `php -m \| grep xhjob` 或 `php -d extension=xhjob.so` 加载。 |
| **`pool_mode` 改后需重启 daemon** | `pool_mode` 通过 env var `XHJOB_POOL_MODE` 在 daemon 启动时读取一次，运行期不可热切换。改配置后需 `xhjob_stop` + `xhjob_start`。详见[双线程池模式对比](../pool-modes/)。 |
| **手动安装需注册 service** | 无 composer 时，手动安装必须把 `\Xhjob\ServiceProvider::class` 加到 `app/service.php`，否则容器无 `xhjob.service` / `xhjob.manager` 绑定，Facade 与 helper 函数都不可用。 |
