# Checklist

## 构建环境
- [ ] `cargo build --release --features persist` 编译成功，生成 `target/release/libxhjob.so`
- [ ] `composer install`（tp/ 目录）成功，`tp/vendor/autoload.php` 存在
- [ ] 集成包已复制到 `tp/extend/Xhjob/`，命名空间可加载
- [ ] `php -d extension=<xhjob.so>` 可加载 TaskManager/TaskBuilder/XhjobService

## 全功能正确性测试覆盖（27 个 xhjob_* 函数）
- [ ] daemon 生命周期：xhjob_start / xhjob_stop / xhjob_restart / xhjob_status 全部 PASS
- [ ] 任务派发与查询：xhjob_dispatch / xhjob_state / xhjob_result / xhjob_get / xhjob_list 全部 PASS
- [ ] 任务控制：xhjob_remove / xhjob_pause / xhjob_resume / xhjob_cancel / xhjob_requeue 全部 PASS
- [ ] 任务修改：xhjob_reschedule / xhjob_modify 全部 PASS
- [ ] 事件与进度：xhjob_events / xhjob_pull_events / xhjob_report_progress 全部 PASS
- [ ] 运行时检查：xhjob_inspect（active/registered/scheduled/stats 全模式）全部 PASS
- [ ] 任务编排：xhjob_chain / xhjob_chain_state / xhjob_group / xhjob_group_state / xhjob_chord / xhjob_chord_state 全部 PASS

## 测试通过率
- [ ] `tp/test_xhjob_all_functions.php` 全部 step PASS（100% 通过率）
- [ ] 测试为真实使用（非 mock/stub/伪代码），每个函数经真实 daemon IPC 调用
- [ ] 运行期间无 daemon crash / 无 PHP fatal error

## .so 编译与版本管理
- [ ] `cargo build --release --features persist` 编译最新 .so
- [ ] `releases/xhjob-php8.2-linux-x86_64.so` 已用最新 .so 覆盖更新
- [ ] `releases/xhjob-php8.2-linux-x86_64.so` 可被 `php -d extension=` 加载

## 提交与推送远程 main
- [ ] 已切换到 main 分支
- [ ] trae/agent-lIaMM4 的修改已合并/快进到 main
- [ ] 全功能测试脚本 `tp/test_xhjob_all_functions.php` 已暂存
- [ ] 更新后的 `releases/xhjob-php8.2-linux-x86_64.so` 已暂存
- [ ] 已提交（git commit）
- [ ] 已推送到 `origin/main`（git push origin main）
- [ ] `git log origin/main` 确认提交存在
- [ ] `git status` 确认工作树干净、与 origin/main 同步

## 最终确认
- [ ] 所有代码审查通过
- [ ] 所有功能正确性测试通过率 100%
- [ ] .so 已编译并提交
- [ ] 所有修改已推送到远程 main 分支
