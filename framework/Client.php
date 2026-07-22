<?php
/**
 * xhjob 跨环境客户端
 *
 * 复用 xhjob_* 扩展函数（CLI 和 FPM 环境均可调用），增加：
 *  - 超时控制（整体重试截止时间）
 *  - 重连配置（retry 次数 + 延迟）
 *  - 统一异常封装（失败抛 IpcException）
 *
 * 与 TaskManager 接口对齐，便于替换。区别在于：
 *  - TaskManager 对字符串错误抛 InvalidTaskConfigException，布尔/null 按原语义返回
 *  - Client 对所有失败（字符串 "error:" / 布尔 false / 数组 error 键）统一抛 IpcException
 *
 * 用法：
 *   $c = new Client('default', '/var/lib/xhjob', 5);
 *   $c->retry(3, 200);  // 重试 3 次，间隔 200ms
 *   $id = $c->dispatch(TaskBuilder::shell('echo hi'));
 *   $state = $c->state($id);
 */

/**
 * 跨环境客户端类
 */
class Client
{
    /** @var string 服务名 */
    protected string $name;

    /** @var string|null 数据目录路径 */
    protected ?string $dataDir;

    /** @var int 整体超时秒数（重试截止时间） */
    protected int $timeoutSec;

    /** @var int 重试次数（0 = 不重试，仅尝试一次） */
    protected int $retryCount = 0;

    /** @var int 重试间隔（毫秒） */
    protected int $retryDelayMs = 100;

    /**
     * 构造函数
     *
     * @param string      $name       服务名，默认 "default"
     * @param string|null $dataDir    数据目录，null 表示使用平台默认路径
     * @param int         $timeoutSec 超时秒数，默认 5
     */
    public function __construct(string $name = 'default', ?string $dataDir = null, int $timeoutSec = 5)
    {
        $this->name = $name;
        $this->dataDir = ($dataDir === '') ? null : $dataDir;
        $this->timeoutSec = max(1, $timeoutSec);
    }

    /**
     * 派发单个任务
     *
     * @param TaskBuilder $b 任务构建器
     * @return string 任务 ID
     * @throws IpcException 派发失败时抛出
     */
    public function dispatch(TaskBuilder $b): string
    {
        $json = $b->toJson();
        return $this->withRetry(function () use ($json) {
            return xhjob_dispatch($json, $this->name, $this->dataDir);
        }, '派发任务');
    }

    /**
     * 派发任务链（顺序流水线）
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return string 链 ID
     * @throws IpcException 创建失败时抛出
     */
    public function dispatchChain(array $builders): string
    {
        $tasks = array_map(function ($b) {
            return $b->toArray();
        }, $builders);
        $json = json_encode($tasks);
        return $this->withRetry(function () use ($json) {
            return xhjob_chain($json, $this->name, $this->dataDir);
        }, '派发任务链');
    }

    /**
     * 派发任务组（并行批处理）
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return string 组 ID
     * @throws IpcException 创建失败时抛出
     */
    public function dispatchGroup(array $builders): string
    {
        $tasks = array_map(function ($b) {
            return $b->toArray();
        }, $builders);
        $json = json_encode($tasks);
        return $this->withRetry(function () use ($json) {
            return xhjob_group($json, $this->name, $this->dataDir);
        }, '派发任务组');
    }

    /**
     * 查询任务状态
     *
     * @param string $id 任务 ID
     * @return array 关联数组（state / attempts / created_at 等）
     * @throws IpcException 查询失败时抛出
     */
    public function state(string $id): array
    {
        $raw = $this->withRetry(function () use ($id) {
            return xhjob_state($id, $this->name, $this->dataDir);
        }, '查询状态');
        return $this->parseStateArray($raw);
    }

    /**
     * 查询任务执行结果
     *
     * @param string $id 任务 ID
     * @return array 关联数组（body / stdout / exit_code 等）
     * @throws IpcException 查询失败时抛出
     */
    public function result(string $id): array
    {
        $raw = $this->withRetry(function () use ($id) {
            return xhjob_result($id, $this->name, $this->dataDir);
        }, '查询结果');
        return $this->parseStateArray($raw);
    }

