# Tasks

- [x] Task 1: 拉取远程主分支并核对工作树一致性
  - [x] SubTask 1.1: `git fetch origin && git status`，确认本地 `main` 与 `origin/main` 同步
  - [x] SubTask 1.2: `git log --oneline -10` 核对最近提交，确认未回滚任何用户改动
  - [x] SubTask 1.3: 确认工作树 clean，无遗留未提交改动

- [x] Task 2: 编译验证（零警告）
  - [x] SubTask 2.1: `cargo build --release --features persist` 零警告通过
  - [x] SubTask 2.2: `cargo build --release`（无 persist feature）零警告通过（修复 `daemon_main.rs` 的 `data_dir` unused 变量警告）
  - [x] SubTask 2.3: `cargo test --release --features persist --lib` 全部通过（43/43）
  - [x] SubTask 2.4: `cargo test --release --lib`（默认 feature）全部通过（43/43）

- [x] Task 3: 部署扩展到 PHP extensions 目录
  - [x] SubTask 3.1: `cp target/release/libxhjob.so $(php-config --extension-dir)/xhjob.so`
  - [x] SubTask 3.2: `php -d extension=xhjob.so -m | grep xhjob` 确认加载
  - [x] SubTask 3.3: `php -d extension=xhjob.so -r 'var_dump(function_exists("xhjob_start"));'` 确认函数注册

- [x] Task 4: 审查 PHP API（src/lib.rs）使用者友好性
  - [x] SubTask 4.1: 核对 README 函数签名表与 `#[php_function]` 实际签名（参数名、顺序、可选性、返回类型）— ReflectionFunction 反射验证完全一致
  - [x] SubTask 4.2: 核对 `Xhjob` 类方法表与 `#[php_impl]` 实际方法（snake→camel 自动转换与文档一致）
  - [x] SubTask 4.3: 检查错误返回路径：发现 `xhjob_dispatch` 与 `Xhjob::dispatch()` 错误前缀不一致，已统一为 `error: <msg>` 格式
  - [x] SubTask 4.4: 检查向后兼容性：不传 data_dir 时行为与旧版一致

- [x] Task 5: 审查 examples/*.php 可运行性
  - [x] SubTask 5.1: 逐个检查 7 个示例（发现 `multi_service.php` 使用 `Xhjob::service()` 静态调用触发 fatal error）
  - [x] SubTask 5.2: 验证示例中调用的 API 都存在且签名匹配
  - [x] SubTask 5.3: 实际执行 7 个示例：multi_service.php 修复后 PASS，其余 6 个均 PASS

- [x] Task 6: 执行全量 .phpt 测试并核对结果
  - [x] SubTask 6.1: `php run-tests.php tests/` 执行全部 .phpt
  - [x] SubTask 6.2: 核对 PASS/SKIP/FAIL 分布：7 PASS / 3 SKIP / 0 FAIL，SKIP 原因合理
  - [x] SubTask 6.3: 对每个 PASS 的测试确认其断言确实验证了功能正确性（非空跑）

- [x] Task 7: 执行 business 业务场景测试
  - [x] SubTask 7.1: `bash tests/business/cli_bus/run_all.sh` 4 步串联测试全部 PASS
  - [x] SubTask 7.2: `php tests/business/fpm_sim/proc_test.php` 10 步多 worker 测试全部 PASS
  - [x] SubTask 7.3: `php tests/business/fpm_sim/client_test.php` 10 步 HTTP fpm 模拟测试全部 PASS

- [x] Task 8: 执行 data_dir 专项测试
  - [x] SubTask 8.1: `php tests/data_dir_smoke.php` 验证 pid/sock/db/log 全部落入指定目录
  - [x] SubTask 8.2: 验证 task 成功执行且 stop 后 db 文件保留

- [x] Task 9: 单独执行功能模块验证脚本（用代码验证执行结果正确性）
  - [x] SubTask 9.1: shell 任务：dispatch echo 命令，验证 stdout 与 exit_code（PASS）
  - [x] SubTask 9.2: retry 任务：dispatch 必失败命令 + withRetry(3)，验证重试 3 次后 FAILED，attempts=4（PASS）
  - [x] SubTask 9.3: cron 任务：tests/cron.phpt 验证 cron 触发（PASS）
  - [x] SubTask 9.4: overlap 任务：慢任务 + allowOverlap(false)，验证第二次触发排队后执行（PASS）
  - [x] SubTask 9.5: persist 任务：persist(true) + 投递，restart daemon 后验证任务状态可查（PASS）
  - [x] SubTask 9.6: 多服务：同时启动两个服务，验证 PID 不同且互不干扰（PASS）

- [x] Task 10: 修复审查中发现的真实问题
  - [x] SubTask 10.1: 记录所有审查发现的问题清单（9 个问题，按 CRITICAL/HIGH/MEDIUM/LOW 分级）
  - [x] SubTask 10.2: 逐个修复真实问题（不引入新功能，不做过度工程）
    - 问题1（CRITICAL）：README + examples/multi_service.php 修复 `Xhjob::service()` 静态调用 fatal error，改为 `Xhjob::task()->service($name)->...`
    - 问题2（HIGH）：删除 README 重复的旧函数签名表
    - 问题3（HIGH）：`xhjob_dispatch` 错误返回统一为 `error: <msg>` 前缀（src/lib.rs:150-153）
    - 问题4（MEDIUM）：`daemon::start` 快速路径增加 IPC socket 就绪检查，避免竞态（src/daemon/mod.rs:245-254）
    - 问题5（MEDIUM）：`xhjob_result` 失败文案改为准确提示，不再误导用户以为 daemon 挂了（src/lib.rs:222）
    - 问题6（MEDIUM）：README API 参考表补 dispatch 错误返回说明
    - 问题7（LOW）：`examples/cron_http.php` 增加 dispatch 错误检查，避免把错误字符串当 task_id 嵌入给用户的命令
    - 问题8（LOW）：`examples/proxy.php` 轮询次数从 100 提到 350（35s，略大于任务 timeout 30s），并打印超时提示
    - 问题9（LOW）：`examples/multi_service.php` dispatch 失败时清理已启动的 daemon 后退出
  - [x] SubTask 10.3: 修复后重新编译 + 重新执行受影响的测试，确认无回归（cargo 43/43、.phpt 7/7、cli_bus 4/4、fpm_sim 20/20、functional_verify 15/15、data_dir_smoke PASS、7 个 examples 全部可运行）

- [x] Task 11: 提交并推送远程主分支
  - [x] SubTask 11.1: `git status` 核对修改文件清单
  - [x] SubTask 11.2: `git diff` 审查改动内容
  - [x] SubTask 11.3: `git add <指定文件>` 暂存改动
  - [x] SubTask 11.4: `git commit -m "..."` 提交到本地 main
  - [x] SubTask 11.5: `git push origin main` 推送到远程主分支（用户在本地执行，沙箱无凭证）

# Task Dependencies
- Task 1 独立，最先执行（确认起点干净）
- Task 2 依赖 Task 1（确认起点后编译）
- Task 3 依赖 Task 2（编译产物部署）
- Task 4、Task 5 可并行，均依赖 Task 3（需扩展加载后才能验证 API）
- Task 6、Task 7、Task 8、Task 9 可并行，均依赖 Task 3
- Task 10 依赖 Task 4-9（汇总审查发现后修复）
- Task 11 依赖 Task 10（修复完成且无回归后提交）
