# 多实例配置模式 Spec

> change-id: `multi-instance-config`
> 前序：`unify-controller-service-config`（已完成，控制器统一通过 `config('xhjob.*')` 读取服务配置）

## Why

当前 `XhjobService` / `TaskManager` 的 `__construct(string $name='default', ?string $dataDir=null)` 默认 `dataDir=null`，而 `config/xhjob.php` 只支持**单套** `service_name` + `data_dir` 配置。这导致两个问题：

**1. 单配置无法满足多业务隔离需求**

实际生产中，用户可能需要同时跑多套独立的 daemon 实例（不同业务隔离、不同 data_dir 分盘、不同 service_name 做灰度等）。当前配置只能定义一套，要切换实例必须改 `.env` 或改代码，无法在同一个项目里同时使用多套。

**2. `dataDir=null` 默认值导致执行报错**

用户反馈："`__construct` 里面 `$dataDir` 都默认 null，导致执行报错，因为控制器里也没有设置"。虽然 `unify-controller-service-config` 已经在控制器层增加了 `runtime_path()` 回退，但用户希望从配置层面就明确多套 data_dir，避免依赖隐式回退。

## What Changes

### ADDED: 多实例配置结构

**`releases/xhjob-thinkphp8-extend/config/xhjob.php`** 和 **`tp/config/xhjob.php`**：

支持两种配置模式，**自动识别**（向后兼容）：

#### 模式 A：单实例模式（老配置，向后兼容）

```php
return [
    'service_name' => env('XHJOB_SERVICE', 'default'),
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    'api_token'    => env('XHJOB_API_TOKEN', null),
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
```

老用户配置不动也能工作。系统内部把它视为只有一个 `'default'` 实例的多实例配置。

#### 模式 B：多实例模式（新配置）

```php
return [
    // 默认实例名（控制器不传 instance 参数时用这个）
    'default' => 'data1',

    // 多实例配置：每个实例独立的 service_name + data_dir
    'instances' => [
        'data1' => [
            'service_name' => env('XHJOB_SERVICE_DATA1', 'default'),
            'data_dir'     => env('XHJOB_DATA_DIR_DATA1', null),
        ],
        'data2' => [
            'service_name' => env('XHJOB_SERVICE_DATA2', 'data2'),
            'data_dir'     => env('XHJOB_DATA_DIR_DATA2', null),
        ],
    ],

    // 全局配置（不属于单个实例）
    'api_token' => env('XHJOB_API_TOKEN', null),
    'pool_mode' => env('XHJOB_POOL_MODE', 'async'),
];
```

#### 自动识别规则

- 配置里有 `instances` 字段（且是数组、非空）→ 多实例模式
- 否则 → 单实例模式，把顶层 `service_name` / `data_dir` 视为 `'default'` 实例的配置

### MODIFIED: 控制器辅助方法支持实例选择

**`releases/xhjob-thinkphp8-extend/controller/XhjobTask.php`** 和 **`tp/app/controller/XhjobTask.php`**：

`xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 四个 protected 方法新增可选参数 `?string $instance = null`：

```php
/**
 * 解析默认实例名。
 * 优先 config('xhjob.default')；未配置时：
 *   - 多实例模式：取 instances 第一个 key
 *   - 单实例模式：返回 'default'
 */
protected function xhjobDefaultInstance(): string
{
    $default = $this->app->config->get('xhjob.default');
    if (is_string($default) && $default !== '') {
        return $default;
    }
    $instances = $this->app->config->get('xhjob.instances');
    if (is_array($instances) && !empty($instances)) {
        return (string) array_key_first($instances);
    }
    return 'default';
}

/**
 * 从 config('xhjob.instances.{instance}.service_name') 读取服务名。
 * 单实例模式回退到 config('xhjob.service_name')。
 * $instance 为 null 时用默认实例。
 */
protected function xhjobServiceName(?string $instance = null): string
{
    $instance = $instance ?? $this->xhjobDefaultInstance();
    // 多实例模式
    $name = $this->app->config->get("xhjob.instances.{$instance}.service_name");
    // 单实例模式回退
    if ($name === null) {
        $name = $this->app->config->get('xhjob.service_name', 'default');
    }
    return is_string($name) && $name !== '' ? $name : 'default';
}

