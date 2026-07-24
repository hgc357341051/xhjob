<!-- docs/README.md -->

# Xhjob

> PHP 异步任务调度扩展 · Rust 驱动 · 双线程池 · 持久化崩溃恢复

Xhjob 是一个用 Rust + [ext-php-rs](https://github.com/davidcole1340/ext-php-rs) 实现的 PHP 扩展，把 PHP 进程内的「耗时任务」异步派发到一个独立的 Rust 守护进程（daemon）执行，daemon 内置 SQLite 持久化、cron 调度、双线程池（async / thread）、链式编排（chain/group/chord）与崩溃恢复。

## 核心特性

<div class="feature-grid">
  <div class="feature-card">
    <span class="icon">⚡</span>
    <h3>双线程池</h3>
    <p>async 模式（tokio M:N，默认并发 1024）适合 IO 密集；thread 模式（std::thread 1:1，默认 CPU 核数）适合 CPU 密集与强隔离场景。</p>
  </div>
  <div class="feature-card">
    <span class="icon">💾</span>
    <h3>持久化崩溃恢复</h3>
    <p>SQLite + WAL 存储任务定义与执行结果；daemon SIGKILL 后重启，Running 任务自动重置为 Pending 重新派发（acksLate）。</p>
  </div>
  <div class="feature-card">
    <span class="icon">🔗</span>
    <h3>链式编排</h3>
    <p>chain 顺序执行（上一步 stdout 作为下一步 stdin）、group 并行执行、chord（header 并行 + callback 回调）。</p>
  </div>
  <div class="feature-card">
    <span class="icon">⏰</span>
    <h3>Cron & 时区</h3>
    <p>5/6 字段 cron 表达式，支持时区、or_cron 多表达式并集、skipDates 跳过节假日、workdaysOnly 仅工作日、misfire_grace_time 补触发。</p>
  </div>
  <div class="feature-card">
    <span class="icon">🔄</span>
    <h3>重试与超时</h3>
    <p>withRetry + retryBackoff 指数退避、softTimeout SIGTERM→SIGKILL 升级链、acksOnFailure(false) 无限重试、expires 过期丢弃。</p>
  </div>
  <div class="feature-card">
    <span class="icon">🎯</span>
    <h3>并发控制</h3>
    <p>maxInstances、allowOverlap、coalesce 合并漏触发、rateLimit 滑动窗口限流、priority 优先级调度、maxExecutions 执行次数限制。</p>
  </div>
  <div class="feature-card">
    <span class="icon">📊</span>
    <h3>进度与事件</h3>
    <p>reportProgress 任务内上报进度（0-100）、events/pullEvents 事件流（14 种 EventType）、inspect 四种模式检视 daemon 状态。</p>
  </div>
  <div class="feature-card">
    <span class="icon">🐘</span>
    <h3>ThinkPHP 8 集成</h3>
    <p>第三方类库方式：composer 安装、ServiceProvider 自动注册、Xhjob Facade、helper 函数、25 个 HTTP 端点。</p>
  </div>
</div>

## 快速开始

```php
<?php
// 1. 加载扩展（或写入 php.ini）
// php -d extension=/path/to/xhjob.so

// 2. 启动 daemon（独立进程，CLI 与 FPM 都连它）
xhjob_start();

// 3. 派发一个 shell 任务，立即返回 task_id
$taskId = xhjob_dispatch(
    (new \Xhjob\TaskBuilder())
        ->shell('echo hello && sleep 2 && echo done')
        ->withRetry(3, 1)
        ->timeout(10)
        ->toJson()
);

echo "task_id = {$taskId}\n";

// 4. 轮询状态与结果
while (true) {
    $state = xhjob_state($taskId);
    $s = $state['state'] ?? 'UNKNOWN';
    echo "state = {$s}\n";
    if (in_array($s, ['success', 'failed', 'cancelled', 'expired', 'interrupted'], true)) {
        break;
    }
    usleep(500_000);
}

print_r(xhjob_result($taskId));
```

## 三种零构建部署方式

本站基于 [Docsify](https://docsify.js.org/) 构建，**零编译、零流水线**，浏览器直接打开即可渲染。

| 方式 | 命令 / 操作 | 适用场景 |
|---|---|---|
| ① 本地预览 | `python -m http.server 8080 -d docs` 后访问 `http://localhost:8080/` | 写文档时本地验证 |
| ② GitHub Pages | 仓库 Settings → Pages → Source: **Deploy from a branch** → 分支 `main` / 目录 `/docs` | 公开托管，无需 Actions |
| ③ 任意静态服务器 | nginx / apache / CDN 直接托管 `docs/` 目录 | 自建或内网部署 |

> ⚠️ 由于 docsify 通过 fetch 加载 markdown，**直接双击 `index.html` 打开（file:// 协议）会被浏览器 CORS 拦截**，请用任意 http server 启动后访问。

## 技术栈

| 组件 | 技术 |
|---|---|
| PHP 扩展 | Rust + ext-php-rs，导出 27 个 `xhjob_*` 函数 + `Xhjob` 链式 Builder 类 |
| 守护进程 | tokio M:N runtime（async 模式）或 std::thread 1:1 池（thread 模式） |
| 持久化 | SQLite + WAL，`XHJOB_PERSIST` 控制（编译时需 `--all-features`） |
| IPC | Unix domain socket（Linux），默认超时 5s（`XHJOB_IPC_TIMEOUT_SECS`） |
| 配置 | `/etc/xhjob/config` 文件 + `XHJOB_*` 环境变量覆盖 |
| PHP 集成 | ThinkPHP 8 第三方类库（ServiceProvider + Facade + helper） |

## 文档导航

- **入门**：[快速开始](quickstart.md) · [架构概览](architecture.md) · [安装与配置](install-config.md)
- **API 参考**：[PHP 函数](api-functions.md) · [TaskBuilder](api-taskbuilder.md) · [TaskManager](api-taskmanager.md)
- **核心能力**：触发器 · 重试超时 · 并发控制 · 持久化崩溃恢复 · 编排 · 进度事件
- **进阶**：[双线程池模式对比](pool-modes.md)
- **生产实战**：后台队列 · 定时任务 · CLI-FPM 共用 · ThinkPHP 8 集成
- **排障**：[故障排查](troubleshooting.md)

## License

Apache-2.0
