<?php
/**
 * xhjob 任务管理门面
 *
 * 封装 xhjob_* 扩展函数，提供任务 CRUD、启停、重排、列表、详情、
 * 事件查询、chain/group 等操作的统一入口。
 *
 * 字符串返回失败的调用（dispatch / chain / group / list / events）会抛出
 * InvalidTaskConfigException；布尔与 null 返回的操作按原语义返回。
 *
 * 用法：
 *   $tm = new TaskManager('default', '/var/lib/xhjob');
 *   $id = $tm->create(TaskBuilder::shell('echo hi')->cron('* * * * *'));
 *   $tm->pause($id);
 *   $state = $tm->state($id);
 *   $tm->remove($id);
 */

/**
 * 任务管理门面类
 */
class TaskManager
{
    /** @var string 服务名 */
    protected string $name;

    /** @var string|null 数据目录路径 */
    protected ?string $dataDir;

    /**
     * 构造函数
     *
     * @param string      $name    服务名，默认 "default"
     * @param string|null $dataDir 数据目录，null 表示使用平台默认路径
     */
    public function __construct(string $name = 'default', ?string $dataDir = null)
    {
        $this->name = $name;
        $this->dataDir = ($dataDir === '') ? null : $dataDir;
    }

    /**
     * 创建单个任务
     *
     * @param TaskBuilder $b 任务构建器
     * @return string 任务 ID
     * @throws InvalidTaskConfigException 派发失败时抛出
     */
    public function create(TaskBuilder $b): string
    {
        $result = xhjob_dispatch($b->toJson(), $this->name, $this->dataDir);
        return $this->parseResponse($result, '创建任务');
    }

    /**
     * 创建任务链（顺序流水线）
     *
     * 每个子任务的 stdout 作为下一个子任务的输入，任一步失败则中断。
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return string 链 ID
     * @throws InvalidTaskConfigException 创建失败时抛出
     */
    public function createChain(array $builders): string
    {
        $tasks = array_map(function ($b) {
            return $b->toArray();
        }, $builders);
        $result = xhjob_chain(json_encode($tasks), $this->name, $this->dataDir);
        return $this->parseResponse($result, '创建任务链');
    }

    /**
     * 创建任务组（并行批处理）
     *
     * 所有子任务并行派发，独立执行。
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return string 组 ID
     * @throws InvalidTaskConfigException 创建失败时抛出
     */
    public function createGroup(array $builders): string
    {
        $tasks = array_map(function ($b) {
            return $b->toArray();
        }, $builders);
        $result = xhjob_group(json_encode($tasks), $this->name, $this->dataDir);
        return $this->parseResponse($result, '创建任务组');
    }

    /**
     * 更新任务定义（先移除再以相同 ID 重新创建）
     *
     * 旧任务被 remove，新任务以相同 ID 插入（replaceExisting=true），
     * execution_count 重置为 0。
     *
     * @param string      $id 任务 ID
     * @param TaskBuilder $b  新的任务构建器
     * @return string 任务 ID
     * @throws InvalidTaskConfigException 创建失败时抛出
     */
    public function update(string $id, TaskBuilder $b): string
    {
        $this->remove($id);
        $b->withId($id)->replaceExisting(true);
        return $this->create($b);
    }