/**
 * 从 config('xhjob.instances.{instance}.data_dir') 读取数据目录。
 * 单实例模式回退到 config('xhjob.data_dir')。
 * 都未配置时回退到 runtime_path() . 'xhjob' . ($instance !== 'default' ? '/' . $instance : '')
 * —— 多实例时按实例名分子目录，避免冲突。
 */
protected function xhjobDataDir(?string $instance = null): ?string
{
    $instance = $instance ?? $this->xhjobDefaultInstance();
    // 多实例模式
    $dir = $this->app->config->get("xhjob.instances.{$instance}.data_dir");
    // 单实例模式回退
    if ($dir === null) {
        $dir = $this->app->config->get('xhjob.data_dir');
    }
    if (is_string($dir) && $dir !== '') {
        return $dir;
    }
    // 回退到 runtime_path/xhjob/{instance}（非 default 实例按实例名分子目录）
    $base = runtime_path() . 'xhjob';
    return $instance !== 'default' ? $base . DIRECTORY_SEPARATOR . $instance : $base;
}

protected function xhjobService(?string $instance = null): XhjobService
{
    return new XhjobService(
        $this->xhjobServiceName($instance),
        $this->xhjobDataDir($instance)
    );
}

protected function xhjobManager(?string $instance = null): TaskManager
{
    return new TaskManager(
        $this->xhjobServiceName($instance),
        $this->xhjobDataDir($instance)
    );
}
```

### MODIFIED: 控制器方法支持 `?instance=` 查询参数

控制器所有方法（`start` / `stop` / `restart` / `status` / `health` / `list` / `get` / `state` / `result` / `logs` / `create*` / `update` / `pause` / `resume` / `reschedule` / `delete` / `chainState` / `groupState` / `demo`）增加可选的 `?instance=data1` 查询参数：

```php
public function start()
{
    $instance = $this->request->param('instance'); // 可选，null 走默认实例
    $svc = $this->xhjobService($instance);
    $pid = $svc->start();
    return $this->json(['pid' => $pid, 'started' => true]);
}
```

所有方法的 `$this->xhjobService()` / `$this->xhjobManager()` 调用都改为 `$this->xhjobService($instance)` / `$this->xhjobManager($instance)`，并在方法开头读取 `$instance = $this->request->param('instance');`。

### MODIFIED: ServiceProvider 容器绑定支持多实例

**`releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php`**：

`xhjob.service` / `xhjob.manager` 保持绑定默认实例（向后兼容）。

新增按需绑定：`xhjob.service.{instance}` / `xhjob.manager.{instance}` —— 但由于 ThinkPHP 容器绑定是懒注册的，无法预知所有实例名，改为在 `resolveConfig` 增加 `$instance` 参数，并通过容器 `bind` 的闭包按需解析：

```php
public function boot()
{
    // 默认实例（向后兼容）
    $this->app->bind('xhjob.service', function (\think\App $app) {
        [$name, $dataDir] = self::resolveConfig($app);
        return new XhjobService($name, $dataDir);
    });
    $this->app->bind('xhjob.manager', function (\think\App $app) {
        [$name, $dataDir] = self::resolveConfig($app);
        return new TaskManager($name, $dataDir);
    });
}

/**
 * 解析 xhjob 配置：service_name 与 data_dir
 *
 * 支持多实例配置：
 *   - config('xhjob.instances.{instance}.service_name') / .data_dir
 *   - 单实例模式回退到 config('xhjob.service_name') / .data_dir
 *   - data_dir 都未配置时回退到 runtime_path/xhjob[/{instance}]
 *
 * @param \think\App $app
 * @param string|null $instance 实例名，null 用默认实例
 * @return array{0:string,1:string} [$name, $dataDir]
 */
