# 修复 daemon_main 日志目录回退到 /var/log/xhjob Spec

> change-id: `fix-daemon-log-dir-fallback`
> 前序：`fix-fpm-binary-resolution-v2`（已合并 main，php_binary 解析已修复）

## Why

v2 修复后用户再次报错，错误从 `exit 16384` (EX_USAGE) 变成 `exit 6` (SIGABRT)——证明 php_binary 解析问题已解决，daemon 子进程已能启动，但 panic 在 `tracing-appender` 初始化阶段：

```
thread '<unnamed>' panicked at tracing-appender-0.2.5/src/rolling.rs:156
initializing rolling file appender failed: InitError { 
  context: "failed to create log directory", 
  source: Os { code: 13, kind: PermissionDenied, message: "Permission denied" } 
}
```

**根因**：`src/daemon_main.rs:66-68` 的 `log_dir` 回退路径与其它路径解析逻辑不一致：

```rust
let log_dir =
    crate::service::current_data_dir().unwrap_or_else(|| "/var/log/xhjob".to_string());
```

当 `current_data_dir()` 返回 `None`（即 `xhjob_run_daemon` 未传 `data_dir` 参数 + `XHJOB_DATA_DIR` env var 未设）时，`log_dir` 回退到 `/var/log/xhjob`。但 FPM worker（uid=1001）无权在 `/var/log/` 下创建目录，`tracing_appender::rolling::daily` 内部 `create_dir_all("/var/log/xhjob")` 失败 → panic。

