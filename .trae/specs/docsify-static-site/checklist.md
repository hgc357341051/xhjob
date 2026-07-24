# Checklist

## 阶段一：清理旧 Jekyll 资产
- [ ] `.github/workflows/docs.yml` 已删除（无 CI 流水线）
- [ ] `docs/_config.yml` 已删除（无 Jekyll 配置）
- [ ] 旧 Jekyll 文档（index.md、6 个分组索引页、19 篇带 front matter 的内容文档）已删除

## 阶段二：Docsify 骨架
- [ ] `docs/index.html` 存在，引入 docsify.js + search/prism/emoji 插件，配置 loadSidebar/coverpage/subMaxLevel/search/auto2top/name
- [ ] `docs/.nojekyll` 存在（GitHub Pages 静态模式不忽略 `_` 文件）
- [ ] `docs/_coverpage.md` 存在，含项目名 + 简介 + 快速开始按钮 + GitHub 链接
- [ ] `docs/_sidebar.md` 存在，6 分组导航（入门/API 参考/核心能力/进阶/生产实战/排障）链接到对应 markdown
- [ ] `docs/README.md` 存在，含项目介绍 + 核心特性 + 快速开始代码 + 部署方式说明

## 阶段三：入门篇
- [ ] `docs/quickstart.md` 含前置条件、so 下载加载、第一个 shell 任务、第一个 cron 任务、注意事项
- [ ] `docs/architecture.md` 含组件图、请求流程、daemon 生命周期、Celery/APScheduler 映射表
- [ ] `docs/install-config.md` 含扩展加载、env var 全表、配置文件、路径解析优先级表、服务名校验、PID 双行格式

## 阶段四：API 参考篇
- [ ] `docs/api-functions.md` 覆盖全部 27 个 `xhjob_*` 函数 + `Xhjob` 类，每个含签名/参数表/返回/错误契约/注意事项/代码演示
- [ ] `docs/api-taskbuilder.md` 覆盖全部 TaskBuilder 静态工厂与链式方法（40+）
- [ ] `docs/api-taskmanager.md` 覆盖全部 TaskManager public 方法，含异常契约

## 阶段五：核心能力篇
- [ ] `docs/triggers.md` 覆盖 cron/interval/runAt/countdown/or_cron/skip_dates/workdays_only/jitter/misfire_grace_time/start_date/end_date（orCron/skipDates/workdaysOnly 标注仅 Rust Xhjob 类可用）
- [ ] `docs/retry-timeout.md` 覆盖 withRetry/retryBackoff/acksOnFailure/timeout/softTimeout/expires，含 SIGTERM→SIGKILL 升级链
- [ ] `docs/concurrency.md` 覆盖 maxInstances/allowOverlap/coalesce/rateLimit/priority/maxExecutions
- [ ] `docs/persistence-recovery.md` 覆盖 persist/acksLate/execution_lease/watchdog(daemon 级)/PID 复用防护/SQLite 完整性/SIGKILL 崩溃恢复
- [ ] `docs/orchestration.md` 覆盖 chain/group/chord + 状态查询
- [ ] `docs/progress-events.md` 覆盖 reportProgress/pullEvents/logs/inspect + EventType 枚举全表（lease_held 正确拼写）

## 阶段六：进阶篇
- [ ] `docs/pool-modes.md` 含实现差异表、性能特征表、配置项、决策树、切换演示、基准对比说明

## 阶段七：生产实战篇
- [ ] `docs/prod-background-queue.md` 含 FPM 派发 + CLI worker 轮询 + 进度上报 + 重试配置 + 注意事项
- [ ] `docs/prod-cron.md` 含 cron + 时区 + persist + acksLate + maxExecutions + coalesce + skipDates 完整端到端
- [ ] `docs/prod-cli-fpm-share.md` 含共用原理 + 两种启动方式 + 路径优先级表 + 多服务隔离 + PID 双行 + IPC 超时 + systemd 演示
- [ ] `docs/prod-thinkphp8.md` 含 composer 安装 + ServiceProvider + Facade(含 chordState) + helper + config + 25 路由 + controller 示例 + 注意事项

## 阶段八：排障篇
- [ ] `docs/troubleshooting.md` 覆盖 8 类故障（无伪方法 watchdogTimeout，watchdog 用 daemon 级 env）

## 阶段九：部署说明与验证
- [ ] `docs/README.md` 含三种零构建部署方式（本地 http.server / GitHub Pages 静态模式 / 静态服务器）
- [ ] 所有 env var 名与 `src/` 实际读取点一致（XHJOB_SERVICE_NAME 非 XHJOB_SERVICE）
- [ ] 所有函数签名与 `src/lib.rs` 导出一致
- [ ] 所有 TaskBuilder/TaskManager 方法与 PHP 类一致（无伪方法 events()/watchdogTimeout()）
- [ ] 所有 EventType 枚举值与 `src/store/mod.rs` 一致（lease_held 非 leaseheld）
- [ ] `python -m http.server -d docs` 启动后访问 http://localhost:8000 封面+导航+内容渲染正常，无 404
- [ ] 不修改任何 Rust/PHP 源码（纯 HTML/markdown 产出）
- [ ] 不回滚用户既有改动（删除的 docs/ 是上一轮 spec 工作产物，按用户明确指令删除重建）
