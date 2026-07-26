# Tasks — fix-daemon-log-dir-fallback

## Phase 1: 修复 daemon_main log_dir 解析

- [ ] Task 1: 修改 `src/daemon_main.rs` 的 `log_dir` 解析与错误处理
  - [ ] SubTask 1.1: `log_dir` 改为 `crate::daemon::resolve_data_dir(crate::service::current_data_dir().as_deref())`
  - [ ] SubTask 1.2: `create_dir_all(&log_dir)` 失败时回退到 `std::env::temp_dir()`，`eprintln!` 警告
  - [ ] SubTask 1.3: 移除 `/var/log/xhjob` 回退路径
  - [ ] SubTask 1.4: 更新 `log_dir` 上方的注释（P0-15 段），说明新解析逻辑与 `xhjob_diag`/`xhjob_start` 一致 + 回退到 temp_dir 的原因
  - [ ] SubTask 1.5: 单测 `test_daemon_main_log_dir_resolves_to_tmp_when_no_data_dir`（单元测试可能困难，因为 `daemon_main` 是函数而非可测试单元；改为对 `resolve_data_dir` 的已有测试覆盖 + 新增一个集成测试验证 `log_dir` 解析逻辑。或者：把 log_dir 解析提取为一个可测函数 `fn resolve_log_dir() -> String`，单测它）

- [ ] Task 2: 提取 `resolve_log_dir()` 为可测函数
  - [ ] SubTask 2.1: 在 `daemon_main.rs` 新增 `fn resolve_log_dir() -> String`，封装 Task 1 的解析 + 回退逻辑
  - [ ] SubTask 2.2: `daemon_main` 调用 `let log_dir = resolve_log_dir();`
  - [ ] SubTask 2.3: 单测 `test_resolve_log_dir_uses_current_data_dir_when_set`（设置 `CURRENT_DATA_DIR` 后断言返回该值——但 OnceLock 不能跨测试重置，改为用 env var `XHJOB_DATA_DIR` 测试）
  - [ ] SubTask 2.4: 单测 `test_resolve_log_dir_falls_back_to_tmp_when_unset`（清空 env var，断言返回 `/tmp` 或 `temp_dir()`）
  - [ ] SubTask 2.5: 单测 `test_resolve_log_dir_falls_back_to_tmp_when_create_dir_fails`（传一个不可写的路径通过 env var，断言回退到 temp_dir——但 `resolve_data_dir` 不验证可写性，回退逻辑在 `resolve_log_dir` 内部。这个测试需要 mock `create_dir_all`，Rust 无 mock 标准库。改为：`resolve_log_dir` 接受一个 `create_dir: impl Fn(&str) -> std::io::Result<()>` 参数，单测传入 always-fail 的闭包。或者：不测这个分支，仅靠 E2E 验证。**决定**：不测这个分支，仅靠 E2E 验证 + 代码 review）

## Phase 2: 编译 + 单测 + clippy

- [ ] Task 3: cargo build + test + clippy
  - [ ] SubTask 3.1: `cargo build --features persist` 无 warning
  - [ ] SubTask 3.2: `cargo test --features persist` 100% 通过（前序 200 + 新增 2 个）
  - [ ] SubTask 3.3: `cargo clippy --all-targets --features persist -- -D warnings` 无 warning
  - [ ] SubTask 3.4: `cargo build --release --features persist` 编译 .so
  - [ ] SubTask 3.5: 复制 `target/release/libxhjob.so` 到 `releases/xhjob-php8.2-linux-x86_64.so`，记录 MD5

## Phase 3: E2E 验证（uid=1001 复现）

- [ ] Task 4: 用 u1001 复现用户场景
  - [ ] SubTask 4.1: 清理 /tmp/xhjob.default.* 残留
  - [ ] SubTask 4.2: 以 u1001 运行 `php -d extension=xhjob.so -r 'xhjob_start("default"); usleep(2000000);'`（不传 data_dir）
  - [ ] SubTask 4.3: 验证 `xhjob_start` 返回 true，daemon 启动成功
  - [ ] SubTask 4.4: 验证 `/tmp/xhjob.default.log.<date>` 存在且属主 u1001
  - [ ] SubTask 4.5: 验证 `/var/log/xhjob` 不存在（不应再创建）
  - [ ] SubTask 4.6: 清理：停止 daemon、删除测试文件

## Phase 4: Git 提交推送

- [ ] Task 5: 创建分支 + 提交 + 推送 + 合并 main
  - [ ] SubTask 5.1: `git checkout -b fix-daemon-log-dir-fallback`（基于当前 main HEAD）
  - [ ] SubTask 5.2: `git add` 仅目标文件（src/daemon_main.rs、releases/xhjob-php8.2-linux-x86_64.so、.trae/specs/fix-daemon-log-dir-fallback/）
  - [ ] SubTask 5.3: `git commit` 用 trae-agent 身份，message 说明根因（log_dir 回退到 /var/log/xhjob 与其它路径解析不一致）+ 修复（复用 resolve_data_dir + create_dir 失败回退 temp_dir）
  - [ ] SubTask 5.4: `git push -u origin fix-daemon-log-dir-fallback`
  - [ ] SubTask 5.5: 切到 main，`git merge --ff-only fix-daemon-log-dir-fallback`，`git push origin main`

# Task Dependencies

- Task 1（修改 daemon_main）与 Task 2（提取 resolve_log_dir）是同一个修改的两种实现风格，**合并实现**：Task 1 直接做 Task 2 的提取，避免重复改代码
- Task 3（编译测试）依赖 Task 1-2
- Task 4（E2E）依赖 Task 3
- Task 5（git 提交）依赖 Task 4
