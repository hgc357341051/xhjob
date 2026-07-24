# Checklist

## 二次审查
- [ ] 上一轮修改的 11 个 Rust 文件确认修复正确无副作用
- [ ] 未修改的 26 个 Rust 文件审查完成，新问题清单汇总
- [ ] PHP 集成包审查完成，新问题清单汇总
- [ ] tp 项目审查完成，新问题清单汇总

## 正确率 100%
- [ ] 27 个 xhjob_* 函数真实业务调用行为正确（业务模拟覆盖）
- [ ] chain/group/chord/countdown 编排真实业务正确
- [ ] cron/interval/run_at/skip_dates/workdays_only 调度正确
- [ ] retry/backoff/rateLimit/maxInstances/timeout/softTimeout 行为正确
- [ ] HTTP 各状态码 + idempotent 重试正确
- [ ] Shell 非零退出 + encoding 转换正确

## 代码质量 100%
- [ ] `cargo build --release --features persist` 0 error 0 warning
- [ ] `cargo clippy --features persist`（若可用）无新增 warning
- [ ] Rust 代码风格一致、关键路径有注释
- [ ] PHP 集成包全部类 `declare(strict_types=1)` + PHPDoc
- [ ] tp 项目代码风格一致

## 功能完善度 100%
- [ ] 现有/文档承诺功能在真实调用下全可用
- [ ] 文档承诺但实现缺失/失效的明显缺口已补齐
- [ ] 未新增未规划功能（避免范围蔓延）

## bug率 100%
- [ ] 新发现问题全部经复现脚本暴露后修复
- [ ] 上一轮 11 个已修复 bug 回归复现脚本（bug_001~011）全部 PASS
- [ ] 本轮新发现 bug 复现脚本全部修复后 PASS
- [ ] 0 已知未修复 bug

## 错误率 100%
- [ ] Rust 扩展运行时 0 panic
- [ ] PHP 端 0 未捕获异常 / fatal
- [ ] IPC 0 永久阻塞
- [ ] daemon 启停重启 0 错误
- [ ] 错误路径均有处理（非 panic）

## 电商订单全链路业务模拟
- [ ] `tp/business/order_pipeline.php` 全链路断言通过
- [ ] 覆盖 Shell（下单）/ HTTP（支付回调）/ chain（发货）/ group（通知）/ chord（对账）
- [ ] 覆盖 cron / retry / rateLimit / maxInstances / encoding
- [ ] 连接独立 daemon（跨进程），非单进程内嵌

## 复现驱动修复
- [ ] 每个新问题有 `tp/repro/bug_<编号>.php` 复现脚本（修复前 FAIL）
- [ ] 修复后复现脚本 PASS
- [ ] Rust 侧 bug 有 cargo test 回归用例
- [ ] 11 个已修复 bug 回归复现脚本 bug_001~011 全部 PASS

## 编译并提交到远程主分支
- [ ] `cargo build --release --features persist` 产出 `target/release/libxhjob.so`
- [ ] .so 复制到 `releases/xhjob-php8.2-linux-x86_64.so`
- [ ] `git add` 所有修改代码 + .so + 新增文件
- [ ] `git commit` 创建提交
- [ ] `git push origin main` 推送到远程主分支成功
