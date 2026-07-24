# Checklist

## 阶段一：站点骨架与发布
- [x] `docs/_config.yml` 存在且配置了 Just-The-Docs 主题、title、baseurl=`/xhjob`、permalink
- [x] `docs/index.md` 存在，含项目一句话介绍、核心特性列表、快速开始入口
- [x] `.github/workflows/docs.yml` 存在，配置 push-to-main 触发 + Pages 部署（permissions: pages: write, id-token: write）
- [x] 文档说明 Pages 源需设为 GitHub Actions

## 阶段二：入门篇
- [x] `docs/quickstart.md` 含前置条件、so 下载加载、第一个 shell 任务、第一个 cron 任务、注意事项
- [x] `docs/architecture.md` 含组件图、请求流程、daemon 生命周期、Celery/APScheduler 映射表
- [x] `docs/install-config.md` 含扩展加载、env var 全表、配置文件、路径解析优先级表、服务名校验、PID 双行格式

## 阶段三：API 参考篇
- [x] `docs/api-functions.md` 覆盖全部 27 个 `xhjob_*` 函数 + `Xhjob` PHP 类，每个含签名/参数表/返回/错误契约/注意事项/代码演示
- [x] `docs/api-taskbuilder.md` 覆盖全部 TaskBuilder 静态工厂与链式方法（40+），每个含签名/用途/注意事项/代码演示
- [x] `docs/api-taskmanager.md` 覆盖全部 TaskManager public 方法，含异常契约（ServiceNotRunningException/TaskNotFoundException）

## 阶段四：核心能力篇
- [x] `docs/triggers.md` 覆盖 cron/interval/runAt/countdown/or_cron/skip_dates/workdays_only/jitter/misfire_grace_time/start_date/end_date
- [x] `docs/retry-timeout.md` 覆盖 withRetry/retryBackoff/acksOnFailure/timeout/softTimeout/expires，含 SIGTERM→SIGKILL 升级链说明
- [x] `docs/concurrency.md` 覆盖 maxInstances/allowOverlap/coalesce/rateLimit/priority/maxExecutions
- [x] `docs/persistence-recovery.md` 覆盖 persist/acksLate/execution_lease/watchdog/PID 复用防护/SQLite 完整性/SIGKILL 崩溃恢复
- [x] `docs/orchestration.md` 覆盖 chain/group/chord + 状态查询
- [x] `docs/progress-events.md` 覆盖 reportProgress/pullEvents/events/inspect + EventType 枚举全表（含 lease_held 正确拼写）

## 阶段五：进阶篇
- [x] `docs/pool-modes.md` 含实现差异表、性能特征表、配置项、决策树、切换演示、基准对比说明

## 阶段六：生产实战篇
- [x] `docs/prod-background-queue.md` 含 FPM 派发 + CLI worker 轮询 + 进度上报 + 重试配置 + 注意事项
- [x] `docs/prod-cron.md` 含 cron + 时区 + persist + acksLate + maxExecutions + coalesce + skipDates 完整端到端
- [x] `docs/prod-cli-fpm-share.md` 含共用原理 + 两种启动方式 + 路径优先级表 + 多服务隔离 + PID 双行 + IPC 超时 + systemd 演示
- [x] `docs/prod-thinkphp8.md` 含 composer 安装 + 手动安装 + ServiceProvider + Facade + helper + config + 25 路由 + controller 示例 + 注意事项

## 阶段七：排障篇
- [x] `docs/troubleshooting.md` 覆盖扩展未加载/daemon 启动失败/error 前缀/卡 Running/SQLite 损坏/FPM 阻塞/persist 未启用/zombie 残留

## 阶段八：导航与验证
- [x] 左侧导航按 6 分组（入门/API 参考/核心能力/进阶/生产实战/排障）正确显示
- [x] 所有代码演示的函数签名与 `src/lib.rs` 导出一致
- [x] 所有代码演示的 TaskBuilder/TaskManager 方法与 `releases/xhjob-thinkphp8-extend/Xhjob/*.php` 一致
- [x] 所有 env var 名与 `src/` 实际读取点一致
- [x] 所有 EventType 枚举值与 `src/store/mod.rs` 一致（`lease_held` 非 `leaseheld`）
- [x] 不修改任何 Rust/PHP 源码（纯文档产出）
- [x] 不回滚用户既有改动