private static function resolveConfig(\think\App $app, ?string $instance = null): array
{
    // 默认实例解析
    if ($instance === null) {
        $instance = self::resolveDefaultInstance($app);
    }

    // 多实例模式：instances.{instance}.service_name
    $name = $app->config->get("xhjob.instances.{$instance}.service_name");
    // 单实例模式回退
    if ($name === null) {
        $name = $app->config->get('xhjob.service_name', 'default');
    }
    $name = is_string($name) && $name !== '' ? $name : 'default';

    // 多实例模式：instances.{instance}.data_dir
    $dataDir = $app->config->get("xhjob.instances.{$instance}.data_dir");
    // 单实例模式回退
    if ($dataDir === null) {
        $dataDir = $app->config->get('xhjob.data_dir');
    }
    if (!is_string($dataDir) || $dataDir === '') {
        // 回退到 runtime_path/xhjob[/{instance}]
        $base = $app->getRuntimePath() . 'xhjob';
        $dataDir = $instance !== 'default' ? $base . DIRECTORY_SEPARATOR . $instance : $base;
    }

    return [$name, $dataDir];
}

private static function resolveDefaultInstance(\think\App $app): string
{
    $default = $app->config->get('xhjob.default');
    if (is_string($default) && $default !== '') {
        return $default;
    }
    $instances = $app->config->get('xhjob.instances');
    if (is_array($instances) && !empty($instances)) {
        return (string) array_key_first($instances);
    }
    return 'default';
}
```

> 注意：`xhjob.service.{instance}` / `xhjob.manager.{instance}` 的按需绑定不在 boot 时预注册——用户如需通过容器获取指定实例，可直接 `new XhjobService(...)` 传实例参数，或调用控制器的 `xhjobService('data1')`。本 spec 不引入复杂的容器按需绑定逻辑，避免过度设计。

## Impact

- **Affected specs**：`unify-controller-service-config`（前序，已合并）。本 spec 在其基础上扩展，不破坏前序改动。
- **Affected code**：
  - `releases/xhjob-thinkphp8-extend/config/xhjob.php`：增加多实例配置示例（注释中说明，默认仍为单实例模式）
  - `tp/config/xhjob.php`：同步
  - `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php`：4 个 protected 辅助方法增加 `?string $instance = null` 参数，新增 `xhjobDefaultInstance()`；所有 public 方法增加 `?instance=` 查询参数读取
  - `tp/app/controller/XhjobTask.php`：同步
  - `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php`：`resolveConfig` 增加 `$instance` 参数，新增 `resolveDefaultInstance` 私有方法
- **BREAKING**：无。所有改动都向后兼容：
  - 老配置（单实例模式）继续工作
  - 老控制器调用 `$this->xhjobService()`（不传 instance）继续工作，走默认实例
  - ServiceProvider 的 `xhjob.service` / `xhjob.manager` 容器绑定行为不变

## ADDED Requirements

### Requirement: 多实例配置结构
`config/xhjob.php` SHALL 支持两种配置模式：单实例模式（顶层 `service_name` / `data_dir`，向后兼容）和多实例模式（`default` + `instances` 字典）。系统 SHALL 通过是否存在 `instances` 字段自动识别模式。

#### Scenario: 单实例模式（老配置）
- **GIVEN** `config/xhjob.php` 仅含顶层 `service_name` / `data_dir`，无 `instances` 字段
- **WHEN** 控制器调用 `$this->xhjobService()`（不传 instance）
- **THEN** 等价于调用 `$this->xhjobService('default')`
- **AND** `xhjobServiceName()` 返回 `config('xhjob.service_name', 'default')`
- **AND** `xhjobDataDir()` 返回 `config('xhjob.data_dir')` 或回退到 `runtime_path() . 'xhjob'`

#### Scenario: 多实例模式（新配置）
- **GIVEN** `config/xhjob.php` 含 `default => 'data1'` 和 `instances => ['data1' => [...], 'data2' => [...]]`
- **WHEN** 控制器调用 `$this->xhjobService()`（不传 instance）
- **THEN** 等价于调用 `$this->xhjobService('data1')`（用 `default` 字段指定的实例）
- **AND** `xhjobServiceName()` 返回 `config('xhjob.instances.data1.service_name')`
- **AND** `xhjobDataDir()` 返回 `config('xhjob.instances.data1.data_dir')` 或回退到 `runtime_path() . 'xhjob/data1'`

#### Scenario: 显式指定实例
- **GIVEN** 多实例模式配置
- **WHEN** 用户调用 `/xhjob/start?instance=data2`
- **THEN** 控制器用 `data2` 实例的 `service_name` + `data_dir` 构造 `XhjobService`
- **AND** 启动的是 `data2` 对应的 daemon（独立于 `data1`）

#### Scenario: 默认实例未配置时取第一个
- **GIVEN** 多实例模式配置但未设 `default` 字段
- **WHEN** 控制器调用 `$this->xhjobService()`（不传 instance）
- **THEN** 用 `instances` 字典的第一个 key 作为默认实例（PHP 数组保持插入顺序）

### Requirement: 控制器方法支持 ?instance= 查询参数
`XhjobTask` 控制器所有 public 方法 SHALL 接受可选的 `?instance=` 查询参数，用于选择 xhjob 实例。未传时使用默认实例。

#### Scenario: 不传 instance
- **GIVEN** 多实例模式配置，`default => 'data1'`
- **WHEN** 用户调用 `/xhjob/start`（无 instance 参数）
- **THEN** 使用 `data1` 实例

#### Scenario: 传 instance
- **GIVEN** 多实例模式配置
- **WHEN** 用户调用 `/xhjob/start?instance=data2`
- **THEN** 使用 `data2` 实例

#### Scenario: 传不存在的 instance
- **GIVEN** 多实例模式配置，`instances` 仅含 `data1` / `data2`
- **WHEN** 用户调用 `/xhjob/start?instance=data3`
- **THEN** `xhjobServiceName('data3')` 回退到单实例模式 `config('xhjob.service_name', 'default')`
- **AND** `xhjobDataDir('data3')` 回退到 `runtime_path() . 'xhjob/data3'`
- **AND** 不会抛异常（按 data3 实例名创建新目录，用户可后续在配置里补充 data3 实例）

### Requirement: data_dir 多实例按实例名分子目录
当 `data_dir` 未显式配置且走 `runtime_path()` 回退时，非 `default` 实例 SHALL 在 `runtime_path/xhjob/` 下按实例名分子目录，避免多实例的 SQLite db / sock / pid 文件互相覆盖。

#### Scenario: 多实例 runtime_path 回退
- **GIVEN** 多实例模式配置，`instances.data1.data_dir` 和 `instances.data2.data_dir` 都未配置
- **WHEN** 控制器分别调用 `$this->xhjobService('data1')` 和 `$this->xhjobService('data2')`
- **THEN** `data1` 的 data_dir = `runtime_path() . 'xhjob/data1'`
- **AND** `data2` 的 data_dir = `runtime_path() . 'xhjob/data2'`
- **AND** 两个实例的 `xhjob.default.db` / `xhjob.default.sock` / `xhjob.default.pid` 位于不同目录，互不干扰

#### Scenario: default 实例不分子目录（向后兼容）
- **GIVEN** 单实例模式或 `default` 实例
- **WHEN** 控制器调用 `$this->xhjobService('default')`
- **THEN** data_dir 回退到 `runtime_path() . 'xhjob'`（无子目录，与 `unify-controller-service-config` 行为一致）

### Requirement: ServiceProvider 默认实例解析
`ServiceProvider` 的 `xhjob.service` / `xhjob.manager` 容器绑定 SHALL 通过 `resolveDefaultInstance()` 解析默认实例名，与控制器的 `xhjobDefaultInstance()` 逻辑一致。

#### Scenario: Facade 与控制器一致
- **GIVEN** 多实例模式配置，`default => 'data1'`
- **WHEN** 控制器调用 `$this->xhjobManager()`（不传 instance）
- **AND** 同时通过 `\Xhjob\facade\Xhjob::create(...)` 调用
- **THEN** 两者都使用 `data1` 实例的 `service_name` + `data_dir`
- **AND** Facade 创建的任务能被控制器的 `list()` 方法看到

## MODIFIED Requirements

### Requirement: 控制器辅助方法签名
`xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 四个 protected 方法 SHALL 接受可选参数 `?string $instance = null`。`$instance` 为 `null` 时使用默认实例（通过 `xhjobDefaultInstance()` 解析）。

### Requirement: ServiceProvider resolveConfig 签名
`ServiceProvider::resolveConfig()` SHALL 接受可选参数 `?string $instance = null`，并新增 `resolveDefaultInstance()` 私有静态方法用于解析默认实例名。

## REMOVED Requirements

无。所有改动都向后兼容。
