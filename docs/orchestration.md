# 编排（chain / group / chord）

> 核心能力篇 · 顺序执行 · 并行汇总 · 回调栅栏 · 三种编排原语的状态查询与对比

Xhjob 提供三种编排原语，覆盖「顺序流水线 / 并行扇出 / 并行+回调聚合」三大场景。本文档按 **签名 → 行为说明 → 注意事项 → 代码演示 → 生产建议** 的统一结构展开，并在末尾给出三者对比表。

---

## 1. chain — 顺序执行（上一步 stdout → 下一步 stdin）

### 签名

```php
// TaskBuilder 静态工厂（self 链式，最后用 dispatch() 终结）
public static function chain(array $builders): self
public function dispatch(): string

// PHP 函数（底层，需自行组装 tasks_json）
function xhjob_chain(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
//   返回 chain_id，失败返回 "error: ..."

// TaskManager
public function chainState(string $chainId): ?array

// PHP 函数（状态查询）
function xhjob_chain_state(string $chain_id, ?string $name = null, ?string $data_dir = null): ?string
//   返回 ChainRecord JSON 或 null
```

### 行为说明

`chain` 把多个子任务按数组顺序串联：

- 顺序执行：第 N 步必须等第 N-1 步完成才开始；
- **数据传递**：每一步的 **stdout 原样作为下一步的 stdin**（典型 Unix 管道语义）；
- **失败中断**：任一步失败（非 0 退出 / 超时 / 假死），整条链立即终止，state 置为 `failed`，后续步骤不再执行；
- 全部成功后链 state 置为 `success`，链结果取**最后一步的 stdout**。

### 注意事项

- stdin/stdout 是**字节流**，不解析为 JSON；如需结构化传递，业务侧自行 `json_encode` / `json_decode`。
- 链中任一子任务也可单独 `persist(true)`，整体崩溃恢复按子任务粒度重派。
- `chain()` 是**静态工厂**，参数是 `TaskBuilder[]`，每个 builder 不要单独调 `dispatch()`。

### 代码演示

```php
<?php
// 三步流水线：抓数据 → 清洗 → 入库
$chainId = \Xhjob\TaskBuilder::chain([
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.example.com/raw'),
    (new \Xhjob\TaskBuilder())->shell('jq -c ".[] | select(.active)"'),
    (new \Xhjob\TaskBuilder())->shell('php /app/bin/import.php'),
])
    ->timeout(60)
    ->persist(true)
    ->dispatch();

echo "chain_id = {$chainId}\n";

// 轮询链状态
while (true) {
    $state = xhjob_chain_state($chainId);
    $rec = json_decode($state, true);
    echo "chain state = {$rec['state']}, step = {$rec['current_step']}/{$rec['total_steps']}\n";
    if (in_array($rec['state'], ['success', 'failed'], true)) break;
    usleep(500_000);
}
print_r($rec);
```

### 生产建议

- 链长建议 ≤ 10 步，过长用 `group` 拆并行。
- 每步脚本开头加 `set -euo pipefail`，避免静默失败导致后续步骤拿到空 stdin。

---

## 2. group — 并行执行 + 汇总

### 签名

```php
// TaskBuilder 静态工厂
public static function group(array $builders): self
public function dispatch(): string

// PHP 函数
function xhjob_group(string $tasks_json, ?string $name = null, ?string $data_dir = null): string
//   返回 group_id，失败返回 "error: ..."

// TaskManager
public function groupState(string $groupId): ?array

// PHP 函数（状态查询，返回 GroupRecord JSON + summary）
function xhjob_group_state(string $group_id, ?string $name = null, ?string $data_dir = null): ?string
```

### 行为说明

`group` 把多个子任务**并行**派发，所有子任务同时跑：

- 并发执行：所有子任务一次性入队，由 daemon 双线程池调度；
- 等待全部完成（success 或 failed）后，group 进入终态；
- 终态汇总规则：
  - 全部 success → `success`
  - 全部 failed → `failed`
  - 部分 failed → `partial_failed`
- `group_state` 返回的 JSON 含 `summary` 字段，含 `total / success / failed` 计数。

### 注意事项

- 并发量受 daemon 线程池容量限制（async 默认 1024，thread 默认 CPU 核数），超过会排队。
- group 不做数据传递，子任务之间**互相独立**，如需聚合结果用 `chord`。
- `partial_failed` 是 group 独有的终态，chain / chord 不会出现。

### 代码演示

```php
<?php
// 并行抓取 5 个数据源
$groupId = \Xhjob\TaskBuilder::group([
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.a.com/data'),
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.b.com/data'),
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.c.com/data'),
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.d.com/data'),
    (new \Xhjob\TaskBuilder())->shell('curl -s https://api.e.com/data'),
])
    ->timeout(30)
    ->persist(true)
    ->dispatch();

echo "group_id = {$groupId}\n";

// 轮询，关注 summary
while (true) {
    $raw = xhjob_group_state($groupId);
    $rec = json_decode($raw, true);
    $s = $rec['summary'];
    echo "group state = {$rec['state']}, success={$s['success']}/{$s['total']}, failed={$s['failed']}\n";
    if (in_array($rec['state'], ['success', 'partial_failed', 'failed'], true)) break;
    usleep(500_000);
}
print_r($rec);
```

