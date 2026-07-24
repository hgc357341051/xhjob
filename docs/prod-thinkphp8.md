# ThinkPHP 8 集成

> 生产实战篇 · composer 安装 · 手动安装 · ServiceProvider 自动注册 · Facade · helper · 配置 · 生产 controller 示例

Xhjob 官方提供 ThinkPHP 8 扩展包 `xhjob/thinkphp8-extend`，把 PHP 扩展的底层 `xhjob_*` 函数封装为符合 ThinkPHP 习惯的 ServiceProvider、Facade、helper 与 TaskBuilder。安装后可通过容器绑定、Facade 静态调用、helper 函数三种方式在 controller / 命令中接入任务队列。

本文档按 **架构说明 → 完整可运行代码 → 注意事项 → 生产建议** 的结构展开，所有 API 严格对齐 `releases/xhjob-thinkphp8-extend/` 中的真实实现。

---

## 架构说明

```
┌──────────────────────────────────────────────────────────────┐
│                    ThinkPHP 8 应用                           │
│                                                              │
│  ┌─────────────┐   ┌──────────────────────┐  ┌────────────┐ │
│  │ Controller  │   │  Xhjob\ServiceProvider│  │  helper    │ │
│  │             │   │  (extra.think.services│  │  xhjob_*() │ │
│  │  Facade     │   │   自动注册)            │  │            │ │
│  │  Xhjob::..  │   │  bind xhjob.service   │  │  xhjob_    │ │
│  │             │   │  bind xhjob.manager   │  │   manager()│ │
│  └──────┬──────┘   └──────────┬───────────┘  │  xhjob_    │ │
│         │                     │              │   service()│ │
│         │ app('xhjob.manager')│              │  xhjob_    │ │
│         │                     │              │   task()   │ │
│         ▼                     ▼              └─────┬──────┘ │
│  ┌─────────────────────────────────────────────────▼──────┐ │
│  │  TaskManager (Xhjob\TaskManager)                       │ │
│  │  · create / state / result / stop / waitForResult ...  │ │
│  │  · 封装 xhjob_dispatch / xhjob_state / xhjob_result    │ │
│  │  └── XhjobService (Xhjob\XhjobService)                 │ │
│  │      · start / stop / restart / status / ensureRunning │ │
│  └────────────────────────┬───────────────────────────────┘ │
└───────────────────────────┼──────────────────────────────────┘
                            │ ext-xhjob (xhjob_* 函数)
                            ▼
                   ┌────────────────────┐
                   │  daemon (Rust)     │
                   └────────────────────┘
```

扩展包提供三层 API，按抽象度从高到低：

1. **Facade**（`\Xhjob\facade\Xhjob`）：静态方法风格，背后委托容器中的 `xhjob.manager`（TaskManager 实例）。最适合 controller 内快速调用。
2. **helper 函数**（`xhjob_manager()` / `xhjob_service()` / `xhjob_task()`）：全局函数，从容器取实例或快捷派发。适合不想引入 use 语句的场景。
3. **TaskManager / XhjobService 实例**：直接 `new` 或 `app('xhjob.manager')` 获取，方法最全。适合需要自定义 service_name / data_dir 的场景。

绑定关系：ServiceProvider 把 `xhjob.service` 绑定为 `XhjobService` 实例、`xhjob.manager` 绑定为 `TaskManager` 实例，两者都从 `config('xhjob.service_name')` 与 `config('xhjob.data_dir')` 读取默认服务名与数据目录。Facade `Xhjob` 的 `getFacadeClass()` 返回 `'xhjob.manager'`，因此 `Xhjob::create(...)` 等价于 `app('xhjob.manager')->create(...)`。

---

## 完整可运行代码

### 1. composer 安装

```bash
# 方式 A：从 Packagist 安装（发布后）
composer require xhjob/thinkphp8-extend

# 方式 B：本地 path repository（开发 / 未发布时）
# 在项目根 composer.json 添加：
#   "repositories": [
#     { "type": "path", "url": "../releases/xhjob-thinkphp8-extend" }
#   ]
composer require xhjob/thinkphp8-extend:@dev
```