**触发条件**：用户调用 `xhjob_start()` 不传 `data_dir`（或传 null/空字符串）时：
1. `xhjob_start` 的 `data_dir` 参数是 `None`
2. `spawn_via_double_fork(daemon_main, service_name, None)` 收到 `data_dir=None`
3. `-r` 代码生成为 `xhjob_run_daemon('default');`（只 1 个参数）
4. daemon 子进程执行 `xhjob_run_daemon('default')`，`data_dir` 参数默认 `None`，`set_current_data_dir` 不被调用
5. `XHJOB_DATA_DIR` env var 也不被设置（[unix.rs:79-83](file:///workspace/src/daemon/unix.rs#L79-L83) 仅在 `data_dir` 是 Some 时设置）
6. daemon 子进程的 `current_data_dir()` 返回 `None`
7. `daemon_main` 的 `log_dir` 回退到 `/var/log/xhjob`
8. `tracing_appender::rolling::daily("/var/log/xhjob", ...)` panic

**关键矛盾**：`xhjob_diag` 显示 `data_dir: /tmp`——因为 `resolve_data_dir(None)` 回退到 `/tmp`（Unix 默认）。但 `daemon_main` 用的是**不同的回退路径**（`/var/log/xhjob`），两者不一致。这是设计 bug：`daemon_main` 应该用与 `xhjob_diag`/`xhjob_start` 相同的路径解析逻辑。

## What Changes

### MODIFIED: daemon_main 日志目录解析

**`src/daemon_main.rs`** `log_dir` 解析改为复用 `daemon::resolve_data_dir`：

```rust
// BEFORE:
let log_dir =
    crate::service::current_data_dir().unwrap_or_else(|| "/var/log/xhjob".to_string());

// AFTER:
let log_dir = crate::daemon::resolve_data_dir(
    crate::service::current_data_dir().as_deref()
);
```

新解析顺序（与 `xhjob_diag`/`xhjob_start` 完全一致）：
1. `current_data_dir()` 返回 `Some`（由 `xhjob_run_daemon(_, Some(dir))` 设置）→ 用该值
2. `current_data_dir()` 返回 `None` → `resolve_data_dir(None)` 走：
   - `XHJOB_DATA_DIR` env var
   - `XHJOB_LOG_DIR` env var（fine-grained）
   - 平台默认 `/tmp`（Unix） / `%TEMP%`（Windows）

**移除** `/var/log/xhjob` 回退路径——它是错误的、与其它路径解析不一致的 fallback。`resolve_data_dir` 已经有完整的回退链，最终回退到 `/tmp`（Unix 默认），不会返回 None。

### MODIFIED: daemon_main 错误处理

当前 `let _ = std::fs::create_dir_all(&log_dir);` 静默吞掉错误。改为：如果 `create_dir_all` 失败，回退到 `/tmp`（Unix）/ `std::env::temp_dir()`（跨平台），并 `eprintln!` 警告。这样即便 `resolve_data_dir` 返回的目录不可写（如用户显式传了不可写的 `XHJOB_DATA_DIR`），daemon 也不会 panic，而是退化到 `/tmp`。

```rust
let log_dir = crate::daemon::resolve_data_dir(
    crate::service::current_data_dir().as_deref()
);
// Try to create the log dir; if it fails (e.g. user-specified XHJOB_DATA_DIR
// is not writable by the daemon's uid), fall back to the system temp dir so
// the daemon can still start and write logs somewhere. This avoids a panic
// in tracing_appender::rolling::daily when create_dir_all fails.
let log_dir = if std::fs::create_dir_all(&log_dir).is_ok() {
    log_dir
} else {
    let fallback = std::env::temp_dir().to_string_lossy().into_owned();
    eprintln!(
        "xhjob: WARNING could not create log dir '{}', falling back to '{}'",
        log_dir, fallback
    );
    fallback
};
```

## Impact

- **Affected specs**：无。本 spec 是 `fix-fpm-binary-resolution-v2` 的后续，独立修复一个独立的根因。
- **Affected code**：
  - `src/daemon_main.rs`：`log_dir` 解析改为 `resolve_data_dir`，`create_dir_all` 失败时回退到 `temp_dir()`
- **BREAKING**：无。`daemon_main` 是内部函数，签名不变；日志文件路径从潜在的 `/var/log/xhjob/xhjob.<name>.log.<date>` 变为 `<data_dir>/xhjob.<name>.log.<date>`（通常 `/tmp/xhjob.<name>.log.<date>`），与 stderr 重定向文件 `/tmp/xhjob.<name>.log` 同目录，行为更一致。

## ADDED Requirements

无。

## MODIFIED Requirements

### Requirement: daemon_main 日志目录解析
`daemon_main()` SHALL 用 `daemon::resolve_data_dir(current_data_dir().as_deref())` 解析日志目录，与 `xhjob_diag`/`xhjob_start` 的路径解析逻辑一致。`/var/log/xhjob` 回退路径 SHALL 被移除。

#### Scenario: 用户不传 data_dir 调用 xhjob_start
- **GIVEN** FPM worker（uid=1001）调用 `xhjob_start("default")`（不传 data_dir）
- **WHEN** daemon 子进程执行 `xhjob_run_daemon("default")`，`current_data_dir()` 返回 None
- **THEN** `daemon_main` 的 `log_dir` = `resolve_data_dir(None)` = `/tmp`（Unix 默认）
- **AND** `tracing_appender::rolling::daily("/tmp", "xhjob.default.log")` 成功创建 `/tmp/xhjob.default.log.<date>`
- **AND** daemon 正常启动，不 panic

#### Scenario: 用户传不可写的 data_dir
- **GIVEN** FPM worker（uid=1001）调用 `xhjob_start("default", "/root/not-writable")`
- **WHEN** daemon 子进程执行 `xhjob_run_daemon("default", "/root/not-writable")`，`current_data_dir()` 返回 `Some("/root/not-writable")`
- **THEN** `log_dir` = `/root/not-writable`
- **AND** `create_dir_all("/root/not-writable")` 失败（PermissionDenied）
- **AND** `daemon_main` 回退到 `std::env::temp_dir()`（`/tmp`），eprintln 警告
- **AND** `tracing_appender::rolling::daily("/tmp", "xhjob.default.log")` 成功
- **AND** daemon 正常启动，不 panic

## REMOVED Requirements

### Requirement: /var/log/xhjob 回退路径
**Reason**: 与其它路径解析逻辑不一致，且 FPM worker（uid=1001）无权写 `/var/log/`，导致 panic。
**Migration**: 无。该回退路径从未在 `xhjob_diag`/`xhjob_start` 中使用，仅 `daemon_main` 误用。
