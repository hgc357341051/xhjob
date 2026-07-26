# 修复 FPM 上下文 binary 解析在生产环境仍失败 Spec

> change-id: `fix-fpm-binary-resolution-v2`
> 前序：`fix-fpm-php-binary-resolution`（已合并到 main，但在用户生产环境未生效）

## Why

用户在 ThinkPHP 8 + PHP-FPM 8.2 生产环境（宝塔 `/www/server/php/82/sbin/php-fpm`，uid=1001）执行控制器 demo 仍报与之前**完全相同**的错误：

```
xhjob_start 返回 false (name=tp-demo)
reason: daemon child exited prematurely with status ExitStatus(unix_wait_status(16384))  # 64<<8 = EX_USAGE
diag: {"php_binary":"/www/server/php/82/sbin/php-fpm",
       "sapi":"fpm-fcgi",
       "extension_loaded_via_php_ini":true,
       "data_dir_writable":true,
       "open_basedir":"",
       ...}  # 注意：没有 php_binary_raw 字段
```

**两条独立根因**：

### 根因 A：用户加载的是旧 .so（首要原因）
诊断 JSON **没有 `php_binary_raw` 字段**，而 `fix-fpm-php-binary-resolution` 的 Task 3 已在 `src/lib.rs:238` 新增该字段并随 commit `9610568` 推送到 main。这说明用户 PHP-FPM 实际加载的 `.so` 还是修复前的版本。可能原因：
- 宝塔面板的 PHP-FPM 通过 `php.ini` 的 `extension=xhjob.so` 加载，但 `xhjob.so` 在 PHP 扩展目录里是**旧版本**（用户没有把新编译的 `releases/xhjob-php8.2-linux-x86_64.so` 复制到 FPM 实际加载的扩展目录，如 `/www/server/php/82/lib/php/extensions/no-debug-non-zts-20220829/`）
- PHP-FPM 未重启（即便替换了 .so 文件，FPM worker 进程仍持有旧 mmap 的 .so）
- 用户从 git 拉取了新源码但没重新编译，或编译后没安装

### 根因 B：即便加载新 .so，`resolve_php_binary()` 在真实 FPM 上下文可能仍失败
前序 spec 的 `validate_php_binary()` 用 `Command::new(candidate).arg("-n").arg("-v").spawn()` 验证候选 CLI php 二进制。在 PHP-FPM worker 上下文，这个 spawn 可能因以下原因失败导致所有候选都被拒绝、最终回退到 `php-fpm`：
1. **PHP-FPM worker 的 `PATH` 被重置为极简值**（如 `/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin`），不含 `php`，`which_php()` 返回 None
2. **`/www/server/php/82/bin/php` 在 FPM worker 视角下不可执行**（宝塔某些配置下 `bin/` 目录权限是 750 属主 root，uid=1001 无权执行）
3. **`open_basedir` 实际生效但 `xhjob_diag` 读不到**（虽然 diag 显示空，但 PHP-FPM 可能在 worker 启动后才 apply open_basedir，`Command::spawn` 的子进程继承该限制无法 exec `/www/server/php/82/bin/php`）
4. **SELinux/AppArmor** 限制 FPM worker fork+exec 其它二进制
5. **`validate_php_binary` 的 1s 超时太短**：冷启动时 `php -v` 可能 > 1s（特别是带 opcache 预加载的构建）

更根本的设计问题：**用 spawn 子进程验证二进制可用性本身在 FPM 上下文就是不可靠的**——如果 spawn 能成功，那 daemon 启动也能成功；如果 spawn 失败，验证就误判候选不可用。验证逻辑应该**只检查文件存在 + 可执行权限**，把"是否真能运行"留给 daemon spawn 自己去试。

## What Changes

### Rust 核心扩展（src/）

#### 修复（MODIFIED）

