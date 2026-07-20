# xhjob
XHJob 是一个基于 Rust（ext-php-rs 0.15）开发的高性能 PHP 异步任务调度扩展，采用 master-worker 多进程池架构（参考 PHP-FPM），提供 真正的 PHP handler 并发执行、链式 API、cron 定时任务，以及跨平台独立 daemon 守护进程模式，无需 C 桥接层，也无需任何外部依赖（supervisor / crontab / Swoole）。
