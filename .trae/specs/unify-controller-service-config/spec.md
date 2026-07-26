# 统一控制器服务名 / 数据目录配置 Spec

> change-id: `unify-controller-service-config`
> 前序：`fix-daemon-log-dir-fallback`（已合并 main，daemon 在非 root 用户下能正常启动）

## Why

当前 `XhjobTask` 控制器存在两个问题：

**1. 服务名 / 数据目录配置散落，不统一**

控制器里有 30+ 处 `new XhjobService()` / `new TaskManager()` 调用，几乎全部不传参数（默认走 `'default', null`），唯独 `demo()` 方法硬编码了 `'tp-demo', '/tmp/xhjob-tp-demo'`：

```php
// 普通方法（默认 default / null）
$svc = new XhjobService();
$mgr = new TaskManager();

// demo() 硬编码 tp-demo（不一致）
$svc = new XhjobService('tp-demo', '/tmp/xhjob-tp-demo');
$mgr = new TaskManager('tp-demo', '/tmp/xhjob-tp-demo');
```

这导致：
- `demo()` 创建的任务在 `tp-demo` 服务下，其它方法在 `default` 服务下 → `demo()` 跑完后用 `list()` 看不到任务
- 用户改 `config/xhjob.php` 的 `service_name` / `data_dir` 完全不生效（控制器根本没读 config）
- `ServiceProvider` 已经把 `service_name` / `data_dir` 注入到容器的 `xhjob.service` / `xhjob.manager`，但控制器直接 `new`，绕过了容器绑定

**2. 宝塔面板 PHP-FPM 以 `www` 用户执行，data_dir 默认 null 不安全**

`config/xhjob.php` 的 `data_dir` 默认是 `null`，由 Rust 扩展内部回退到 `/tmp`。`/tmp` 虽然 www 用户可写，但：
- 多用户共享，存在符号链接 / 名称抢占攻击面
- sticky bit 下其它用户无法删除别人的文件，但可读取（日志泄露风险）
- 与项目隔离，运维不好定位

更合理的默认值是 ThinkPHP 的 `runtime_path() . 'xhjob'`——这是项目自己的运行时目录，www 用户必然有写权限（项目文件本身就是 www 拥有），且与项目其它运行时文件（log / cache / session）位于同一棵目录树下，运维熟悉。

## What Changes

### MODIFIED: 控制器统一读取服务配置

**`releases/xhjob-thinkphp8-extend/controller/XhjobTask.php`** 和 **`tp/app/controller/XhjobTask.php`**：

新增 `protected` 辅助方法，从 `config('xhjob.*')` 读取统一的 `(service_name, data_dir)`，所有方法改为调用这两个辅助方法构造 `XhjobService` / `TaskManager`：

```php
/**
 * 从 config('xhjob.*') 读取统一的服务名。
 * 默认 'default'，可通过 config/xhjob.php 或 .env 的 XHJOB_SERVICE 覆盖。
 */
protected function xhjobServiceName(): string
{
    return (string) ($this->app->config->get('xhjob.service_name', 'default') ?? 'default');
}

/**
 * 从 config('xhjob.*') 读取统一的数据目录。
 * 默认回退到 runtime_path() . 'xhjob'，确保 www / nginx 等 web 用户可写。
 * 显式配置 null / '' 时也走 runtime_path 回退（不传给 Rust 扩展让其回退 /tmp）。
 */
protected function xhjobDataDir(): ?string
{
    $dir = $this->app->config->get('xhjob.data_dir');
    if (is_string($dir) && $dir !== '') {
        return $dir;
    }
    // 回退到 runtime_path/xhjob（web 用户必然可写）
    $runtime = runtime_path();
    return $runtime . 'xhjob';
}

/**
 * 构造 XhjobService，使用统一的服务名 + 数据目录。
 */
protected function xhjobService(): XhjobService
{
    return new XhjobService($this->xhjobServiceName(), $this->xhjobDataDir());
}

/**
 * 构造 TaskManager，使用统一的服务名 + 数据目录。
 */
protected function xhjobManager(): TaskManager
{
    return new TaskManager($this->xhjobServiceName(), $this->xhjobDataDir());
}
```