composer.json 的 `extra.think.services` 字段声明了 ServiceProvider，ThinkPHP 8 会**自动注册**，无需手动改 `app/service.php`：

```json
{
    "extra": {
        "think": {
            "services": [
                "Xhjob\\ServiceProvider"
            ]
        }
    }
}
```

`autoload.files` 自动引入 `Xhjob/helper.php`，helper 函数全局可用。

### 2. 手动安装（无 composer 时）

```bash
# 1. 复制扩展包源码到项目
cp -r releases/xhjob-thinkphp8-extend/Xhjob/  tp/extend/Xhjob/
cp releases/xhjob-thinkphp8-extend/config/xhjob.php tp/config/xhjob.php

# 2. 追加路由（把扩展包的路由组合并到项目 route/app.php）
cat releases/xhjob-thinkphp8-extend/route/app.php >> tp/route/app.php

# 3. 注册 ServiceProvider（编辑 tp/app/service.php，加入）
#   return [
#       \Xhjob\ServiceProvider::class,
#       // ... 其他服务
#   ];

# 4. 手动引入 helper（若未走 composer autoload.files）
# 在 tp/app/common.php 顶部加：
#   require_once __DIR__ . '/../extend/Xhjob/helper.php';
```

### 3. 配置文件（`config/xhjob.php`）

扩展包提供 4 个配置键，全部支持 `env()` 覆盖：

```php
<?php
// config/xhjob.php
return [
    // 服务名（多实例时通过该值区分 daemon 与 sock 文件）
    'service_name' => env('XHJOB_SERVICE', 'default'),
    // 数据目录（默认 null 由扩展内置解析）
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    // API Token（必须配置，否则鉴权中间件抛 500）
    'api_token'    => env('XHJOB_API_TOKEN', null),
    // 任务执行池模式：async（默认，IO 密集）/ thread（CPU 密集）
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

| 配置键 | 默认值 | 对应 env 变量 | 说明 |
|--------|--------|--------------|------|
| `service_name` | `'default'` | `XHJOB_SERVICE` | 服务名，命名空间化 sock/pid/log/db 文件 |
| `data_dir` | `null` | `XHJOB_DATA_DIR` | 数据目录，`null` 走扩展内置路径解析 |
| `api_token` | `null` | `XHJOB_API_TOKEN` | **必须配置**，否则 HTTP 网关中间件抛 500 |
| `pool_mode` | `'async'` | `XHJOB_POOL_MODE` | `async`（tokio M:N）/ `thread`（1:1 OS 线程） |

### 4. Xhjob Facade 用法

Facade 把容器中的 `xhjob.manager`（TaskManager 实例）以静态方法形式暴露，共 **23 个 @method**：

```php
<?php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

// 创建任务
$id = Xhjob::create(TaskBuilder::shell('echo hello-thinkphp'));
$chainId = Xhjob::createChain([TaskBuilder::shell('echo a'), TaskBuilder::shell('echo b')]);
$groupId = Xhjob::createGroup([TaskBuilder::shell('echo g1'), TaskBuilder::shell('echo g2')]);
$chordId = Xhjob::createChord(
    [TaskBuilder::shell('echo h1'), TaskBuilder::shell('echo h2')],
    TaskBuilder::shell('echo callback')
);

// 更新任务（先 remove 再以同 id 重建）
Xhjob::update($id, TaskBuilder::shell('echo updated')->cron('*/5 * * * *'));

// 查询
Xhjob::get($id);                                    // 任务详情 array|null
Xhjob::list('running', 'report');                   // 任务列表 array
Xhjob::state($id);                                  // 状态 array
Xhjob::result($id);                                 // 结果 array
Xhjob::logs($id, time() - 3600);                    // 事件日志 array

