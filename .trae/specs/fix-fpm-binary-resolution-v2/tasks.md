# Tasks — fix-fpm-binary-resolution-v2

> 顺序执行：Phase 1 改 validate 为静态检查 + 扩展候选，Phase 2 加诊断字段，Phase 3 编译测试，Phase 4 端到端验证，Phase 5 git 提交推送。

## Phase 1: Rust 核心鲁棒性增强

- [x] Task 1: `validate_php_binary` 改为静态检查
  - [x] SubTask 1.1: `src/daemon/mod.rs` `validate_php_binary` 实现改为：`path.is_file()` + Unix 下 `mode & 0o111 != 0`，不 spawn 子进程
  - [x] SubTask 1.2: 保留旧 spawn-based 逻辑为新函数 `validate_php_binary_spawn(path: &Path) -> bool`（私有，仅单测调用），避免删代码引发回归
  - [x] SubTask 1.3: 更新 `validate_php_binary` docstring 说明改为静态检查的原因（FPM 上下文 spawn 不可靠）
  - [x] SubTask 1.4: 单测 `test_validate_php_binary_returns_false_for_missing`（已有，仍通过）、`test_validate_php_binary_returns_true_for_executable_file`（新建）、`test_validate_php_binary_returns_false_for_non_executable`（新建，Unix-gated）、`test_validate_php_binary_spawn_returns_false_for_missing`（新建，覆盖旧 spawn 逻辑）

- [x] Task 2: `resolve_php_binary` 扩展候选 + 候选探测记录
  - [x] SubTask 2.1: `src/daemon/mod.rs` `resolve_php_binary` 候选列表新增 `<raw_dir>/../sbin/php`，去重（同目录 php 与 ../sbin/php 在某些布局可能解析到同一路径，用 `Path::canonicalize` 去重，canonicalize 失败时 fallback 字符串比较）
  - [x] SubTask 2.2: 新增 `struct PhpBinaryCandidate { path: PathBuf, valid: bool, reason: String }`，`resolve_php_binary` 内部记录每个候选的探测结果到 `Vec<PhpBinaryCandidate>`
  - [x] SubTask 2.3: `resolve_php_binary` 返回值改为 `(PathBuf, PathBuf, Vec<PhpBinaryCandidate>)`（resolved, raw, candidates）；调用点（unix.rs/windows.rs/check_data_dir_writable/lib.rs）同步更新解构
  - [x] SubTask 2.4: 所有候选失败时 `tracing::warn!` 输出完整候选列表 + 失败原因（格式 `[{path} -> {reason}, ...]`）
  - [x] SubTask 2.5: 单测 `test_resolve_php_binary_records_candidates`（断言返回的 candidates 非空且每个元素有 path/valid/reason 字段）；同步更新 `test_resolve_php_binary_respects_env_override` 与 `test_resolve_php_binary_falls_back_to_raw` 解构 3-tuple

## Phase 2: 诊断字段

- [x] Task 3: `xhjob_diag` 增加 `php_binary_candidates` 与 `xhjob_so_info` 字段
  - [x] SubTask 3.1: `src/lib.rs` `xhjob_diag` 调用 `daemon::resolve_php_binary()` 拿到 `(resolved, raw, candidates)`，`php_binary` = resolved，`php_binary_raw` = raw（前序已有），新增 `php_binary_candidates` = candidates 序列化为 JSON 数组（每元素 `{path, valid, reason}`）
  - [x] SubTask 3.2: 新增 `fn resolve_xhjob_so_path() -> Option<PathBuf>`：Linux 下扫描 `/proc/self/maps` 找 `file_name == "xhjob.so"` 的行，解析出路径；非 Linux 或未找到返回 None
  - [x] SubTask 3.3: 新增 `fn xhjob_so_info() -> serde_json::Value`：调用 `resolve_xhjob_so_path`，拿到路径后读 `metadata` 取 size + mtime，返回 `{"path":..., "size_bytes":..., "mtime_epoch":...}`；路径为 None 时返回 `{"path":null, "error":"..."}`；metadata 失败时返回 `{"path":..., "error":"<io>"}`
  - [x] SubTask 3.4: `xhjob_diag` JSON 输出新增 `"php_binary_candidates": [...]` 与 `"xhjob_so_info": {...}` 字段
  - [x] SubTask 3.5: docstring 更新：列出两个新字段 + 说明用途（xhjob_so_info 用于比对 .so 版本；php_binary_candidates 用于定位 resolved 路径选择原因）
  - [x] SubTask 3.6: 单测 `test_diag_includes_php_binary_candidates`、`test_diag_includes_xhjob_so_info`、`test_resolve_xhjob_so_path_returns_option_no_panic`；同步更新 `test_diag_returns_required_fields` 的 `required_keys` 加入两个新字段

## Phase 3: 编译 + 单测 + clippy