    /**
     * 获取任务完整定义
     *
     * 返回包含全部配置字段（cron / retry_max / timeout / priority 等）
     * 的关联数组，区别于 state() 返回的精简 StateInfo 视图。
     *
     * @param string $id 任务 ID
     * @return array|null 任务定义数组，不存在时返回 null
     */
    public function get(string $id): ?array
    {
        $result = xhjob_get($id, $this->name, $this->dataDir);
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * 列出任务
     *
     * @param string|null $stateFilter 状态过滤（如 'PENDING'、'SUCCESS'）
     * @param string|null $tag         标签过滤
     * @return array 任务列表
     * @throws InvalidTaskConfigException 查询失败时抛出
     */
    public function list(?string $stateFilter = null, ?string $tag = null): array
    {
        $result = xhjob_list($this->name, $stateFilter, $tag, $this->dataDir);
        $parsed = $this->parseResponse($result, '列表查询');
        $decoded = json_decode($parsed, true);
        return is_array($decoded) ? $decoded : [];
    }

    /**
     * 查询任务状态
     *
     * @param string $id 任务 ID
     * @return array 关联数组（state / attempts / created_at / paused 等）
     */
    public function state(string $id): array
    {
        $result = xhjob_state($id, $this->name, $this->dataDir);
        return $this->parseStateArray($result);
    }

    /**
     * 查询任务执行结果
     *
     * @param string $id 任务 ID
     * @return array 关联数组（body / stdout / exit_code 等）
     */
    public function result(string $id): array
    {
        $result = xhjob_result($id, $this->name, $this->dataDir);
        return $this->parseStateArray($result);
    }

    /**
     * 停止任务（取消）
     *
     * Pending 任务进入 Cancelled 终态；Running 任务置 cancel_requested，
     * 执行完成后不再重试。
     *
     * @param string $id 任务 ID
     * @return bool 是否成功
     */
    public function stop(string $id): bool
    {
        return xhjob_cancel($id, $this->name, $this->dataDir);
    }

    /**
     * 重启任务（重新入队）
     *
     * 终态任务被 requeue 为 Pending，next_fire 设为 now，立即触发。
     *
     * @param string $id 任务 ID
     * @return bool 是否成功
     */
    public function restart(string $id): bool
    {
        return xhjob_requeue($id, $this->name, $this->dataDir);
    }

    /**
     * 暂停任务
     *
     * 任务定义保留但 cron 触发器不再触发。
     *
     * @param string $id 任务 ID
     * @return bool 是否成功
     */
    public function pause(string $id): bool
    {
        return xhjob_pause($id, $this->name, $this->dataDir);
    }

    /**
     * 恢复已暂停的任务
     *
     * @param string $id 任务 ID
     * @return bool 是否成功
     */
    public function resume(string $id): bool
    {
        return xhjob_resume($id, $this->name, $this->dataDir);
    }

    /**
     * 移除任务定义
     *
     * 从 store 中删除任务定义，不影响正在运行的实例。
     *
     * @param string $id 任务 ID
     * @return bool 是否成功
     */
    public function remove(string $id): bool
    {
        return xhjob_remove($id, $this->name, $this->dataDir);
    }

    /**
     * 查询任务事件日志
     *
     * 返回该任务的全部事件记录（started / succeeded / failed / cancelled 等），
     * 每条事件含 ts + event_type + payload。
     *
     * @param string $id       任务 ID
     * @param int    $sinceTs  起始时间戳（0 表示全部）
     * @return array 事件数组
     * @throws InvalidTaskConfigException 查询失败时抛出
     */
    public function logs(string $id, int $sinceTs = 0): array
    {
        $result = xhjob_events($sinceTs, $id, $this->name, $this->dataDir);
        $parsed = $this->parseResponse($result, '日志查询');
        $decoded = json_decode($parsed, true);
        return is_array($decoded) ? $decoded : [];
    }

    /**
     * 重新调度 cron 任务
     *
     * 在线修改 cron 表达式，保留任务状态 / execution_count / attempts / meta，
     * 仅更新 cron 和 next_fire。
     *
     * @param string $id   任务 ID
     * @param string $cron 新的 cron 表达式
     * @return bool 是否成功
     */
    public function reschedule(string $id, string $cron): bool
    {
        return xhjob_reschedule($id, $cron, $this->name, $this->dataDir);
    }

    /**
     * 查询任务链状态
     *
     * @param string $chainId 链 ID
     * @return array|null 链状态数组（含 current_step / state / tasks 等），不存在时返回 null
     */
    public function chainState(string $chainId): ?array
    {
        $result = xhjob_chain_state($chainId, $this->name, $this->dataDir);
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * 查询任务组状态
     *
     * @param string $groupId 组 ID
     * @return array|null 组状态数组（含 summary / state / tasks 等），不存在时返回 null
     */
    public function groupState(string $groupId): ?array
    {
        $result = xhjob_group_state($groupId, $this->name, $this->dataDir);
        if ($result === null) {
            return null;
        }
        $decoded = json_decode($result, true);
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * 轮询等待任务进入指定状态
     *
     * 每 200ms 轮询一次 state()，直到匹配或超时。
     *
     * @param string $id            任务 ID
     * @param string $expectedState 期望状态（如 'SUCCESS'、'FAILED'）
     * @param int    $timeoutSec    超时秒数，默认 30
     * @return bool 是否在超时前达到指定状态
     */
    public function waitForState(string $id, string $expectedState, int $timeoutSec = 30): bool
    {
        $deadline = time() + $timeoutSec;
        while (time() < $deadline) {
            $state = $this->state($id);
            if (($state['state'] ?? 'UNKNOWN') === $expectedState) {
                return true;
            }
            usleep(200000); // 200ms
        }
        return false;
    }

    /**
     * 轮询等待任务产生结果
     *
     * 每 200ms 轮询一次 result()，直到有结果或超时。
     *
     * @param string $id         任务 ID
     * @param int    $timeoutSec 超时秒数，默认 30
     * @return array|null 结果数组，超时返回 null
     */
    public function waitForResult(string $id, int $timeoutSec = 30): ?array
    {
        $deadline = time() + $timeoutSec;
        while (time() < $deadline) {
            $result = $this->result($id);
            if (!isset($result['error'])) {
                return $result;
            }
            usleep(200000); // 200ms
        }
        return null;
    }

    // =====================================================================
    // 辅助方法
    // =====================================================================

    /**
     * 解析响应字符串，失败时抛出异常
     *
     * xhjob_dispatch / xhjob_chain / xhjob_group / xhjob_list / xhjob_events
     * 等函数在失败时返回 "error: xxx" 字符串，本方法检测该前缀并抛出异常。
     *
     * @param string $result  原始响应
     * @param string $context 操作上下文描述（用于异常消息）
     * @return string 成功时的响应
     * @throws InvalidTaskConfigException 响应以 "error:" 开头时抛出
     */
    protected function parseResponse($result, string $context = 'operation'): string
    {
        if (is_string($result) && strpos($result, 'error:') === 0) {
            $msg = trim(substr($result, 6));
            throw new InvalidTaskConfigException("{$context} 失败: {$msg}");
        }
        return $result;
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
