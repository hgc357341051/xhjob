# Checklist

## 控制器辅助方法
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 新增 `xhjobDefaultInstance()` protected 方法（第 52 行）
- [x] `tp/app/controller/XhjobTask.php` 同步新增 `xhjobDefaultInstance()`（第 56 行）
- [x] `xhjobDefaultInstance()` 优先读 `config('xhjob.default')`，未配置时取 `instances` 第一个 key，再回退 `'default'`
- [x] `xhjobServiceName(?string $instance = null)` 支持多实例：先读 `config("xhjob.instances.{$instance}.service_name")`，回退到 `config('xhjob.service_name', 'default')`（第 74/78 行）
- [x] `xhjobDataDir(?string $instance = null)` 支持多实例：先读 `config("xhjob.instances.{$instance}.data_dir")`，回退到 `config('xhjob.data_dir')`，再回退到 `runtime_path() . 'xhjob'`（default 实例）或 `runtime_path() . 'xhjob/' . $instance`（非 default 实例）（第 96/100 行）
- [x] `xhjobService(?string $instance = null)` / `xhjobManager(?string $instance = null)` 透传 `$instance` 给 `xhjobServiceName` / `xhjobDataDir`（第 119/123、133/137 行）

## 控制器 public 方法
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 所有 25 个 public 方法（index/start/stop/restart/status/health/list/get/state/result/logs/create/createShell/createHttp/createCron/createChain/createGroup/update/pause/resume/reschedule/delete/chainState/groupState/demo）开头读取 `$instance = $this->request->param('instance');`
- [x] 上述所有方法内的 `$this->xhjobService()` / `$this->xhjobManager()` 调用都传入 `$instance`（grep 验证无残留无参调用）
- [x] `tp/app/controller/XhjobTask.php` 同步上述改动（25 个方法，`diag()` 保持原样未改）
- [x] tp 版本 `$instance` 一律置于 try 块之前（方法体第一行），try 块内复用同一变量

## ServiceProvider
- [x] `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php` 的 `resolveConfig` 增加 `?string $instance = null` 参数（第 64 行）
- [x] 新增 `resolveDefaultInstance(\think\App $app): string` 私有静态方法（第 104 行）
- [x] `resolveConfig` 内 `$instance === null` 时调用 `resolveDefaultInstance($app)` 解析默认实例
- [x] `resolveConfig` 多实例模式：先读 `config("xhjob.instances.{$instance}.service_name")` / `.data_dir`，回退到单实例顶层字段
- [x] `resolveConfig` data_dir 回退时，非 default 实例走 `runtime_path/xhjob/{instance}`，default 实例走 `runtime_path/xhjob`
- [x] `xhjob.service` / `xhjob.manager` 闭包调用 `resolveConfig($app)`（不传 instance，走默认）—— `boot()` 方法未修改

## config/xhjob.php
- [x] `releases/xhjob-thinkphp8-extend/config/xhjob.php` 注释中说明两种配置模式，给出多实例配置示例
- [x] `tp/config/xhjob.php` 同步注释
- [x] 默认配置仍为单实例模式（顶层 `service_name` / `data_dir`，`instances` 仅出现在注释中，未进入 return 数组）

## 静态验证
- [x] `php -l` 通过所有修改的 PHP 文件（5 个文件全部 "No syntax errors detected"）
- [x] `grep "request->param('instance')"` 确认所有控制器 public 方法都读取了 `$instance`（25 处/文件）
- [x] `grep "xhjobService\\(\\)|xhjobManager\\(\\)"` 确认无残留无参调用（两个文件均无匹配）
- [x] `grep "function xhjob"` 确认 5 个辅助方法签名正确（xhjobDefaultInstance + 4 个带 `?string $instance = null` 参数的方法）
- [x] 单实例配置（无 `instances` 字段）仍能正常解析（向后兼容）—— `instances` 仅出现在注释，return 数组仍是顶层 `service_name` / `data_dir`
- [x] 多实例配置（有 `instances` 字段）能正确解析默认实例 + 指定实例 —— `xhjobDefaultInstance()` 优先读 `default` 字段，`resolveConfig` 多实例优先读 `instances.{instance}.*`
- [x] 非默认实例的 data_dir 回退路径按实例名分子目录（`runtime_path/xhjob/{instance}`）