### 生产建议

- 子任务数 > 池容量时，拆成多个 group 串行执行，避免单个 group 长时间占满池。
- 用 `summary.failed` 触发补偿：`partial_failed` 时对失败子任务重派一个新 group。

---

## 3. chord — header 并行 + callback 回调栅栏

### 签名

```php
// TaskBuilder 静态工厂
public static function chord(array $headerBuilders, self $callback): self
public function dispatch(): string

// PHP 函数
function xhjob_chord(string $header_json, string $callback_json, ?string $name = null, ?string $data_dir = null): string
//   返回 chord_id，失败返回 "error: ..."

// TaskManager
public function chordState(string $chordId): ?array

// PHP 函数（状态查询）
function xhjob_chord_state(string $chord_id, ?string $name = null, ?string $data_dir = null): ?string
```

### 行为说明

`chord` 是「并行 + 栅栏 + 回调」三段式：

1. **header 并发**：`headerBuilders` 数组中的子任务一次性并行派发（类似 group）；
2. **栅栏等待**：daemon 等待所有 header 完成；
3. **callback 触发**：仅当**所有 header 全部 success** 时，才派发 `callback` 任务；
   - callback 的 **meta 携带各 header 的结果**（按 header 顺序的数组），业务可从 meta 取出聚合；
4. **失败短路**：任一 header failed → **不触发 callback**，chord 直接进入 `partial_failed` 终态。

### 注意事项

- `chord` 的终态只有三种：`success`（header 全成功 + callback 成功）、`partial_failed`（任一 header 失败，callback 不触发）、`failed`（header 全成功但 callback 失败）。
- callback 是**单个** TaskBuilder，不是数组；它接收的是 header 结果数组，不是 stdout 管道。
- header 失败时 callback 不会被丢弃，但也不会重试 —— 如需失败补偿，在外层包 `withRetry` 或自行重派。

### 代码演示

```php
<?php
// 三个 header 并行抓数据 → 全成功后 callback 聚合入库
$chordId = \Xhjob\TaskBuilder::chord(
    // header: 三个并行抓取
    [
        (new \Xhjob\TaskBuilder())->shell('curl -s https://api.a.com/users'),
        (new \Xhjob\TaskBuilder())->shell('curl -s https://api.b.com/users'),
        (new \Xhjob\TaskBuilder())->shell('curl -s https://api.c.com/users'),
    ],
    // callback: meta 含三个 header 的结果数组
    (new \Xhjob\TaskBuilder())->shell('php /app/bin/merge-users.php')
)
    ->timeout(60)
    ->persist(true)
    ->dispatch();

echo "chord_id = {$chordId}\n";

// 轮询
while (true) {
    $raw = xhjob_chord_state($chordId);
    $rec = json_decode($raw, true);
    echo "chord state = {$rec['state']}, headers_done = {$rec['headers_done']}/{$rec['headers_total']}, callback = {$rec['callback_state']}\n";
    if (in_array($rec['state'], ['success', 'partial_failed', 'failed'], true)) break;
    usleep(500_000);
}

if ($rec['state'] === 'partial_failed') {
    echo "some header failed, callback NOT triggered\n";
    print_r($rec['header_results']);   // 看哪个 header 失败
}
```

`merge-users.php` 内读取 callback meta 的方式（业务侧约定）：

```php
<?php
// /app/bin/merge-users.php
// callback 启动时，daemon 把各 header 结果以 JSON 写入环境变量 XHJOB_CHORD_META
$meta = json_decode(getenv('XHJOB_CHORD_META') ?: '[]', true);
// $meta = [['stdout'=>'...','exit_code'=>0], ['stdout'=>'...','exit_code'=>0], ...]
$all = [];
foreach ($meta as $i => $header) {
    $users = json_decode($header['stdout'] ?? '[]', true) ?: [];
    foreach ($users as $u) $all[] = $u;
}
echo json_encode($all, JSON_UNESCAPED_UNICODE);
```

### 生产建议

- chord 是「MapReduce」的 Map 阶段（header）+ Reduce 阶段（callback），适合聚合计算。
- header 数量 ≥ 3 才有意义用 chord；只有 2 个并行任务用 group 即可。
- callback 脚本要**幂等**：daemon 崩溃重启时若 callback 已派发未完成，可能重派。

---

## 4. 三种编排的状态查询

### 签名

```php
// TaskManager（返回数组或 null）
public function chainState(string $chainId): ?array
public function groupState(string $groupId): ?array
public function chordState(string $chordId): ?array

// PHP 函数（返回 JSON 字符串或 null）
function xhjob_chain_state(string $chain_id, ?string $name = null, ?string $data_dir = null): ?string
function xhjob_group_state(string $group_id, ?string $name = null, ?string $data_dir = null): ?string
function xhjob_chord_state(string $chord_id, ?string $name = null, ?string $data_dir = null): ?string
```

