<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 任务管理门面
// +----------------------------------------------------------------------
// | 封装 xhjob_dispatch / xhjob_state / xhjob_result / xhjob_list 等任务
// | 相关函数，提供 create / state / result / stop / restart / waitForState
// | 等高层 API
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob;

use Xhjob\Exception\InvalidTaskConfigException;
use Xhjob\Exception\ServiceNotRunningException;
use Xhjob\Exception\TaskNotFoundException;

/**
 * 任务管理门面
 *
 * 封装任务相关 xhjob 函数，提供高层 API：
 *   - create / createChain / createGroup / createChord：创建任务
 *   - get / list / state / result / logs：查询任务
 *   - stop / restart / pause / resume / remove / reschedule：控制任务
 *   - chainState / groupState / chordState：查询链 / 组 / chord 状态
 *   - waitForState / waitForResult：轮询等待
 *
 * 用法：
 *   $mgr = new TaskManager('default', '/var/lib/xhjob');
 *   $id = $mgr->create(TaskBuilder::shell('echo hi'));
 *   $mgr->waitForState($id, 'success', 10);
 *   $r = $mgr->result($id);
 */
class TaskManager
{
    /** @var string 服务名 */
    private $name;

    /** @var string|null 数据目录 */
    private $dataDir;

    /**
     * 构造方法
     *
     * @param string      $name    服务名
     * @param string|null $dataDir 数据目录
     */
    public function __construct(string $name = 'default', ?string $dataDir = null)
    {
        $this->name    = $name;
        $this->dataDir = $dataDir;
    }

    /**
     * 获取服务名
     *
     * @return string
     */
    public function getName(): string
    {
        return $this->name;
    }

    /**
     * 获取数据目录
     *
     * @return string|null
     */
    public function getDataDir(): ?string
    {
        return $this->dataDir;
    }

    // -----------------------------------------------------------------
    // 创建任务
    // -----------------------------------------------------------------

    /**
     * 创建单个任务
     *
     * @param TaskBuilder $b 任务构建器
     *
     * @return string task_id
     *
     * @throws InvalidTaskConfigException 当配置非法或 daemon 返回 error
     */
    public function create(TaskBuilder $b): string
    {
        $result = xhjob_dispatch($b->toJson(), $this->name, $this->dataDir);
        return $this->parseResponse($result, 'dispatch');
    }

    /**
     * 创建任务链
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return string chain_id
     *
     * @throws InvalidTaskConfigException
     */
    public function createChain(array $builders): string
    {
        $tasksJson = json_encode(array_map(function (TaskBuilder $b) {
            return $b->toArray();
        }, $builders));
        $result = xhjob_chain($tasksJson, $this->name, $this->dataDir);
        return $this->parseResponse($result, 'chain');
    }

    /**
     * 创建任务组
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return string group_id
     *
     * @throws InvalidTaskConfigException
     */
    public function createGroup(array $builders): string
    {
        $tasksJson = json_encode(array_map(function (TaskBuilder $b) {
            return $b->toArray();
        }, $builders));
        $result = xhjob_group($tasksJson, $this->name, $this->dataDir);
        return $this->parseResponse($result, 'group');
    }

    /**
     * 创建 chord（header + body 回调）
     *
     * dispatch 时调用 xhjob_chord，并行执行所有 header 任务，
     * 全部成功后执行 callback 任务，callback 的 meta 携带所有 header 结果。
     * 任一 header 失败时 chord 转 partial_failed 终态，不派发 callback。
     * 参考 Celery chord。
     *
     * @param array       $headerBuilders TaskBuilder 实例数组（header）
     * @param TaskBuilder $callback       回调 TaskBuilder（body）
     *
     * @return string chord_id
     *
     * @throws InvalidTaskConfigException
     */
    public function createChord(array $headerBuilders, TaskBuilder $callback): string
    {
        $headerJson = json_encode(array_map(function (TaskBuilder $b) {
            return $b->toArray();
        }, $headerBuilders));
        $callbackJson = $callback->toJson();
        $result = xhjob_chord($headerJson, $callbackJson, $this->name, $this->dataDir);
        return $this->parseResponse($result, 'chord');
    }

