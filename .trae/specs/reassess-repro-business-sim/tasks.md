# Tasks

- [ ] Task 1: 二次审查全部 Rust 源文件（37 个），聚焦修复后新问题与遗漏边界
  - [ ] SubTask 1.1: 审查上一轮修改的 11 个文件（crypto/service/outcome/queue/retry/task/in_memory/sqlite/shell/daemon/store）确认修复正确无副作用
  - [ ] SubTask 1.2: 审查未修改的 26 个文件（lib/ipc/pool/executor/scheduler 其余/config/errors/utils/daemon_main）找新问题
  - [ ] SubTask 1.3: 汇总新发现问题清单（编号 + 文件 + 触发条件 + 严重度），不瞎猜，存疑先记

- [ ] Task 2: 审查 PHP 集成包 + tp 项目代码
  - [ ] SubTask 2.1: 审查 `releases/xhjob-thinkphp8-extend/Xhjob/` 全部类（TaskBuilder/TaskManager/XhjobService/Client/ServiceProvider/facade/helper/Exception）
  - [ ] SubTask 2.2: 审查 `tp/app/` + `tp/config/` + 现有测试脚本
  - [ ] SubTask 2.3: 汇总新发现问题清单

- [ ] Task 3: 编写电商订单全链路业务模拟脚本
  - [ ] SubTask 3.1: `tp/business/order_pipeline.php` —— 下单(Shell 生成订单文件) → 支付回调(HTTP) → 发货(chain 三步流水线) → 通知(group 并行 邮件/短信/推送) → 对账(chord header 汇总 + callback 生成报表)
  - [ ] SubTask 3.2: 串联 cron 夜间结算 / retry 支付失败重试 / rateLimit 通知限流 / maxInstances 防重复发货 / encoding GBK 发票
  - [ ] SubTask 3.3: 启动独立 daemon，运行业务模拟，全链路断言通过

- [ ] Task 4: 对新发现问题逐个写复现脚本并修复
  - [ ] SubTask 4.1: 每个新问题写 `tp/repro/bug_<编号>_<简述>.php`，运行确认修复前 FAIL
  - [ ] SubTask 4.2: 修复（Rust 侧同时补 cargo test 回归用例；PHP 侧在集成包或 tp 修复）
  - [ ] SubTask 4.3: 修复后重跑复现脚本确认 PASS

- [ ] Task 5: 对上一轮 11 个已修复 bug 补写回归复现脚本
  - [ ] SubTask 5.1: `tp/repro/bug_001_crypto_key_cache.php` ~ `bug_011_overflow_safety.php`
  - [ ] SubTask 5.2: 在当前已修复代码上运行 11 个脚本均 PASS（证明未回归）

- [ ] Task 6: 五维度评估 + 编译提交
  - [ ] SubTask 6.1: `cargo build --release --features persist` 0 warning + `cargo test --features persist` 100% 通过
  - [ ] SubTask 6.2: 运行业务模拟 + 全部 repro 脚本 100% PASS
  - [ ] SubTask 6.3: 五维度 checklist 逐项确认 100%
  - [ ] SubTask 6.4: 复制 .so 到 `releases/xhjob-php8.2-linux-x86_64.so`，`git add` + `git commit` + `git push origin main`

# Task Dependencies
- Task 1、Task 2 可并行（不同代码层）
- Task 3 依赖 Task 1/2 审查无阻塞性 bug（业务模拟基于稳定内核）
- Task 4 依赖 Task 1/2（发现问题后复现修复）
- Task 5 独立，可与 Task 3/4 并行
- Task 6 依赖 Task 3/4/5 全部完成
