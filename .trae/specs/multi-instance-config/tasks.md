# Tasks

- [x] Task 1: 改造控制器辅助方法支持多实例
  - [x] SubTask 1.1: 在 `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 新增 `xhjobDefaultInstance()` 方法
  - [x] SubTask 1.2: 修改 `xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 增加 `?string $instance = null` 参数
  - [x] SubTask 1.3: `xhjobDataDir()` 多实例回退时按实例名分子目录（`runtime_path/xhjob/{instance}`）
  - [x] SubTask 1.4: 同步到 `tp/app/controller/XhjobTask.php`

- [x] Task 2: 控制器所有 public 方法支持 `?instance=` 查询参数
  - [x] SubTask 2.1: `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 所有 public 方法（25 个）开头读取 `$instance = $this->request->param('instance');`，并把 `$this->xhjobService()` / `$this->xhjobManager()` 改为传 `$instance`
  - [x] SubTask 2.2: 同步到 `tp/app/controller/XhjobTask.php`（25 个方法，`diag()` 不改）

- [x] Task 3: ServiceProvider `resolveConfig` 支持多实例
  - [x] SubTask 3.1: 修改 `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php` 的 `resolveConfig` 增加 `?string $instance = null` 参数
  - [x] SubTask 3.2: 新增 `resolveDefaultInstance()` 私有静态方法
  - [x] SubTask 3.3: `xhjob.service` / `xhjob.manager` 闭包调用 `resolveConfig($app)`（不传 instance，走默认）—— boot() 未修改

- [x] Task 4: 更新 `config/xhjob.php` 注释，示例多实例配置
  - [x] SubTask 4.1: `releases/xhjob-thinkphp8-extend/config/xhjob.php` 顶部注释增加多实例配置示例（默认仍为单实例模式）
  - [x] SubTask 4.2: `tp/config/xhjob.php` 同步

- [x] Task 5: 静态验证
  - [x] SubTask 5.1: `php -l` 语法检查所有修改的 PHP 文件（5 个文件全部通过）
  - [x] SubTask 5.2: `grep` 确认所有控制器 public 方法都读取了 `$instance` 参数（25 处/文件）
  - [x] SubTask 5.3: 验证单实例配置（无 `instances` 字段）仍能正常解析（向后兼容）—— `instances` 仅出现在注释，return 数组仍是顶层 `service_name` / `data_dir`
  - [x] SubTask 5.4: 验证多实例配置（有 `instances` 字段）能正确解析默认实例 + 指定实例 —— 代码审查确认 `xhjobDefaultInstance()` 优先读 `default` 字段，`resolveConfig` 多实例优先读 `instances.{instance}.*`

# Task Dependencies
- Task 2 依赖 Task 1（先有辅助方法支持 instance 才能在 public 方法里传）
- Task 3 独立于 Task 1/2（ServiceProvider 改造与控制器改造不耦合）
- Task 5 依赖 Task 1/2/3/4 全部完成