- **`validate_php_binary()` 改为只做静态检查**（`src/daemon/mod.rs`）：不再 spawn `<candidate> -n -v`，改为：
  1. `path.is_file()` 必须为 true
  2. Unix 下检查 `metadata.permissions().mode() & 0o111 != 0`（任意可执行位）
  3. 返回 `true`，不实际 spawn
  - 理由：spawn 验证在 FPM 上下文不可靠（见根因 B），且即便 spawn 成功也不保证 daemon spawn 成功（环境不同）。静态检查足够排除明显不存在的候选，剩下的让 daemon spawn 自己暴露真实失败原因（通过 `check_child_alive` 的 exit 64 hint + 日志尾部）。
  - 保留 `validate_php_binary` 函数名与签名，避免改动调用点。
  - **保留**前序的 spawn-based 验证逻辑为一个新函数 `validate_php_binary_spawn()`，仅用于单测（不在线上路径调用），避免删代码引发回归。

- **`resolve_php_binary()` 增加候选扩展**（`src/daemon/mod.rs`）：在现有候选（同目录 `php`、`../bin/php`、`which php`）基础上，新增：
  - `<raw_dir>/../sbin/php`（某些安装布局 sbin 与 bin 同级）
  - 当 raw 文件名是 `php-fpm` 时，尝试 `<raw_dir>/php-fpm` → `<raw_without "-fpm">`（即把 `php-fpm` 文件名去掉 `-fpm` 后缀作为候选，如 `/www/server/php/82/sbin/php-fpm` → `/www/server/php/82/sbin/php`，与同目录 php 重复但无害）
  - 候选顺序：同目录 `php` → `../bin/php` → `../sbin/php` → `which php` → 回退 raw

- **`resolve_php_binary()` 候选探测增加日志**（`src/daemon/mod.rs`）：每个候选的 `tracing::debug!` 已存在，但增加一条 `tracing::warn!` 在所有候选都失败时输出**完整候选列表 + 每个候选失败的具体原因**（`is_file=false` / `not executable` / etc），写入 daemon 日志文件让用户能看到为什么回退到 raw。`xhjob_last_start_error()` 在 spawn 失败后也应包含这条候选探测摘要。

- **`xhjob_diag()` 增加 `php_binary_candidates` 字段**（`src/lib.rs`）：返回 JSON 数组，列出 `resolve_php_binary()` 探测过的所有候选路径及其 validate 结果（`{"path":"...","valid":bool,"reason":"..."}`），让用户在 diag 输出里直接看到为什么 resolved 是某个路径。同时**保留** `php_binary_raw` 字段（前序已加）。

- **`xhjob_diag()` 增加 `xhjob_so_md5` 字段**（`src/lib.rs`）：返回当前加载的 `libxhjob.so` 的 MD5 哈希（通过读取 `/proc/self/maps` 找到 xhjob.so 的 mmap 路径，或回退到 `current_exe()` 同目录推断），让用户能确认加载的是哪个版本的 .so。这是根因 A 的诊断手段——如果 MD5 与 `releases/xhjob-php8.2-linux-x86_64.so` 不一致，说明加载的是旧 .so。
  - 实现细节：用 `ext_php_rs` 的 `module_loaded("xhjob")` 确认扩展已加载；MD5 计算用 `md5` crate（**需新增依赖**）或用 `sha2`（已在依赖树里？检查 Cargo.lock）。如果不想加依赖，退化方案：返回 `.so` 文件的 mtime + size 作为指纹（不如 MD5 准确但足够区分新旧）。
  - **决定**：不新增依赖。返回 `.so` 文件路径 + size + mtime（Unix epoch 秒）三元组，PHP 侧用户可以用 `md5_file($path)` 自己算 MD5 比对。字段名 `xhjob_so_info`：`{"path":"/www/server/.../xhjob.so","size_bytes":11828376,"mtime_epoch":1753560123}`。
  - 路径解析：优先从 `DL_LOAD` / `zend_module_entry` 获取（ext-php-rs 可能不暴露），退化到扫描 `/proc/self/maps` 找包含 `xhjob.so` 的行。若都失败，返回 `{"path":null,"error":"..."}`。