    /**
     * 获取任务完整定义
     *
     * 注意：xhjob_get 在任务不存在或 daemon 不可达时均返回 null，
     * 因此本方法不抛异常，返回 null 表示未找到。
     *
     * @param string $id 任务 ID
     * @return array|null 任务定义数组，不存在时返回 null
     */
    public function get(string $id): ?array
    {
        $result = $this->withRetry(function () use ($id) {
            return xhjob_get($id, $this->name, $this->dataDir);
        }, '获取任务');
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * 列出任务
     *
     * @param string|null $stateFilter 状态过滤
     * @param string|null $tag         标签过滤
     * @return array 任务列表
     * @throws IpcException 查询失败时抛出
     */
    public function list(?string $stateFilter = null, ?string $tag = null): array
    {
        $result = $this->withRetry(function () use ($stateFilter, $tag) {
            return xhjob_list($this->name, $stateFilter, $tag, $this->dataDir);
        }, '列表查询');
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : [];
    }

    /**
     * 查询事件
     *
     * @param int         $sinceTs 起始时间戳（0 表示全部）
     * @param string|null $taskId  任务 ID 过滤，null 表示不过滤
     * @return array 事件数组
     * @throws IpcException 查询失败时抛出
     */
    public function events(int $sinceTs = 0, ?string $taskId = null): array
    {
        $result = $this->withRetry(function () use ($sinceTs, $taskId) {
            return xhjob_events($sinceTs, $taskId, $this->name, $this->dataDir);
        }, '事件查询');
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : [];
    }

    /**
     * 取消任务
     *
     * @param string $id 任务 ID
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function cancel(string $id): bool
    {
        return $this->withRetry(function () use ($id) {
            return xhjob_cancel($id, $this->name, $this->dataDir);
        }, '取消任务');
    }

    /**
     * 暂停任务
     *
     * @param string $id 任务 ID
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function pause(string $id): bool
    {
        return $this->withRetry(function () use ($id) {
            return xhjob_pause($id, $this->name, $this->dataDir);
        }, '暂停任务');
    }

    /**
     * 恢复任务
     *
     * @param string $id 任务 ID
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function resume(string $id): bool
    {
        return $this->withRetry(function () use ($id) {
            return xhjob_resume($id, $this->name, $this->dataDir);
        }, '恢复任务');
    }

    /**
     * 重新入队任务
     *
     * @param string $id 任务 ID
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function requeue(string $id): bool
    {
        return $this->withRetry(function () use ($id) {
            return xhjob_requeue($id, $this->name, $this->dataDir);
        }, '重新入队');
    }

    /**
     * 重新调度 cron 任务
     *
     * @param string $id   任务 ID
     * @param string $cron 新的 cron 表达式
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function reschedule(string $id, string $cron): bool
    {
        return $this->withRetry(function () use ($id, $cron) {
            return xhjob_reschedule($id, $cron, $this->name, $this->dataDir);
        }, '重新调度');
    }

    /**
     * 移除任务
     *
     * @param string $id 任务 ID
     * @return bool 成功返回 true
     * @throws IpcException 失败时抛出
     */
    public function remove(string $id): bool
    {
        return $this->withRetry(function () use ($id) {
            return xhjob_remove($id, $this->name, $this->dataDir);
        }, '移除任务');
    }

    /**
     * 查询任务链状态
     *
     * 注意：xhjob_chain_state 在链不存在或 daemon 不可达时均返回 null，
     * 因此本方法不抛异常，返回 null 表示未找到。
     *
     * @param string $chainId 链 ID
     * @return array|null 链状态数组，不存在时返回 null
     */
    public function chainState(string $chainId): ?array
    {
        $result = $this->withRetry(function () use ($chainId) {
            return xhjob_chain_state($chainId, $this->name, $this->dataDir);
        }, '查询链状态');
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * 查询任务组状态
     *
     * 注意：xhjob_group_state 在组不存在或 daemon 不可达时均返回 null，
     * 因此本方法不抛异常，返回 null 表示未找到。
     *
     * @param string $groupId 组 ID
     * @return array|null 组状态数组，不存在时返回 null
     */
    public function groupState(string $groupId): ?array
    {
        $result = $this->withRetry(function () use ($groupId) {
            return xhjob_group_state($groupId, $this->name, $this->dataDir);
        }, '查询组状态');
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    // =====================================================================
    // 链式配置方法
    // =====================================================================

    /**
     * 配置重试参数
     *
     * @param int $n       重试次数（0 = 不重试）
     * @param int $delayMs 重试间隔（毫秒），默认 100
     * @return $this 支持链式调用
     */
    public function retry(int $n, int $delayMs = 100): self
    {
        $this->retryCount = max(0, $n);
        $this->retryDelayMs = max(0, $delayMs);
        return $this;
    }

    /**
     * 设置超时秒数
     *
     * @param int $sec 超时秒数
     * @return $this 支持链式调用
     */
    public function setTimeout(int $sec): self
    {
        $this->timeoutSec = max(1, $sec);
        return $this;
    }

    // =====================================================================
    // 内部实现
    // =====================================================================

    /**
     * 带重试的调用包装
     *
     * 检测返回值中的错误信号（"error:" 前缀 / false / 数组 error 键），
     * 按配置重试，耗尽后抛出 IpcException。
     *
     * @param callable $fn      返回 xhjob_* 调用结果的闭包
     * @param string   $context 操作上下文描述
     * @return mixed 成功时的返回值
     * @throws IpcException 重试耗尽后抛出
     */
    protected function withRetry(callable $fn, string $context)
    {
        $lastError = null;
        $deadline = microtime(true) + $this->timeoutSec;

        for ($attempt = 0; $attempt <= $this->retryCount; $attempt++) {
            // 非首次尝试前延迟 + 超时检查
            if ($attempt > 0) {
                if (microtime(true) >= $deadline) {
                    break;
                }
                usleep($this->retryDelayMs * 1000);
            }

            $result = $fn();
            $error = $this->extractError($result);

            if ($error !== null) {
                $lastError = $error;
                continue;
            }

            return $result;
        }

        $retryInfo = $this->retryCount > 0
            ? "（已重试 {$this->retryCount} 次）"
            : '';
        $errorInfo = $lastError !== null ? ": {$lastError}" : '';
        throw new IpcException("{$context} 失败{$retryInfo}{$errorInfo}");
    }

    /**
     * 从返回值中提取错误信息
     *
     * @param mixed $result xhjob_* 函数返回值
     * @return string|null 错误信息，无错误时返回 null
     */
    protected function extractError($result): ?string
    {
        // 字符串: "error: xxx"
        if (is_string($result) && strpos($result, 'error:') === 0) {
            return trim(substr($result, 6));
        }

        // 布尔: false 表示失败
        if ($result === false) {
            return '操作返回 false';
        }

        // 数组: 检查 error 键（兼容 [[k,v],...] 与关联数组）
        if (is_array($result)) {
            $assoc = $this->parseStateArray($result);
            if (isset($assoc['error'])) {
                return (string)$assoc['error'];
            }
        }

        // null 是有效返回（表示未找到），不视为错误
        return null;
    }

    /**
     * 将 [[key, value], ...] 对数组转为关联数组
     *
     * xhjob_state / xhjob_result 返回 Vec<(String, String)>，在 PHP 中
     * 可能表现为 [[k, v], ...] 对数组或已转换的关联数组。本方法兼容两种格式。
     *
     * @param array $pairs 键值对数组
     * @return array 关联数组
     */
    protected function parseStateArray(array $pairs): array
    {
        if (empty($pairs)) {
            return [];
        }

        $first = reset($pairs);

        // 首元素是 2 元素数组 → [[key, value], ...] 格式
        if (is_array($first) && count($first) === 2) {
            $result = [];
            foreach ($pairs as $pair) {
                if (is_array($pair) && count($pair) === 2) {
                    $result[$pair[0]] = $pair[1];
                }
            }
            return $result;
        }

        // 已经是关联数组，直接返回
        return $pairs;
    }
}