    /**
     * 更新任务（先 remove 再以指定 id 重建）
     *
     * @param string      $id 旧任务 ID
     * @param TaskBuilder $b  新任务构建器
     *
     * @return string 新任务 ID
     *
     * @throws InvalidTaskConfigException
     */
    public function update(string $id, TaskBuilder $b): string
    {
        // 尝试删除旧任务（不存在不视为错误）
        try {
            $this->remove($id);
        } catch (TaskNotFoundException $e) {
            // 旧任务不存在时直接忽略
        }
        // 以旧 id 重建（启用 replace_existing 以防 daemon 中残留同 id）
        $b->withId($id)->replaceExisting(true);
        return $this->create($b);
    }

    // -----------------------------------------------------------------
    // 查询任务
    // -----------------------------------------------------------------

    /**
     * 获取任务详情
     *
     * @param string $id 任务 ID
     *
     * @return array|null 不存在时返回 null
     *
     * @throws ServiceNotRunningException daemon 不可达时
     */
    public function get(string $id): ?array
    {
        $json = xhjob_get($id, $this->name, $this->dataDir);
        if ($json === null || $json === '') {
            return null;
        }
        // 监控类返回 "error: ..." 表示 daemon 不可达
        if (strncmp($json, 'error:', 6) === 0) {
            throw new ServiceNotRunningException(
                'get 失败：' . trim(substr($json, 6))
                . " (id=$id, name={$this->name})"
            );
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : null;
    }

    /**
     * 列出任务
     *
     * @param string|null $stateFilter 状态过滤，如 'pending' / 'running' / 'success'
     * @param string|null $tag          标签过滤
     *
     * @return array 任务数组
     *
     * @throws ServiceNotRunningException daemon 不可达时
     */
    public function list(?string $stateFilter = null, ?string $tag = null): array
    {
        $json = xhjob_list($this->name, $stateFilter, $tag, $this->dataDir);
        if (strncmp($json, 'error:', 6) === 0) {
            throw new ServiceNotRunningException(
                'list 失败：' . trim(substr($json, 6))
                . " (name={$this->name})"
            );
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : [];
    }

    /**
     * 查询任务状态（关联数组）
     *
     * @param string $id 任务 ID
     *
     * @return array 包含 state / attempts / created_at 等字段
     *
     * @throws ServiceNotRunningException daemon 不可达或任务不存在时
     */
    public function state(string $id): array
    {
        $raw = xhjob_state($id, $this->name, $this->dataDir);
        $parsed = $this->parseStateArray($raw);
        // 原生函数在 daemon 不可达 / 任务不存在时返回带 'error' 键的数组。
        // 不再静默吞错，按 docblock 契约抛出异常。
        if (isset($parsed['error'])) {
            throw new ServiceNotRunningException(
                'state 失败：' . $parsed['error']
                . " (id=$id, name={$this->name})"
            );
        }
        return $parsed;
    }

    /**
     * 查询任务执行结果
     *
     * @param string $id 任务 ID
     *
     * @return array 包含 stdout / stderr / exit_code 字段；
     *               当结果不存在时（ignoreResult=true 或任务未产出输出），
     *               返回带 'error' 键的数组，调用方可通过 isset($r['error']) 判断。
     *               使用 state() 检查 daemon 可达性（其会抛异常）。
     */
    public function result(string $id): array
    {
        $raw = xhjob_result($id, $this->name, $this->dataDir);
        $parsed = $this->parseStateArray($raw);
        // 原生函数在结果不存在时（ignoreResult=true / 任务未产出输出）
        // 返回带 'error' 键的数组。这是预期的业务条件，不抛异常；
        // 调用方通过 isset($r['error']) 或 empty($r['stdout']) 判断。
        return $parsed;
    }

    /**
     * 查询任务事件日志
     *
     * @param string $id      任务 ID
     * @param int    $sinceTs 起始时间戳（Unix 秒），默认 0 = 全部
     *
     * @return array 事件数组
     *
     * @throws ServiceNotRunningException daemon 不可达时
     */
    public function logs(string $id, int $sinceTs = 0): array
    {
        $json = xhjob_events($sinceTs, $id, $this->name, $this->dataDir);
        if (strncmp($json, 'error:', 6) === 0) {
            throw new ServiceNotRunningException(
                'logs 失败：' . trim(substr($json, 6))
                . " (id=$id, name={$this->name})"
            );
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : [];
    }

    // -----------------------------------------------------------------
    // 控制任务
    // -----------------------------------------------------------------

    /**
     * 停止（取消）任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function stop(string $id): bool
    {
        return (bool) xhjob_cancel($id, $this->name, $this->dataDir);
    }

    /**
     * 重启（重新入队）任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function restart(string $id): bool
    {
        return (bool) xhjob_requeue($id, $this->name, $this->dataDir);
    }

    /**
     * 暂停任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function pause(string $id): bool
    {
        return (bool) xhjob_pause($id, $this->name, $this->dataDir);
    }

    /**
     * 恢复任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function resume(string $id): bool
    {
        return (bool) xhjob_resume($id, $this->name, $this->dataDir);
    }

    /**
     * 删除任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     *
     * @throws TaskNotFoundException 当任务不存在时
     */
    public function remove(string $id): bool
    {
        $ok = xhjob_remove($id, $this->name, $this->dataDir);
        if (!$ok) {
            throw new TaskNotFoundException(
                "任务不存在或删除失败 (id=$id, name={$this->name})"
            );
        }
        return true;
    }

    /**
     * 重新调度任务（修改 cron 表达式）
     *
     * @param string $id   任务 ID
     * @param string $cron 新的 cron 表达式
     *
     * @return bool
     */
    public function reschedule(string $id, string $cron): bool
    {
        return (bool) xhjob_reschedule($id, $cron, $this->name, $this->dataDir);
    }

    // -----------------------------------------------------------------
    // 链 / 组状态
    // -----------------------------------------------------------------

    /**
     * 查询任务链状态
     *
     * @param string $chainId 链 ID
     *
     * @return array|null 不存在时返回 null
     */
    public function chainState(string $chainId): ?array
    {
        $json = xhjob_chain_state($chainId, $this->name, $this->dataDir);
        if ($json === null || $json === '') {
            return null;
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : null;
    }

    /**
     * 查询任务组状态
     *
     * @param string $groupId 组 ID
     *
     * @return array|null 不存在时返回 null
     */
    public function groupState(string $groupId): ?array
    {
        $json = xhjob_group_state($groupId, $this->name, $this->dataDir);
        if ($json === null || $json === '') {
            return null;
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : null;
    }

    /**
     * 查询 chord 状态
     *
     * 返回 ChordRecord 关联数组，包含 id / header_task_ids /
     * callback_json / callback_task_id / state / created_at /
     * updated_at 字段。chord 不存在或 daemon 不可达时返回 null。
     *
     * @param string $chordId chord ID
     *
     * @return array|null
     */
    public function chordState(string $chordId): ?array
    {
        $json = xhjob_chord_state($chordId, $this->name, $this->dataDir);
        if ($json === null || $json === '') {
            return null;
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : null;
    }

    // -----------------------------------------------------------------
    // 进度上报 / 事件拉取 / 聚合检查
    // -----------------------------------------------------------------

    /**
     * 上报任务进度
     *
     * 参考 Celery update_state(state='PROGRESS', meta=...)。
     *
     * @param string      $id      任务 ID
     * @param int         $percent 进度百分比 0-100
     * @param string|null $meta    任意 JSON 元数据
     *
     * @return bool
     */
    public function reportProgress(string $id, int $percent, ?string $meta = null): bool
    {
        $result = xhjob_report_progress($id, $percent, $meta, $this->name, $this->dataDir);
        return is_bool($result) ? $result : (bool) $result;
    }

    /**
     * 拉取任务事件
     *
     * @param int         $sinceTs   起始时间戳（Unix 秒），默认 0 = 全部
     * @param string|null $eventType 事件类型过滤（started/succeeded/failed/...）
     *
     * @return array 事件数组
     */
    public function pullEvents(int $sinceTs = 0, ?string $eventType = null): array
    {
        $json = xhjob_pull_events($sinceTs, $eventType, $this->name, $this->dataDir);
        if (strncmp($json, 'error:', 6) === 0) {
            return [];
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : [];
    }

    /**
     * 聚合检查 daemon 状态
     *
     * @param string $mode 查询模式：active / registered / scheduled / stats（默认）
     *
     * @return array
     */
    public function inspect(string $mode = 'stats'): array
    {
        $json = xhjob_inspect($mode, $this->name, $this->dataDir);
        if (strncmp($json, 'error:', 6) === 0) {
            return [];
        }
        $data = json_decode($json, true);
        return is_array($data) ? $data : [];
    }

    // -----------------------------------------------------------------
    // 轮询等待
    // -----------------------------------------------------------------

    /**
     * 轮询等待任务进入期望状态
     *
     * @param string $id            任务 ID
     * @param string $expectedState 期望状态（pending/running/success/...）
     * @param int    $timeoutSec    超时秒数
     *
     * @return bool 是否在超时前进入期望状态
     */
    public function waitForState(string $id, string $expectedState, int $timeoutSec = 30): bool
    {
        $deadline = time() + $timeoutSec;
        while (time() < $deadline) {
            try {
                $state = $this->state($id);
            } catch (\Throwable $e) {
                // 临时错误（如 daemon 重启中）继续轮询
                usleep(500000);
                continue;
            }
            if (($state['state'] ?? '') === $expectedState) {
                return true;
            }
            // 已进入终态且非期望，提前返回
            if (in_array($state['state'] ?? '', ['failed', 'cancelled', 'expired', 'interrupted', 'success'], true)
                && ($state['state'] ?? '') !== $expectedState) {
                return false;
            }
            usleep(300000);
        }
        return false;
    }

    /**
     * 轮询等待任务结果
     *
     * 仅在任务进入 success / failed / cancelled 等终态后返回结果数组，
     * 超时则返回 null。
     *
     * @param string $id         任务 ID
     * @param int    $timeoutSec 超时秒数
     *
     * @return array|null
     */
    public function waitForResult(string $id, int $timeoutSec = 30): ?array
    {
        $terminalStates = ['success', 'failed', 'cancelled', 'expired', 'interrupted'];
        $deadline = time() + $timeoutSec;
        while (time() < $deadline) {
            try {
                $state = $this->state($id);
            } catch (\Throwable $e) {
                usleep(500000);
                continue;
            }
            if (in_array($state['state'] ?? '', $terminalStates, true)) {
                return $this->result($id);
            }
            usleep(300000);
        }
        return null;
    }

    // -----------------------------------------------------------------
    // 辅助方法
    // -----------------------------------------------------------------

    /**
     * 解析 dispatch / chain / group 的返回值
     *
     * 检测 "error:" 前缀抛 InvalidTaskConfigException，
     * 否则把结果当作任务 ID 返回。
     *
     * @param string $result  原始返回
     * @param string $context 调用上下文（用于错误信息）
     *
     * @return string 任务 / 链 / 组 ID
     *
     * @throws InvalidTaskConfigException
     */
    protected function parseResponse(string $result, string $context): string
    {
        if (strncmp($result, 'error:', 6) === 0) {
            throw new InvalidTaskConfigException(
                $context . ' 失败：' . trim(substr($result, 6))
                . " (name={$this->name})"
            );
        }
        return $result;
    }

    /**
     * 将 xhjob_state / xhjob_result / xhjob_status 的返回值归一化为关联数组
     *
     * 扩展当前返回关联数组（['k' => 'v', ...]）；
     * 同时兼容 [[k, v], ...] 形式的键值对数组，保证后续升级兼容。
     *
     * @param array $raw 原始返回
     *
     * @return array
     */
    protected function parseStateArray(array $raw): array
    {
        if (empty($raw)) {
            return [];
        }
        $first = reset($raw);
        if (is_array($first)) {
            $out = [];
            foreach ($raw as $pair) {
                if (is_array($pair) && count($pair) >= 2) {
                    $out[(string) $pair[0]] = $pair[1];
                }
            }
            return $out;
        }
        return $raw;
    }
}