### 编译产物 + 安装文档

- **重新编译 .so**：`cargo build --release --features persist`，覆盖 `releases/xhjob-php8.2-linux-x86_64.so`，记录 MD5。
- **README 增加"如何确认加载的 .so 版本"小节**：访问 `/xhjob/diag?name=tp-demo&data_dir=/tmp/xhjob-tp-demo`，检查 JSON 里是否有 `php_binary_raw` 与 `xhjob_so_info` 字段。如果没有这两个字段，说明加载的是 v2 之前的旧 .so，需重新安装 + 重启 FPM。**不**创建独立 md 文档（遵守 guardrail）。

### Git 提交

- **新分支** `fix-fpm-binary-resolution-v2`，commit + push 到 origin。**不**直接合并 main（用户上次要求合并 main，这次走 PR 流程让用户审阅）。

## Impact

- **Affected specs**：
  - `fix-fpm-php-binary-resolution`（前序）：本 spec 不推翻前序，仅增强 `validate_php_binary` 鲁棒性 + 新增诊断字段。前序的 `resolve_php_binary` 主体逻辑保留。
  - `fix-fpm-daemon-start`（前前序）：不动。
- **Affected code**：
  - `src/daemon/mod.rs`：`validate_php_binary` 改为静态检查；`resolve_php_binary` 增加 `../sbin/php` 候选 + 候选失败日志增强；新增 `validate_php_binary_spawn`（保留旧逻辑供单测）。
  - `src/lib.rs`：`xhjob_diag` 增加 `php_binary_candidates` 与 `xhjob_so_info` 字段。
  - `releases/xhjob-php8.2-linux-x86_64.so`：重新编译。
  - `README.md`：增加"确认 .so 版本"小节。
- **BREAKING**：无。
  - `xhjob_diag` 仅新增字段，不删不改变现有字段语义。
  - `validate_php_binary` 签名不变（`fn(&Path) -> bool`），仅实现改为静态检查。调用点不变。

## ADDED Requirements

### Requirement: 静态二进制验证
`validate_php_binary()` SHALL 只做静态检查（文件存在 + 可执行位），不 spawn 子进程，避免在 FPM 上下文因 spawn 限制导致所有候选被误判不可用。

#### Scenario: 候选二进制存在但 FPM worker 无权 spawn
- **GIVEN** PHP-FPM worker（uid=1001），候选 `/www/server/php/82/bin/php` 存在且 mode=0755，但 FPM worker 被 `disable_functions` 或 SELinux 限制无法 spawn 子进程
- **WHEN** `resolve_php_binary()` 调用 `validate_php_binary("/www/server/php/82/bin/php")`
- **THEN** 返回 `true`（基于静态检查：is_file=true, mode & 0o111 != 0）
- **AND** `resolve_php_binary()` 返回 `(PathBuf::from("/www/server/php/82/bin/php"), raw)`
- **AND** daemon spawn 实际尝试用该路径，若 spawn 失败由 `check_child_alive` 捕获并写入 `xhjob_last_start_error`

#### Scenario: 候选二进制不存在
- **GIVEN** 候选 `/www/server/php/82/sbin/php` 不存在
- **WHEN** `validate_php_binary("/www/server/php/82/sbin/php")`
- **THEN** 返回 `false`（is_file=false）

### Requirement: 扩展候选路径
`resolve_php_binary()` SHALL 在现有候选基础上新增 `<raw_dir>/../sbin/php`，覆盖 sbin/bin 同级布局的安装。

#### Scenario: 宝塔布局 sbin 优先
- **GIVEN** raw=`/www/server/php/82/sbin/php-fpm`，`/www/server/php/82/sbin/php` 不存在，`/www/server/php/82/bin/php` 存在
- **WHEN** `resolve_php_binary()`
- **THEN** 依次尝试：`/www/server/php/82/sbin/php`（fail）→ `/www/server/php/82/bin/php`（success）
- **AND** 返回 resolved=`/www/server/php/82/bin/php`

