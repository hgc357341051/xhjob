# Audit Checklist

## 覆盖性检查

- [ ] 审查覆盖全部 27 个 Rust 源文件（daemon/executor/ipc/pool/scheduler/store/task/utils/service/outcome/retry/lib）
- [ ] 每个模块都有独立的审查结论
- [ ] 审查结论引用具体文件:行号

## 优点清单检查

- [ ] 列出值得保留的好功能（如两种池模式、chord、持久化、多服务隔离等）
- [ ] 列出好的设计决策（如 double-fork、work-stealing、EVAL 脚本等）
- [ ] 列出好的工程实践（如 tracing、错误传播、配置覆盖等）
- [ ] 每条优点有文件:行号引用与简短说明

## 缺点清单检查

- [ ] 识别代码异味（过长函数、重复代码、魔法数字等）
- [ ] 识别潜在 bug（unwrap/expect、整数溢出、竞态等）
- [ ] 识别资源泄漏风险（未关闭的句柄、未 join 的线程等）
- [ ] 识别不一致行为（命名、错误返回、JSON 契约等）
- [ ] 每条缺点标注 P0/P1/P2 优先级
- [ ] 每条缺点有建议修复方向

## 待完善功能检查

- [ ] 识别 TODO / FIXME / unimplemented! / todo! 标记
- [ ] 识别 stub 实现（如 events TTL 常量化、list tag 过滤未暴露等）
- [ ] 识别未实现的增强项（如分布式、chord 持久化等）
- [ ] 每条待完善项有改进建议

## 维度专项检查

- [ ] 架构设计：模块边界清晰、依赖方向合理、无循环依赖
- [ ] 并发模型：async/thread 池切换正确、无 block_on 嵌套、锁粒度合理
- [ ] 错误处理：无裸 unwrap/expect 在关键路径、错误传播链完整
- [ ] 资源管理：子进程、socket、文件、SQLite 连接正确释放
- [ ] 安全性：shell 命令注入风险、路径遍历、credential 泄漏
- [ ] 可观测性：tracing 覆盖关键路径、日志级别合理
- [ ] API 设计：PHP 导出函数签名一致、error: 约定统一、向后兼容

## 报告质量检查

- [ ] AUDIT_REPORT.md 存在于 /workspace/AUDIT_REPORT.md
- [ ] 报告结构清晰（优点 / 缺点 / 待完善 / 模块结论 / 优先级建议）
- [ ] 报告中所有引用可点击或可定位到文件:行号
- [ ] 报告末尾有改进建议优先级汇总表（P0/P1/P2 数量与条目）