// 控制
Xhjob::stop($id);                                   // 取消任务
Xhjob::restart($id);                                // 重新入队
Xhjob::pause($id);                                  // 暂停
Xhjob::resume($id);                                 // 恢复
Xhjob::remove($id);                                 // 删除
Xhjob::reschedule($id, '*/15 * * * *');             // 改 cron

// 链 / 组 / chord 状态
Xhjob::chainState($chainId);                        // array|null
Xhjob::groupState($groupId);                        // array|null
Xhjob::chordState($chordId);                        // array|null

// 进度 / 事件 / 检视
Xhjob::reportProgress($id, 50, json_encode(['step' => 'half']));
Xhjob::pullEvents(time() - 600, 'failed');          // 拉事件
Xhjob::inspect('stats');                            // 聚合状态

// 轮询等待（仅在 CLI 内用，勿在 FPM 内用）
Xhjob::waitForState($id, 'success', 30);            // bool
Xhjob::waitForResult($id, 30);                      // array|null
```

Facade 全方法 @method 列表（23 个）：

| 分类 | 方法 |
|------|------|
| 创建 | `create` / `createChain` / `createGroup` / `createChord` / `update` |
| 查询 | `get` / `list` / `state` / `result` / `logs` |
| 控制 | `stop` / `restart` / `pause` / `resume` / `remove` / `reschedule` |
| 编排状态 | `chainState` / `groupState` / `chordState` |
| 进度/事件/检视 | `reportProgress` / `pullEvents` / `inspect` |
| 轮询等待 | `waitForState` / `waitForResult` |

### 5. helper 函数

```php
<?php
// 获取 TaskManager 实例（从容器取 xhjob.manager，使用 config 默认 service_name / data_dir）
$mgr = xhjob_manager();          // \Xhjob\TaskManager
$id  = $mgr->create(TaskBuilder::shell('echo hi'));

// 获取 XhjobService 实例（daemon 生命周期管理）
$svc = xhjob_service();          // \Xhjob\XhjobService
$svc->ensureRunning();           // 确保 daemon 在运行（开发环境用）

// 快捷创建并派发 shell 任务（一行搞定）
$taskId = xhjob_task('php /app/bin/cleanup.php');   // 返回 task_id

// 指定服务名 / 数据目录的快捷派发
$taskId = xhjob_task('echo billing', 'billing', '/var/lib/xhjob-billing');
```

### 6. 生产 controller 调用示例

下面是一个完整的 controller，演示创建 cron 任务、查状态、取结果、取消任务：

```php
<?php
// app/controller/JobController.php
namespace app\controller;

use app\BaseController;
use think\Response;
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

class JobController extends BaseController
{
    protected function json($data, int $code = 200): Response
    {
        return Response::create($data, 'json', $code);
    }

    /**
     * POST /job/create
     *
     * 创建一个 cron 定时任务，立即返回 task_id。
     */
    public function create(): Response
    {
        $cmd  = $this->request->post('cmd', 'echo hello');
        $cron = $this->request->post('cron', '*/5 * * * *');

        $id = Xhjob::create(
            TaskBuilder::shell((string) $cmd)
                ->cron((string) $cron)
                ->withTimezone('Asia/Shanghai')
                ->persist(true)
                ->acksLate(true)
                ->withRetry(3, 5)
                ->retryBackoff(true)
                ->timeout(300)
                ->maxInstances(1)
                ->tag('demo')
        );

        return $this->json(['task_id' => $id], 201);
    }

    /**
     * GET /job/state?id=xxx
     */
    public function state(): Response
    {
        $id = $this->request->param('id');
        return $this->json(Xhjob::state((string) $id));
    }

    /**
     * GET /job/result?id=xxx
     */
    public function result(): Response
    {
        $id = $this->request->param('id');
        $r  = Xhjob::result((string) $id);
        // result 在无结果时返回带 error 键的数组（如 ignoreResult=true）
        return $this->json($r);
    }