#### Scenario: sbin 同级布局
- **GIVEN** raw=`/opt/php/sbin/php-fpm`，`/opt/php/sbin/php` 不存在，`/opt/php/bin/php` 也不存在，`/opt/php/sbin/php` 是同目录候选已试过，但 `/opt/php/sbin/../sbin/php` 与同目录重复
- **WHEN** `resolve_php_binary()`
- **THEN** `../sbin/php` 候选等于同目录 `php`，去重后跳过（不重复 spawn validate）

### Requirement: 诊断字段暴露候选探测详情
`xhjob_diag()` SHALL 返回 `php_binary_candidates` 字段，列出所有探测过的候选路径 + validate 结果 + 失败原因，让用户无需翻日志即可定位为什么 resolved 是某路径。

#### Scenario: FPM 上下文 diag 暴露候选列表
- **WHEN** 在 FPM 上下文调用 `xhjob_diag('tp-demo', '/tmp/xhjob-tp-demo')`
- **THEN** 返回 JSON 含 `"php_binary_candidates": [{"path":"/www/server/php/82/sbin/php","valid":false,"reason":"not a file"},{"path":"/www/server/php/82/bin/php","valid":true,"reason":"ok"},...]`
- **AND** `php_binary` 字段 = 第一个 valid 的候选
- **AND** `php_binary_raw` 字段 = `current_exe()` 原始值

### Requirement: 诊断字段暴露 .so 文件信息
`xhjob_diag()` SHALL 返回 `xhjob_so_info` 字段，包含当前加载的 `xhjob.so` 文件路径 + size + mtime，让用户能确认加载的是哪个版本的 .so。

#### Scenario: 用户加载旧 .so 时 diag 暴露路径
- **GIVEN** FPM 通过 `/www/server/php/82/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so` 加载扩展，该文件是旧版本（无 `php_binary_raw` 字段的 .so）
- **WHEN** 调用 `xhjob_diag()`
- **THEN** 返回 JSON 含 `"xhjob_so_info": {"path":"/www/server/php/82/lib/php/extensions/no-debug-non-zts-20220829/xhjob.so","size_bytes":11797120,"mtime_epoch":...}`
- **AND** 用户比对 `releases/xhjob-php8.2-linux-x86_64.so` 的 size（11828376）发现不一致，确认加载的是旧 .so

#### Scenario: 无法解析 .so 路径
- **GIVEN** `/proc/self/maps` 不可读或不含 xhjob.so 条目
- **WHEN** 调用 `xhjob_diag()`
- **THEN** 返回 `"xhjob_so_info": {"path":null,"error":"could not resolve xhjob.so path from /proc/self/maps"}`

## MODIFIED Requirements

### Requirement: resolve_php_binary 候选失败诊断
`resolve_php_binary()` 在所有候选都失败回退到 raw 时，SHALL 通过 `tracing::warn!` 输出完整候选列表 + 每个候选的失败原因，并在 `xhjob_diag()` 的 `php_binary_candidates` 字段中暴露同样信息。

#### Scenario: 所有候选失败时 diag 暴露详情
- **GIVEN** raw=`/www/server/php/82/sbin/php-fpm`，同目录无 php、`../bin/php` 不可执行、`which php` 返回 None
- **WHEN** 调用 `xhjob_diag()`
- **THEN** `php_binary` = raw（回退）
- **AND** `php_binary_candidates` 列出所有尝试过的候选 + 每个的失败原因
- **AND** daemon 日志含 `tracing::warn!` 输出候选列表

## REMOVED Requirements

无。前序的 spawn-based `validate_php_binary` 逻辑保留为 `validate_php_binary_spawn` 供单测，不删除。
