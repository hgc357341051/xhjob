# Checklist

## 环境基线
- [ ] 已确认当前 HEAD（7a71a59）与工作树状态
- [ ] 已确认 Rust 代码量（~15410 行 / 37 文件）与 PHP 代码量（~10719 行）
- [ ] 已确认 target/release/libxhjob.so 已构建
- [ ] 已确认 tp/extend/Xhjob/ 集成包已安装且命名空间可解析

## 新一轮全代码深度审查
### Rust 内核
- [ ] 组 A：daemon/ + ipc/ + pool/ 逐函数审查完成
- [ ] 组 B：executor/ + scheduler/ 逐函数审查完成
- [ ] 组 C：store/ + crypto/ + task/ 逐函数审查完成
- [ ] 组 D：outcome/ + retry/ + service/ + utils/ + config/ + errors/ + lib.rs + daemon_main.rs 逐函数审查完成
- [ ] 审查结果已汇总，真实 bug / 误报 / 不确定项已分类
- [ ] 不确定项已询问用户，未瞎猜

### PHP 集成包与 tp 项目
- [ ] releases/xhjob-thinkphp8-extend/Xhjob/ 全部类逐方法审查完成
- [ ] tp/app/controller/ + 配置 + 路由审查完成
- [ ] 现有测试脚本覆盖盲区已识别
- [ ] PHP 层问题已汇总分类

## 生产业务模拟测试套件（tp/xhjob_production_business.php）
- [ ] 测试框架实现（启动 daemon → 场景 → 清理）
- [ ] 场景1：订单异步处理 PASS
- [ ] 场景2：库存扣减 chain + 回滚 PASS
- [ ] 场景3：消息批量推送 group PASS
- [ ] 场景4：定时报表生成 cron + 文件验证 PASS
- [ ] 场景5：定时对账 interval PASS
- [ ] 场景6：失败补偿指数退避 PASS
- [ ] 场景7：软超时熔断 PASS
- [ ] 场景8：限流保护 PASS
- [ ] 场景9：幂等去重（replace_existing）PASS
- [ ] 场景10：崩溃恢复（acks_late + daemon 重启）PASS
- [ ] 全部场景使用真实 IO（非 mock/伪代码）
- [ ] 测试结束后清理临时文件与 daemon

## Bug 复现套件（tp/xhjob_bug_repro_suite.php）
- [ ] 套件框架实现（按 bug id 分 case）
- [ ] 上一轮 11 个 bug 回归 case：
  - [ ] crypto 无效密钥告警而非静默明文
  - [ ] kill 截断防护（pid=0 / pid>i32::MAX 拒绝）
  - [ ] RateLimiter 终态 forget 防泄漏
  - [ ] load_result 失败保留真实 dispatch 结果
  - [ ] shell drain 超时不误标成功任务失败
  - [ ] service_name 路径注入防护
  - [ ] outcome query_state/query_result 校验 service_name
  - [ ] modify_job 清空 trigger 字段（None→NULL）
  - [ ] 未知状态不复活为 Pending（→Failed 终态）
  - [ ] 溢出安全（saturating_add）
  - [ ] 未知 task_type 落 Shell + warn
- [ ] 本轮新发现 bug 的复现 case（先复现 bug 行为，再验证修复后正确）
- [ ] 全部 case PASS

## 本轮新 bug 修复
- [ ] 每个确认的真实 bug 已最小化针对性修复
- [ ] 误报已明确排除并记录原因
- [ ] 不确定项已询问用户后处理
- [ ] 修复未引入新 warning / 新测试失败

## 四维度 100% 通过率
### 正确率
- [ ] `cargo test --features persist` 100% 通过
- [ ] 跨进程 client_test 100% PASS
- [ ] 生产业务模拟测试 100% PASS（10 场景）
- [ ] bug 复现套件 100% PASS

### 代码质量
- [ ] `cargo build --release --features persist` 0 error 0 warning
- [ ] `cargo clippy --features persist` 无新增警告
- [ ] PHP 代码风格一致
- [ ] PHP 公开方法 PHPDoc 完整

### 功能完善度
- [ ] 全部 25 个 `xhjob_*` 函数有测试覆盖
- [ ] TaskBuilder 全部链式方法有测试覆盖
- [ ] XhjobService 生命周期方法有测试覆盖
- [ ] 编排/重试/限流/重叠/持久化/崩溃恢复有测试覆盖

### bug 率与错误率
- [ ] 审查发现的 bug 全部修复
- [ ] 运行时无 panic
- [ ] PHP 端无未捕获异常
- [ ] IPC 无永久阻塞
- [ ] daemon 启动/停止/重启流程无错误
- [ ] 全部测试运行期间无 daemon crash

## 编译并本地提交
- [ ] `cargo build --release --features persist` 产出 `target/release/libxhjob.so`
- [ ] 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- [ ] `git add` 包含所有修改代码 + `.so` + 新增文件
- [ ] `git commit` 创建提交到本地 main
- [ ] 记录最终 commit hash（push 待用户提供凭据）
