# Tasks

- [ ] Task 1: 重建构建环境
  - [ ] SubTask 1.1: 执行 `cargo build --release --features persist` 重新编译 .so，验证产物 `target/release/libxhjob.so` 生成
  - [ ] SubTask 1.2: 执行 `composer install`（在 tp/ 目录）恢复 tp 依赖
  - [ ] SubTask 1.3: 复制 `releases/xhjob-thinkphp8-extend/Xhjob/` 到 `tp/extend/Xhjob/`
  - [ ] SubTask 1.4: 验证集成包可加载（php -d extension=... 加载 TaskManager/TaskBuilder/XhjobService）

- [ ] Task 2: 全功能正确性测试编写（`tp/test_xhjob_all_functions.php`）
  - [ ] SubTask 2.1: 编写 CLI 测试入口（step 框架、独立服务名 `tp-allfn`、数据目录 `/tmp/xhjob-tp-allfn`、daemon 启动/清理）
  - [ ] SubTask 2.2: 覆盖 daemon 生命周期函数：xhjob_start / xhjob_stop / xhjob_restart / xhjob_status（真实启停验证）
  - [ ] SubTask 2.3: 覆盖任务派发与查询函数：xhjob_dispatch / xhjob_state / xhjob_result / xhjob_get / xhjob_list（shell + http 真实任务）
  - [ ] SubTask 2.4: 覆盖任务控制函数：xhjob_remove / xhjob_pause / xhjob_resume / xhjob_cancel / xhjob_requeue（cron 任务真实控制）
  - [ ] SubTask 2.5: 覆盖任务修改函数：xhjob_reschedule / xhjob_modify（修改 cron 表达式与字段）
  - [ ] SubTask 2.6: 覆盖事件与进度函数：xhjob_events / xhjob_pull_events / xhjob_report_progress（真实事件流 + 进度上报）
  - [ ] SubTask 2.7: 覆盖运行时检查函数：xhjob_inspect（active/registered/scheduled/stats 全模式真实查询）
  - [ ] SubTask 2.8: 覆盖任务编排函数：xhjob_chain / xhjob_chain_state / xhjob_group / xhjob_group_state / xhjob_chord / xhjob_chord_state（真实编排 + 状态查询）

- [ ] Task 3: 运行全功能测试并修复至 100% 通过
  - [ ] SubTask 3.1: 运行 `php -d extension=<xhjob.so> tp/test_xhjob_all_functions.php`
  - [ ] SubTask 3.2: 修复测试中发现的失败用例（测试代码 bug 或扩展 bug）
  - [ ] SubTask 3.3: 重复运行直至全部 PASS（100% 通过率）

- [ ] Task 4: 编译 .so 并更新 releases 预构建产物
  - [ ] SubTask 4.1: 执行 `cargo build --release --features persist` 编译最新 .so
  - [ ] SubTask 4.2: 将 `target/release/libxhjob.so` 复制为 `releases/xhjob-php8.2-linux-x86_64.so`（覆盖旧产物）
  - [ ] SubTask 4.3: 验证 `releases/xhjob-php8.2-linux-x86_64.so` 可被 `php -d extension=` 加载

- [ ] Task 5: 提交并推送到远程 main 分支
  - [ ] SubTask 5.1: 切换到 main 分支（`git checkout main`）
  - [ ] SubTask 5.2: 将 trae/agent-lIaMM4 分支的修改合并/快进到 main（`git merge trae/agent-lIaMM4 --ff-only` 或等价）
  - [ ] SubTask 5.3: 暂存全功能测试脚本与更新后的 .so（`git add tp/test_xhjob_all_functions.php releases/xhjob-php8.2-linux-x86_64.so`）
  - [ ] SubTask 5.4: 提交（`git commit`，描述全功能测试 + .so 更新）
  - [ ] SubTask 5.5: 推送到远程 main（`git push origin main`）
  - [ ] SubTask 5.6: 验证 `git log origin/main` 与 `git status` 确认推送成功

# Task Dependencies
- Task 2 依赖 Task 1（环境就绪才能编写运行测试）
- Task 3 依赖 Task 2（测试编写完成才能运行）
- Task 4 依赖 Task 3（测试全过后再编译最终 .so，确保 .so 质量已验证）
- Task 5 依赖 Task 4（.so 就绪 + 测试通过才能提交推送）
