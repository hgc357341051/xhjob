---
title: 核心能力
nav_order: 30
has_children: true
permalink: /core-features/
---

# 核心能力

xhjob 的核心调度与可靠性能力。

- [触发器](triggers/) — cron / interval / runAt / or_cron / skip_dates / jitter
- [重试与超时](retry-timeout/) — retry / backoff / hard / soft timeout / expires
- [并发控制](concurrency/) — maxInstances / overlap / coalesce / rateLimit / priority
- [持久化与崩溃恢复](persistence-recovery/) — persist / acksLate / lease / watchdog / PID 复用防护
- [编排](orchestration/) — chain / group / chord
- [进度上报与事件](progress-events/) — reportProgress / pullEvents / inspect
