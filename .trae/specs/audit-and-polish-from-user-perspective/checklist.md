# Checklist

## 起点（拉取与核对）
- [x] `git fetch origin` 成功，无网络错误
- [x] 本地 `main` 分支与 `origin/main` 同步（`git status` 显示 up to date）
- [x] 工作树 clean，无未提交改动
- [x] 最近 5 个 commit 符合预期（data_dir 功能 + 预编译 .so + Windows 放弃记录 + import 修复 + 规划文档）

## 编译验证（零警告）
- [x] `cargo build --release --features persist` 退出码 0 且无 warning
- [x] `cargo build --release`（默认 feature）退出码 0 且无 warning（修复 daemon_main.rs 的 data_dir unused 警告）
- [x] `cargo test --release --features persist --lib` 全部通过（43/43）
- [x] `cargo test --release --lib`（默认 feature）全部通过（43/43）

## 扩展部署
- [x] `libxhjob.so` 已复制到 `php-config --extension-dir`
- [x] `php -d extension=xhjob.so -m` 输出包含 `xhjob`
- [x] `function_exists('xhjob_start')` 返回 true
- [x] `class_exists('Xhjob')` 返回 true

## PHP API 审查（src/lib.rs）
- [x] README 顶层函数签名表与 `#[php_function]` 实际签名一致（ReflectionFunction 反射验证）
- [x] README `Xhjob` 类方法表与 `#[php_impl]` 实际方法一致（ReflectionClass 反射验证，snake→camel 转换正确）
- [x] `xhjob_start` / `stop` / `restart` / `status` / `dispatch` / `state` / `result` / `run_daemon` 均接受可选 `data_dir` 参数
- [x] `Xhjob::dataDir()` 链式方法存在且返回 `&mut self`
- [x] 失败返回路径一致：start/stop/restart 返回 bool，dispatch 返回 `error: <msg>` 字符串（已统一前缀），state/result 返回 array
- [x] 向后兼容：不传 data_dir 时行为与旧版一致（回退到 env var 与平台默认）

## examples 审查
- [x] `examples/chain_api.php` 实际执行 PASS（shell + HTTP 任务）
- [x] `examples/cron_http.php` 实际执行 PASS（修复后增加 dispatch 错误检查）
- [x] `examples/encoding.php` 实际执行 PASS（GBK + auto 编码转换）
- [x] `examples/multi_service.php` 实际执行 PASS（修复 Xhjob::service() 静态调用 fatal error）
- [x] `examples/overlap.php` 实际执行 PASS（allowOverlap 控制验证）
- [x] `examples/proxy.php` 实际执行 PASS（无代理环境下任务 FAILED 但脚本不 fatal，轮询超时改为 35s）
- [x] `examples/timezone.php` 实际执行 PASS（多时区 cron + 无效时区 dispatch 返回 error:）
- [x] 不依赖外部网络的示例实际执行无 fatal error

## .phpt 测试
- [x] `tests/start_stop.phpt` PASS
- [x] `tests/dispatch_shell.phpt` PASS
- [x] `tests/cron.phpt` PASS
- [x] `tests/retry.phpt` PASS
- [x] `tests/overlap.phpt` PASS
- [x] `tests/persist.phpt` PASS
- [x] `tests/multi_service.phpt` PASS
- [x] `tests/dispatch_http.phpt` SKIP 原因为无网络（非失败）
- [x] `tests/encoding.phpt` SKIP 原因为 Windows-only（非失败）
- [x] `tests/proxy.phpt` SKIP 原因为无 XHJOB_TEST_PROXY env（非失败）

## business 业务场景测试
- [x] `cli_bus/run_all.sh` 4 步全部 PASS（start/operate/restart/stop）
- [x] `fpm_sim/proc_test.php` 10 步全部 PASS
- [x] `fpm_sim/client_test.php` 10 步全部 PASS

## data_dir 专项测试
- [x] `tests/data_dir_smoke.php` PASS
- [x] pid/sock/db/log 全部生成在用户指定目录
- [x] task 成功执行（state=SUCCESS，stdout 匹配）
- [x] stop 后 db 文件保留在指定目录

## 功能模块独立验证（用代码验证执行结果）
- [x] shell 任务：stdout 与 exit_code 正确
- [x] retry 任务：重试 3 次后 FAILED，attempts=4
- [x] cron 任务：每分钟任务至少触发一次（tests/cron.phpt）
- [x] overlap 任务：慢任务 + allowOverlap(false) 第二次触发排队后执行
- [x] persist 任务：restart 后任务状态可查
- [x] 多服务：两个服务 PID 不同，stop 一个不影响另一个

## 修复与回归
- [x] 审查发现的所有真实问题已记录（9 个问题，按 CRITICAL/HIGH/MEDIUM/LOW 分级）
- [x] 每个问题已逐个修复（无过度工程，每个改动 1-10 行）
- [x] 修复后重新编译零警告（两种 feature 均通过）
- [x] 修复后重新执行受影响测试无回归（cargo 43/43、.phpt 7/7、cli_bus 4/4、fpm_sim 20/20、functional_verify 15/15、7 个 examples 全部可运行）

## 提交与推送
- [x] `git status` 核对修改文件清单
- [x] `git diff` 审查改动内容
- [x] `git add <指定文件>` 暂存改动
- [x] `git commit -m "..."` 提交到本地 main
- [x] `git push origin main` 推送到远程主分支（用户在本地执行）
- [x] 推送后 `git log origin/main` 确认远程 HEAD 已更新（`git status` 显示 `Your branch is up to date with 'origin/main'`，本地 `698f794` = 远程 `origin/main`）