    /**
     * POST /job/cancel?id=xxx
     */
    public function cancel(): Response
    {
        $id = $this->request->param('id');
        Xhjob::stop((string) $id);   // stop = cancel，保留记录
        return $this->json(['cancelled' => true, 'id' => $id]);
    }

    /**
     * GET /job/list
     */
    public function list(): Response
    {
        $state = $this->request->param('state');
        $tag   = $this->request->param('tag');
        return $this->json(Xhjob::list($state, $tag));
    }
}
```

对应路由（追加到 `route/app.php`）：

```php
use think\facade\Route;

Route::group('job', function () {
    Route::post('create', 'JobController/create');
    Route::get('state', 'JobController/state');
    Route::get('result', 'JobController/result');
    Route::post('cancel', 'JobController/cancel');
    Route::get('list', 'JobController/list');
});
```

### 7. CLI 命令内调用（ThinkPHP console）

```php
<?php
// app/command/JobStatus.php
namespace app\command;

use think\console\Command;
use think\console\Input;
use think\console\Output;
use Xhjob\facade\Xhjob;

class JobStatus extends Command
{
    protected function configure(): void
    {
        $this->setName('xhjob:status')
             ->addArgument('id')
             ->setDescription('查询 xhjob 任务状态');
    }

    protected function execute(Input $input, Output $output): void
    {
        $id = $input->getArgument('id');
        $state = Xhjob::state((string) $id);
        $output->writeln(sprintf(
            'task %s: state=%s attempts=%s progress=%s%%',
            $id,
            $state['state'] ?? 'UNKNOWN',
            $state['attempts'] ?? '0',
            $state['progress'] ?? '0'
        ));

        // CLI 内可安全使用 waitForResult（FPM 内禁止）
        $result = Xhjob::waitForResult((string) $id, 30);
        if ($result !== null && !isset($result['error'])) {
            $output->writeln('stdout: ' . substr((string) ($result['stdout'] ?? ''), 0, 500));
        }
    }
}
```

```bash
# 注册命令后执行
php think xhjob:status <task_id>
```

---

## 扩展包内置的 HTTP 端点

扩展包自带一个演示用的 controller（`controller/XhjobTask.php`）与路由（`route/app.php`），开箱即用提供 **25 个 HTTP 端点**（GET / POST / DELETE），覆盖 daemon 生命周期、任务 CRUD、链 / 组创建、状态查询、事件日志等。这些端点适合开发联调与内部运维面板。

> 路由前缀为 `/xhjob`，具体路由路径与 HTTP 方法请查阅源码 [`releases/xhjob-thinkphp8-extend/route/app.php`](https://github.com/xhjob/xhjob/blob/main/releases/xhjob-thinkphp8-extend/route/app.php)。生产环境**务必**配合鉴权中间件（`api_token`）暴露，不要直接对公网开放。

内置 controller 的关键设计：

- `stop` / `restart` 方法同时承担「停止 / 重启 daemon」与「停止 / 重启单个任务」两种语义，通过是否传入 `id` 参数区分：**无 id 操作 daemon，有 id 操作单个任务**。
- `demo` 端点用独立的 `tp-demo` 服务实例跑完整演示，避免污染默认服务。

---

## 注意事项

- **`api_token` 必须配置**：`config/xhjob.php` 的 `api_token` 默认 `null`。若启用了 HTTP 网关鉴权中间件，未配置 `XHJOB_API_TOKEN` 会导致中间件直接抛 **500** 错误。生产环境务必在 `.env` 中设置 `XHJOB_API_TOKEN=<long-random-token>`。
- **路由中 demo 类端点必须为 POST**：内置 controller 的 `create` / `createShell` / `createHttp` / `createCron` / `createChain` / `createGroup` / `update` / `pause` / `resume` / `reschedule` 等写操作均为 POST，避免被搜索引擎 / 爬虫意外触发（GET 会被爬虫遍历）。`demo` 端点虽是 GET（演示用），生产环境应移除或改为 POST。
- **`stop` / `restart` 无 id 时操作 daemon**：调用 `Xhjob::stop($id)` 传 id 是取消单个任务；若直接调 controller 的 `/xhjob/stop`（无 id 参数）会停止整个 daemon，影响所有任务。controller 内已通过 `if ($id) {...} else {...}` 区分，但调用方需注意传参。
- **TaskBuilder PHP 类用 `withId`，不是 `id`**：扩展包的 `Xhjob\TaskBuilder`（PHP 端实现）用 `withId(string $id)` 设置任务 ID。这与 Rust 侧 `Xhjob` 链式类的 `id()` 方法不同（Rust 端因 ext-php-rs 不转换单词方法名而暴露为 `id`）。两者来源不同，不要混淆。
- **ServiceProvider 自动注册依赖 `extra.think.services`**：composer 安装后 ThinkPHP 8 会自动扫描并注册 `Xhjob\ServiceProvider`。手动安装时需在 `app/service.php` 显式加入 `\Xhjob\ServiceProvider::class`，否则容器中无 `xhjob.manager` / `xhjob.service` 绑定，Facade 与 helper 调用会报错。
- **`waitForResult` / `waitForState` 仅限 CLI**：这两个方法内部是 `while` 轮询，会阻塞调用进程。FPM 请求内调用会占住 worker，严禁使用。在 ThinkPHP console 命令（CLI）内可安全使用。
- **Facade 读取容器默认服务**：`Xhjob::create(...)` 等价于 `app('xhjob.manager')->create(...)`，使用 `config('xhjob.service_name')` 与 `config('xhjob.data_dir')` 的默认值。需要操作其他服务时，直接 `new TaskManager($name, $dataDir)` 而非走 Facade。

---

## 生产建议

- **daemon 由 systemd 托管，不要在 controller 内 `ensureRunning`**：`XhjobService::ensureRunning()` 在 daemon 未运行时会调 `xhjob_start` 拉起，这在 FPM 请求内可能导致 spawn 阻塞。生产环境 daemon 应由 systemd 长驻（见 [CLI 与 FPM 共用服务连接](prod-cli-fpm-share.md)），controller 只做 IPC 客户端。
- **`.env` 分环境配置**：`XHJOB_SERVICE`、`XHJOB_DATA_DIR`、`XHJOB_API_TOKEN`、`XHJOB_POOL_MODE` 通过 `.env` 注入，开发 / 预发 / 生产各用不同值。生产 `XHJOB_DATA_DIR` 指向持久盘（`/var/lib/xhjob`），开发可留空走 `/tmp`。
- **HTTP 端点加鉴权中间件**：内置 25 个端点若暴露 HTTP，必须套一层 token 鉴权中间件（校验 `X-Xhjob-Token` 头 == `config('xhjob.api_token')`）。否则任何人都能通过 `/xhjob/stop` 停掉 daemon。生产建议只在内网 / VPN 内开放，或直接不挂载这些路由。
- **controller 内派发用 `try/catch`**：`TaskManager::create()` 在 daemon 返回 `error:` 时抛 `InvalidTaskConfigException`，`state()` / `result()` 在 daemon 不可达时抛 `ServiceNotRunningException`。controller 应捕获这些异常并返回友好错误，而非让 ThinkPHP 默认异常处理器暴露内部细节。
- **`pool_mode` 按任务类型选**：IO 密集（HTTP 请求、shell 命令）用默认 `async`（tokio M:N，最大并发 1024）；CPU 密集（纯计算）用 `thread`（1:1 OS 线程，线程数 = CPU 核数）。可通过 `XHJOB_POOL_MODE` 环境变量切换，无需改代码。
- **封装业务级 job service**：不要在 controller 里直接堆 `TaskBuilder::shell(...)->cron(...)->...`。把任务构建逻辑收敛到 `app/service/JobService.php`，controller 只调 `$jobService->dispatchReport($userId, $date)`，便于复用与测试。