所有 `new XhjobService()` / `new TaskManager()` 调用替换为 `$this->xhjobService()` / `$this->xhjobManager()`。

### MODIFIED: demo() 方法放弃硬编码 tp-demo

`demo()` 方法原本用 `'tp-demo', '/tmp/xhjob-tp-demo'` 隔离演示数据。改为使用统一的 `$this->xhjobService()` / `$this->xhjobManager()`，让 demo 跑完后用户能直接通过 `/xhjob/list` 看到演示任务、通过 `/xhjob/get?id=xxx` 查询结果。

如果用户确实需要隔离 demo 数据，可通过 `.env` 设置 `XHJOB_SERVICE=tp-demo` + `XHJOB_DATA_DIR=/tmp/xhjob-tp-demo` 临时切换，无需改代码。

### MODIFIED: config/xhjob.php 默认 data_dir 改为 runtime_path

**`releases/xhjob-thinkphp8-extend/config/xhjob.php`** 和 **`tp/config/xhjob.php`**：

`data_dir` 字段的注释和默认值改为更明确的 www-用户友好路径：

```php
// 数据目录（默认 null 由控制器回退到 runtime_path/xhjob，
// 确保 www / nginx 等 web 用户可写；显式设置可覆盖）
'data_dir' => env('XHJOB_DATA_DIR', null),
```

**保持 `null` 默认值不变**，因为：
- `env()` 返回 null 时，控制器层会回退到 `runtime_path() . 'xhjob'`
- `XhjobService` / `TaskManager` 仍接受 null（透传给 Rust 扩展，扩展内部回退 `/tmp`）—— 不破坏底层契约
- 只有控制器层增加 `runtime_path()` 回退，不影响其它入口（CLI 脚本、Facade）

### ADDED: ServiceProvider 也用 runtime_path 回退

**`releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php`** 和 **`tp/extend/Xhjob/ServiceProvider.php`**（如果存在）：

容器绑定时同样增加 `runtime_path()` 回退，确保通过 `app('xhjob.service')` / `app('xhjob.manager')` / `\Xhjob\facade\Xhjob` 调用的代码也能拿到 www-可写的 data_dir：

```php
$this->app->bind('xhjob.service', function (\think\App $app) {
    $name    = $app->config->get('xhjob.service_name', 'default');
    $dataDir = $app->config->get('xhjob.data_dir');
    if (!is_string($dataDir) || $dataDir === '') {
        $dataDir = $app->getRuntimePath() . 'xhjob';
    }
    return new XhjobService($name, $dataDir);
});
```

## Impact

- **Affected specs**：无。本 spec 是 PHP 层调整，不涉及 Rust 扩展。
- **Affected code**：
  - `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php`：新增 4 个 protected 辅助方法，替换 30+ 处 `new XhjobService()` / `new TaskManager()`
  - `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php`：容器绑定增加 `runtime_path()` 回退
  - `releases/xhjob-thinkphp8-extend/config/xhjob.php`：注释更新（默认值不变）
  - `tp/app/controller/XhjobTask.php`：同步上述控制器改动
  - `tp/config/xhjob.php`：同步注释更新
- **BREAKING**：
  - `demo()` 方法不再使用 `tp-demo` 服务，改为默认 `default` 服务。原先 `/tmp/xhjob-tp-demo/` 下的 db / log / sock / pid 文件不再被 demo 创建。如果用户依赖 demo 隔离，需通过 `.env` 设置 `XHJOB_SERVICE` + `XHJOB_DATA_DIR`。
  - data_dir 默认从 `/tmp`（Rust 扩展回退）变为 `<runtime_path>/xhjob`（PHP 控制器回退）。**老用户升级后，原先在 `/tmp/xhjob.default.db` 的 SQLite 数据库不会自动迁移**——daemon 会在新目录下创建空 db，旧任务丢失。如果用户依赖持久化任务，需手动迁移：`cp /tmp/xhjob.default.db* <runtime_path>/xhjob/`。

## ADDED Requirements