- [x] Task 4: Release 编译 + 全量单测 + clippy
  - [x] SubTask 4.1: `cargo build --release --features persist` 编译通过（1m06s），无 warning
  - [x] SubTask 4.2: `cargo test --features persist` 100% 通过（200 passed; 0 failed; 0 ignored — 前序 193 + 新增 7 个）
  - [x] SubTask 4.3: `cargo clippy --all-targets --features persist -- -D warnings` 无 warning（exit 0）
  - [x] SubTask 4.4: 复制 `target/release/libxhjob.so` 到 `releases/xhjob-php8.2-linux-x86_64.so`，MD5 一致：`ba6f147358dfd624b3eb9143616d69d6`（11830208 bytes；含 Task 5.6 record 闭包 bug 修复后的重新编译产物）

## Phase 4: 端到端验证

- [x] Task 5: FPM 模拟 + 字段验证
  - [x] SubTask 5.1: 安装新 .so 到 phpenv 扩展目录，CLI 调用 `xhjob_diag` 验证返回 JSON 含 `php_binary_candidates`（数组，1 个候选）+ `xhjob_so_info`（对象，path 指向扩展目录，size=11830208 与 release 一致）
  - [x] SubTask 5.2: FPM 模拟（cp 真实 php 到 `/tmp/fpm-sim/php-fpm`）调用 `xhjob_diag`，`php_binary_candidates` 列出 4 个候选（同目录 php、../bin/php、../sbin/php、which php），每个有 path/valid/reason；前 3 个 valid=false reason="not a file"，第 4 个 valid=true reason="ok"
  - [x] SubTask 5.3: FPM 模拟调用 `xhjob_start('tp-demo', '/tmp/xhjob-tp-demo')` 返回 true，daemon 启动成功（pid=16517 running=true，PID 文件存在）
  - [x] SubTask 5.4: `xhjob_so_info.path` 指向 `/root/.phpenv/versions/8.2snapshot/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so`，size=11830208 与 `releases/xhjob-php8.2-linux-x86_64.so` 一致
  - [x] SubTask 5.5: 清理：停止 daemon、删除 `/tmp/xhjob-tp-demo` 与 `/tmp/fpm-sim`、`pkill xhjob_run_daemon` 无残留进程
  - [x] SubTask 5.6（额外，bug 修复）: 发现并修复 `record` 闭包的 valid 判定 bug——raw 是 CLI php 时 reason_override="current_exe is CLI php" 被误判为 valid=false。修复为 `valid = reason == "ok" || reason == "current_exe is CLI php"`，重新编译验证通过

## Phase 5: Git 提交推送

- [x] Task 6: 创建分支 + 提交 + 推送
  - [x] SubTask 6.1: `git checkout -b fix-fpm-binary-resolution-v2`（基于当前 main HEAD `acc5676`）
  - [x] SubTask 6.2: `git add` 仅 9 个目标文件（3 spec 文档 + src/daemon/mod.rs + src/daemon/unix.rs + src/daemon/windows.rs + src/lib.rs + releases/xhjob-php8.2-linux-x86_64.so + README.md，未使用 `-A`）
  - [x] SubTask 6.3: `git commit` 用 `git -c user.name=trae-agent -c user.email=trae-agent@users.noreply.github.com` 创建 commit `5eb5e26`（9 files, +864/-42），message 说明根因 A（旧 .so）+ 根因 B（validate spawn 不可靠）+ 修复（静态检查 + 候选扩展 + 诊断字段 + record 闭包 bug 修复）
  - [x] SubTask 6.4: `git push -u origin fix-fpm-binary-resolution-v2` 成功推送，PR 链接：https://github.com/hgc357341051/xhjob/pull/new/fix-fpm-binary-resolution-v2

# Task Dependencies

- Task 1（validate 静态化）与 Task 2（候选扩展）独立可并行，但都改 mod.rs，建议串行避免冲突
- Task 3（diag 字段）依赖 Task 2 的 `resolve_php_binary` 新返回值
- Task 4（编译测试）依赖 Task 1-3
- Task 5（端到端）依赖 Task 4
- Task 6（git 提交）依赖 Task 5

# 风险点

- **`resolve_xhjob_so_path` 跨平台**：`/proc/self/maps` 仅 Linux 有；macOS/Windows 退化返回 None。Linux 是用户生产环境，必须工作。
- **`Path::canonicalize` 去重**：候选路径可能不存在（canonicalize 失败），需 fallback 到字符串比较
- **`xhjob_so_info` 在 CLI 测试环境**：测试二进制（cargo test runner）不加载 xhjob.so，`resolve_xhjob_so_path` 返回 None，`xhjob_so_info` 返回 `{"path":null,"error":"..."}`。这是预期的，单测断言字段存在即可，不断言 path 非空
- **README 修改**：guardrail 禁止 proactive 创建 md 文档，但**修改**现有 README.md 是允许的（用户明确要求"代码提交"，README 是代码的一部分）。仅增加一小节，不重写
- **不直接合并 main**：上次用户要求合并 main，这次走 PR 流程，让用户审阅后再决定是否合并
