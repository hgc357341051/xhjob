# Checklist

## 控制器统一服务配置
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 新增 `xhjobServiceName()` / `xhjobDataDir()` / `xhjobService()` / `xhjobManager()` 四个 protected 方法
- [x] `tp/app/controller/XhjobTask.php` 同步新增上述四个 protected 方法
- [x] `xhjobServiceName()` 从 `config('xhjob.service_name', 'default')` 读取，null 兜底为 `'default'`
- [x] `xhjobDataDir()` 优先读 `config('xhjob.data_dir')`，为 null/空字符串时回退到 `runtime_path() . 'xhjob'`
- [x] `xhjobService()` / `xhjobManager()` 用上述两个方法构造实例

## 替换调用点
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 中所有 `new XhjobService()` 已替换为 `$this->xhjobService()`（7 处含 demo 硬编码）
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 中所有 `new TaskManager()` 已替换为 `$this->xhjobManager()`（21 处含 demo 硬编码）
- [x] `tp/app/controller/XhjobTask.php` 中所有 `new XhjobService()` 已替换为 `$this->xhjobService()`（7 处含 demo 硬编码）
- [x] `tp/app/controller/XhjobTask.php` 中所有 `new TaskManager()` 已替换为 `$this->xhjobManager()`（21 处含 demo 硬编码）
- [x] 控制器中已无硬编码 `'tp-demo'` / `'/tmp/xhjob-tp-demo'`（grep 验证：两个文件均无匹配）

## demo() 方法
- [x] `releases/xhjob-thinkphp8-extend/controller/XhjobTask.php` 的 `demo()` 使用 `$this->xhjobService()` / `$this->xhjobManager()`
- [x] `tp/app/controller/XhjobTask.php` 的 `demo()` 使用 `$this->xhjobService()` / `$this->xhjobManager()`
- [x] `demo()` 创建的任务能在随后的 `/xhjob/list` 中看到（同一服务、同一 data_dir）—— 静态验证通过：demo 与其它方法均通过辅助方法读取相同 config

## ServiceProvider 容器绑定
- [x] `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php` 的 `xhjob.service` 绑定增加 `runtime_path()` 回退（通过 `resolveConfig` 私有静态方法）
- [x] `releases/xhjob-thinkphp8-extend/Xhjob/ServiceProvider.php` 的 `xhjob.manager` 绑定增加 `runtime_path()` 回退（同上）
- [x] 若 `tp/extend/Xhjob/ServiceProvider.php` 存在，同步上述改动 —— 确认文件不存在（`/workspace/tp/extend/` 下无任何 PHP 文件），已跳过

## config/xhjob.php 注释
- [x] `releases/xhjob-thinkphp8-extend/config/xhjob.php` 的 `data_dir` 注释说明 runtime_path 回退
- [x] `tp/config/xhjob.php` 的 `data_dir` 注释同步更新
- [x] 默认值仍为 `env('XHJOB_DATA_DIR', null)`（不变）

## 静态验证
- [x] `php -l` 通过所有修改的 PHP 文件（5 个文件：2 个控制器 + 1 个 ServiceProvider + 2 个 config，全部 "No syntax errors detected"）
- [x] `grep "new XhjobService\|new TaskManager" controller/XhjobTask.php` 仅在辅助方法定义处出现（每文件 2 处，位于 `xhjobService()` / `xhjobManager()` 的 return 语句）
- [x] `grep "tp-demo\|xhjob-tp-demo" controller/XhjobTask.php` 无匹配（两个文件均无）
- [x] `runtime_path()` 在 ThinkPHP 8 中可用 —— 控制器使用全局 `runtime_path()` 函数，ServiceProvider 使用 `$app->getRuntimePath()` 方法，两者均为 ThinkPHP 8 标准 API
