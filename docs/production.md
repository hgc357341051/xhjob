---
title: 生产实战
nav_order: 50
has_children: true
permalink: /production/
---

# 生产实战

生产环境端到端示例，含完整可运行代码与注意事项。

- [后台任务队列](prod-background-queue/) — FPM 派发 + CLI worker 轮询 + 限流 + 重试
- [定时任务](prod-cron/) — cron + 时区 + 持久化 + 崩溃恢复 + 节假日跳过
- [CLI 与 FPM 共用服务连接](prod-cli-fpm-share/) — 同一 daemon 多端接入 + 路径解析 + systemd
- [ThinkPHP 8 集成](prod-thinkphp8/) — composer 安装 + ServiceProvider + Facade + 25 路由
