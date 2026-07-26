# Tasks

- [x] Task 1: 新增控制器 protected 辅助方法（统一服务配置读取）
  - [x] SubTask 1.1: 在 `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 新增 `xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 四个 protected 方法
  - [x] SubTask 1.2: 同步到 `tp/app/controller/XhjobTask.php`

- [x] Task 2: 替换控制器内所有 `new XhjobService()` / `new TaskManager()` 调用
  - [x] SubTask 2.1: `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 全部 30+ 处替换为 `$this->xhjobService()` / `$this->xhjobManager()`
  - [x] SubTask 2.2: `tp/app/controller/XhjobTask.php` 全部 30+ 处替换

- [x] Task 3: 修改 `demo()` 方法放弃硬编码 `tp-demo` 服务
  - [x] SubTask 3.1: `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 的 `demo()` 改用 `$this->xhjobService()` / `$this->xhjobManager()`
  - [x] SubTask 3.2: `tp/app/controller/XhjobTask.php` 的 `demo()` 同步改动

- [x] Task 4: ServiceProvider 容器绑定增加 `runtime_path()` 回退
  - [x] SubTask 4.1: 修改 `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php` 的 `xhjob.service` / `xhjob.manager` 绑定（提取 `resolveConfig` 私有静态方法）
  - [x] SubTask 4.2: 检查 `tp/extend/Xhjob/ServiceProvider.php` 是否存在，存在则同步改动（确认不存在，已跳过）

- [x] Task 5: 更新 `config/xhjob.php` 注释
  - [x] SubTask 5.1: `releases/xhjob-thinkphp8-extend/config/xhjob.php` 的 `data_dir` 注释改为说明 runtime_path 回退
  - [x] SubTask 5.2: `tp/config/xhjob.php` 同步注释更新

- [x] Task 6: 静态验证（语法 + 调用点检查）
  - [x] SubTask 6.1: `php -l` 语法检查所有修改的 PHP 文件（5 个文件全部通过）
  - [x] SubTask 6.2: `grep` 确认控制器中已无 `new XhjobService(` / `new TaskManager(` 直接调用（仅辅助方法定义处保留 2 处/文件）
  - [x] SubTask 6.3: `grep` 确认控制器中已无硬编码 `'tp-demo'` / `'/tmp/xhjob-tp-demo'`（无匹配）

# Task Dependencies
- Task 2 依赖 Task 1（先有辅助方法才能调用）
- Task 3 依赖 Task 1（demo 改用辅助方法）
- Task 6 依赖 Task 2/3/4/5 全部完成
