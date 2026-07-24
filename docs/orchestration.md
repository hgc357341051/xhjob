---
title: 编排（chain/group/chord）
parent: 核心能力
nav_order: 35
---

# 编排（chain / group / chord）

Xhjob 提供三种任务编排原语，对齐 Celery 的 chain / group / chord 语义，分别覆盖 **顺序流水线**、**并行批处理**、**并行 + 回调** 三类组合场景。相关实现位于 `src/scheduler/chain.rs`、`src/scheduler/group.rs` 与 `src/scheduler/chord.rs`。

三种编排均可通过 `TaskBuilder` 的工厂方法创建并直接 `dispatch()`，也可通过 `TaskManager` 的 `createChain` / `createGroup` / `createChord` 方法创建。

---

## chain（顺序执行）

`TaskBuilder::chain(array $builders)` 将多个任务串联成一条 **顺序流水线**：

- 任务按数组顺序依次执行。
- **上一步的 stdout 作为下一步的 stdin**，形成数据传递管道。
- 任意一步失败则 **中断整条链**，剩余步骤被跳过，链状态变为 `failed`。
- 所有步骤成功后，链状态变为 `success`。

参考 Celery `chain(t1, t2, t3)`。

> 实现细节：链记录以 `TaskBuilder` JSON 列表形式持久化。每步成功后 daemon 递增 `current_step` 并派发下一步。为防止"两个并发完成回调同时读取 `current_step=N` 并各自推进到 N+2、跳过某步且重复某步"的 check-then-act 竞态，每个 chain_id 由一把独立 mutex 串行化 `advance()` 操作。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 三步流水线：echo 输出 → 转大写 → 统计字节数
// step1 的 stdout 传给 step2 的 stdin，step2 的 stdout 传给 step3 的 stdin
$chainId = TaskBuilder::chain([
    TaskBuilder::shell('echo step1-data'),
    TaskBuilder::shell('tr a-z A-Z'),   // 接收 step1 的 stdout
    TaskBuilder::shell('wc -c'),         // 接收 step2 的 stdout
])->dispatch();
```

也可通过 `TaskManager` 创建：

```php
use Xhjob\TaskBuilder;
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

$chainId = $mgr->createChain([
    TaskBuilder::shell('echo step1-data'),
    TaskBuilder::shell('tr a-z A-Z'),
    TaskBuilder::shell('wc -c'),
]);
```

---

## group（并行执行）

`TaskBuilder::group(array $builders)` 将多个任务作为一个 **并行批** 同时派发：

- 所有子任务 **并发执行**。
- 每个子任务完成后，daemon 更新其完成记录。
- 全部完成后汇总 group 结果。

状态流转：`pending` → `running` → `success`（全部成功）/ `partial_failed`（部分失败）/ `failed`（全部失败）。

参考 Celery `group(t1, t2, t3)`。

> 实现细节：group 记录以 `TaskBuilder` JSON 列表持久化。创建时 daemon 并发派发所有子任务；`group_state` 查询时实时读取各子任务的当前状态聚合得出（而非维护冗余计数器），对长生命周期 group 可接受；超大 group（>10k 任务）未来可能引入反范式完成计数器。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 并行抓取 3 个用户信息
$groupId = TaskBuilder::group([
    TaskBuilder::http('GET', 'https://api.example.com/user/1'),
    TaskBuilder::http('GET', 'https://api.example.com/user/2'),
    TaskBuilder::http('GET', 'https://api.example.com/user/3'),
])->dispatch();
```

---

## chord（并行 + 回调）

`TaskBuilder::chord(array $headerBuilders, self $callback)` 是 group 的增强版：header 并行执行，**全部成功后** 派发 callback 回调任务。

- **header**：一组并行任务，行为同 group。
- **callback**：header 全部成功后才派发的回调任务。callback 的 `meta` 被设置为一个 JSON 数组，携带每个 header 任务的结果（`{ id, result }`，含 stdout / stderr / exit_code / body / status_code）。
- **header 任一失败**（failed / cancelled / expired）→ chord 直接进入 **`partial_failed` 终态**，**callback 不被派发**。

参考 Celery `chord(header, body)`。

> 实现细节：chord 记录持久化 `header_task_ids` 与 `callback_json`。状态流转：`running` → `success`（全部 header 成功 → 派发 callback）/ `partial_failed`（任一 header 失败 → 不派发 callback）。为防止"两个并发 header 完成回调同时通过 all-succeeded 检查、重复插入并派发 callback"，每个 chord_id 由一把独立 mutex 串行化 `refresh_state`。

> 空 chord（header 为空数组）遵循 Celery `chord([])` 的 no-op 语义：立即转 `success` 终态，不派发 callback。

### 代码演示

```php
use Xhjob\TaskBuilder;

// 3 个 header 并行预处理，全部成功后执行汇总 callback
$chordId = TaskBuilder::chord(
    // header：3 个并行任务
    [
        TaskBuilder::shell('echo header-1'),
        TaskBuilder::shell('echo header-2'),
        TaskBuilder::shell('echo header-3'),
    ],
    // callback：header 全部成功后执行
    TaskBuilder::shell('echo callback-done')
        ->withMeta('{"source":"chord"}')
)->dispatch();
```

---

## 状态查询

三种编排均提供对应的状态查询接口，返回编排记录的当前状态与元信息：

| 编排 | 查询方法（TaskManager） | 查询方法（PHP 函数） |
|------|------------------------|---------------------|
| chain | `chainState($id)` | `xhjob_chain_state($id)` |
| group | `groupState($id)` | `xhjob_group_state($id)` |
| chord | `chordState($id)` | `xhjob_chord_state($id)` |

### 返回字段

返回的 state 对象通常包含以下字段：

- `id`：编排记录 ID。
- `state`：编排状态（`pending` / `running` / `success` / `failed` / `partial_failed`）。
- `created_at` / `updated_at`：创建与最后更新时间戳。
- chain：`current_step`（当前执行到第几步）。
- group：各子任务的完成情况（`completed` / `failed` / `total` 等聚合字段）。
- chord：`header_task_ids`（header 任务 ID 列表）、`callback_json`（回调任务配置）、`callback_task_id`（回调任务被派发后的 ID）。

### 代码演示

```php
use Xhjob\TaskManager;

$mgr = new TaskManager('default', '/var/lib/xhjob');

// 查询 chain 状态
$chainState = $mgr->chainState($chainId);
// $chainState['state']         // 'running' / 'success' / 'failed'
// $chainState['current_step']  // 当前步骤序号

// 查询 group 状态
$groupState = $mgr->groupState($groupId);
// $groupState['state']      // 'running' / 'success' / 'partial_failed' / 'failed'

// 查询 chord 状态
$chordState = $mgr->chordState($chordId);
// $chordState['state']            // 'running' / 'success' / 'partial_failed'
// $chordState['header_task_ids']  // header 任务 ID 数组
// $chordState['callback_task_id'] // 回调任务 ID（派发后才有）
```