### Requirement: 控制器统一服务配置
`XhjobTask` 控制器 SHALL 通过 `protected` 辅助方法 `xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 统一构造 `XhjobService` / `TaskManager`，所有方法（包括 `demo()`）SHALL 使用这些辅助方法，禁止直接 `new XhjobService(...)` / `new TaskManager(...)` 传硬编码参数。

#### Scenario: 默认配置
- **GIVEN** `config/xhjob.php` 未配置 `service_name` / `data_dir`，`.env` 也未设置 `XHJOB_SERVICE` / `XHJOB_DATA_DIR`
- **WHEN** 任意控制器方法调用 `$this->xhjobService()`
- **THEN** 返回 `XhjobService('default', '<runtime_path>/xhjob')`
- **AND** `<runtime_path>/xhjob` 目录存在且 www 用户可写（由控制器首次调用时创建）

#### Scenario: 用户显式配置 service_name
- **GIVEN** `.env` 设置 `XHJOB_SERVICE=tp-demo`
- **WHEN** 任意控制器方法调用 `$this->xhjobService()`
- **THEN** 返回 `XhjobService('tp-demo', '<runtime_path>/xhjob')`

#### Scenario: 用户显式配置 data_dir
- **GIVEN** `.env` 设置 `XHJOB_DATA_DIR=/var/lib/xhjob`
- **WHEN** 任意控制器方法调用 `$this->xhjobService()`
- **THEN** 返回 `XhjobService('default', '/var/lib/xhjob')`（不走 runtime_path 回退）

#### Scenario: demo() 与其它方法共享同一服务
- **GIVEN** 默认配置
- **WHEN** 用户依次调用 `/xhjob/demo` 然后 `/xhjob/list`
- **THEN** `list` 返回的数组包含 `demo` 创建的任务（同一 `default` 服务、同一 data_dir）

### Requirement: data_dir 默认回退到 runtime_path
当 `config('xhjob.data_dir')` 为 `null` 或空字符串时，控制器 / ServiceProvider SHALL 回退到 `runtime_path() . 'xhjob'`，确保 web 用户（www / nginx / apache）可写。

#### Scenario: 宝塔面板 www 用户
- **GIVEN** 宝塔面板部署，PHP-FPM 以 `www` 用户执行，项目根目录 `/www/wwwroot/myapp` 由 `www` 拥有
- **WHEN** 用户调用 `/xhjob/start`
- **THEN** data_dir = `/www/wwwroot/myapp/tp/runtime/xhjob/`
- **AND** `www` 用户对该目录有读写权限（继承自项目 runtime 目录）
- **AND** daemon 子进程以 www 用户启动，能创建 `xhjob.default.db` / `xhjob.default.log` / `xhjob.default.sock` / `xhjob.default.pid`

### Requirement: ServiceProvider 容器绑定同步回退
`ServiceProvider` 注册的 `xhjob.service` / `xhjob.manager` 容器绑定 SHALL 与控制器使用相同的 `runtime_path()` 回退逻辑，确保通过 Facade `\Xhjob\facade\Xhjob` 或 `app('xhjob.service')` 调用的代码拿到与控制器一致的 `XhjobService` / `TaskManager` 实例。

#### Scenario: Facade 与控制器一致
- **GIVEN** 默认配置
- **WHEN** 控制器方法内同时使用 `$this->xhjobManager()` 和 `\Xhjob\facade\Xhjob::create(...)`
- **THEN** 两者使用相同的 `service_name` + `data_dir`
- **AND** Facade 创建的任务能被控制器的 `list()` 方法看到

## MODIFIED Requirements

### Requirement: demo() 方法服务隔离
`demo()` 方法 SHALL 使用与其它方法相同的 `service_name` + `data_dir`（来自 `config('xhjob.*')`），不再硬编码 `'tp-demo', '/tmp/xhjob-tp-demo'`。需要隔离 demo 数据的用户 SHALL 通过 `.env` 配置 `XHJOB_SERVICE=tp-demo` + `XHJOB_DATA_DIR=/tmp/xhjob-tp-demo` 实现。

## REMOVED Requirements

### Requirement: demo() 硬编码 tp-demo 服务
**Reason**: 与其它方法不一致，导致 demo 创建的任务在 `list()` 中不可见，用户体验割裂。
**Migration**: 如需保留隔离，通过 `.env` 配置 `XHJOB_SERVICE` + `XHJOB_DATA_DIR`。