### 行为说明

三者查询接口对称：

- `*State()` 返回对应的 `ChainRecord` / `GroupRecord` / `ChordRecord`；
- group 状态额外带 `summary`（计数），chord 状态额外带 `headers_done` / `headers_total` / `callback_state`；
- 不存在的 id 返回 `null`（PHP 函数）或 `null`（TaskManager），**不会抛异常**。

### 注意事项

- `xhjob_chain_state` 等返回的是 **JSON 字符串**，调用方需自行 `json_decode`；`TaskManager::chainState` 等返回的是**已解码数组**，更易用。
- 查询频率建议 ≥ 500ms，过密会给 SQLite 增加读压力（WAL 模式下读不阻塞写，但仍占连接）。
- `name` 参数用于多 daemon 隔离场景，单 daemon 默认 null 即可。

### 代码演示

```php
<?php
// 方式 A: PHP 函数（返回 JSON 字符串）
$chainState = xhjob_chain_state($chainId);
$groupState = xhjob_group_state($groupId);
$chordState = xhjob_chord_state($chordId);

if ($chainState !== null) {
    $rec = json_decode($chainState, true);
    echo "chain: state={$rec['state']}, steps={$rec['current_step']}/{$rec['total_steps']}\n";
}
if ($groupState !== null) {
    $rec = json_decode($groupState, true);
    echo "group: state={$rec['state']}, summary=" . json_encode($rec['summary']) . "\n";
}
if ($chordState !== null) {
    $rec = json_decode($chordState, true);
    echo "chord: state={$rec['state']}, callback={$rec['callback_state']}\n";
}

// 方式 B: TaskManager（返回数组，推荐）
$tm = new \Xhjob\TaskManager();
if ($c = $tm->chainState($chainId)) {
    echo "chain: state={$c['state']}\n";
}
if ($g = $tm->groupState($groupId)) {
    echo "group: state={$g['state']}, failed={$g['summary']['failed']}\n";
}
if ($h = $tm->chordState($chordId)) {
    echo "chord: state={$h['state']}, callback={$h['callback_state']}\n";
}
```

### 生产建议

- 长轮询场景用 TaskManager 数组版，省去反复 `json_decode`。
- 把编排 id（chain_id / group_id / chord_id）和任务 id 一起存到业务表，便于追溯。

---

## 5. 三者区别对比表

| 维度 | chain | group | chord |
|---|---|---|---|
| **执行顺序** | 严格顺序（step 1 → 2 → ... → N） | 全部并行 | header 全部并行，callback 在 header 全成功后单独执行 |
| **数据传递** | 上一步 stdout → 下一步 stdin（管道） | 无传递，子任务独立 | header 无传递；callback 通过 meta 收到各 header 结果数组 |
| **失败行为** | 任一步失败 → 链终止，state=`failed` | 各子任务独立失败，终态按比例 `success`/`partial_failed`/`failed` | 任一 header 失败 → **不触发 callback**，state=`partial_failed` |
| **终态集合** | `success` / `failed` | `success` / `partial_failed` / `failed` | `success` / `partial_failed` / `failed` |
| **典型适用场景** | 流水线（抓取→清洗→入库）、有依赖的串行步骤 | 扇出（批量抓取、批量通知）、无依赖并行 | MapReduce（Map=header 并行，Reduce=callback 聚合）、需要栅栏同步 |
| **结果获取** | 最后一步 stdout | 各子任务各自 result | callback 的 stdout（header 失败时无） |
| **状态查询** | `chainState` / `xhjob_chain_state` | `groupState` / `xhjob_group_state`（带 summary） | `chordState` / `xhjob_chord_state`（带 headers_done / callback_state） |
| **子任务 builder** | `TaskBuilder::chain($builders)` | `TaskBuilder::group($builders)` | `TaskBuilder::chord($headerBuilders, $callback)` |
| **底层函数** | `xhjob_chain($tasks_json)` | `xhjob_group($tasks_json)` | `xhjob_chord($header_json, $callback_json)` |

### 选型口诀

- **有依赖、要管道** → `chain`
- **无依赖、要快** → `group`
- **无依赖、但要等齐再聚合** → `chord`

### 生产建议（综合）

- 编排 id 一旦派发不可修改，业务侧把 id 与业务实体绑定存表，便于对账。
- 三种编排都支持 `persist(true)`，长耗时编排**务必**开启持久化 + `acksLate(true)`，避免 daemon 重启丢链。
- 编排嵌套目前不支持（chain 内不能直接放 group），需要嵌套时拆成多个独立编排，外层用业务脚本串接。
- 监控编排终态分布：`partial_failed` 比例升高通常意味着上游数据质量下降或下游服务抖动。
