<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 跨环境客户端
// +----------------------------------------------------------------------
// | 在 TaskManager 基础上提供 retry / setTimeout 等鲁棒性增强，
// | 适用于 CLI / 长连接 / 容器外等需要容错的环境
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob;

use Xhjob\Exception\InvalidTaskConfigException;
use Xhjob\Exception\ServiceNotRunningException;

/**
 * 跨环境客户端
 *
 * 在 TaskManager 之上提供：
 *   - retry($n, $delayMs)：对读 / 写操作自动重试
 *   - setTimeout($sec)：单次 IPC 调用的整体超时软约束
 *
 * 适用于以下场景：
 *   - CLI 长脚本中可能出现短暂 IPC 抖动
 *   - 容器 / Supervisor 进程外访问 daemon
 *   - FPM worker 多次复用时重连
 *
 * 用法：
 *   $c = new Client('default', '/var/lib/xhjob');
 *   $c->retry(3, 200);
 *   $id = $c->dispatch(TaskBuilder::shell('echo hi'));
 *   $state = $c->state($id);
 */
class Client
{
    /** @var TaskManager 内部委托的 TaskManager 实例 */
    private $mgr;

    /** @var int 重试次数（0 = 不重试） */
    private $retries = 0;

    /** @var int 重试间隔（毫秒） */
    private $retryDelayMs = 100;

    /** @var int 单次操作整体超时秒数（软约束，仅用于日志提示） */
    private $timeoutSec = 5;

    /**
     * 构造方法
     *
     * @param string      $name      服务名
     * @param string|null $dataDir   数据目录
     * @param int         $timeoutSec 单次操作超时秒数
     */
    public function __construct(string $name = 'default', ?string $dataDir = null, int $timeoutSec = 5)
    {
        $this->mgr = new TaskManager($name, $dataDir);
        $this->timeoutSec = $timeoutSec;
    }

    /**
     * 设置重试参数
     *
     * @param int $n        重试次数
     * @param int $delayMs  重试间隔（毫秒）
     *
     * @return self
     */
    public function retry(int $n, int $delayMs = 100): self
    {
        $this->retries = $n;
        $this->retryDelayMs = $delayMs;
        return $this;
    }

    /**
     * 设置单次操作超时秒数
     *
     * @param int $sec 超时秒数
     *
     * @return self
     */
    public function setTimeout(int $sec): self
    {
        $this->timeoutSec = $sec;
        return $this;
    }

    // -----------------------------------------------------------------
    // 写操作
    // -----------------------------------------------------------------

    /**
     * 派发单个任务
     *
     * @param TaskBuilder $b 任务构建器
     *
     * @return string task_id
     */
    public function dispatch(TaskBuilder $b): string
    {
        return $this->callWithRetry(function () use ($b) {
            return $this->mgr->create($b);
        });
    }

    /**
     * 派发任务链
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return string chain_id
     */
    public function dispatchChain(array $builders): string
    {
        return $this->callWithRetry(function () use ($builders) {
            return $this->mgr->createChain($builders);
        });
    }

    /**
     * 派发任务组
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return string group_id
     */
    public function dispatchGroup(array $builders): string
    {
        return $this->callWithRetry(function () use ($builders) {
            return $this->mgr->createGroup($builders);
        });
    }

    // -----------------------------------------------------------------
    // 读操作
    // -----------------------------------------------------------------

    /**
     * 查询任务状态
     *
     * @param string $id 任务 ID
     *
     * @return array
     */
    public function state(string $id): array
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->state($id);
        }, []);
    }

    /**
     * 查询任务结果
     *
     * @param string $id 任务 ID
     *
     * @return array
     */
    public function result(string $id): array
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->result($id);
        }, []);
    }

    /**
     * 获取任务详情
     *
     * @param string $id 任务 ID
     *
     * @return array|null
     */
    public function get(string $id): ?array
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->get($id);
        }, null);
    }

    /**
     * 列出任务
     *
     * @param string|null $stateFilter 状态过滤
     * @param string|null $tag          标签过滤
     *
     * @return array
     */
    public function list(?string $stateFilter = null, ?string $tag = null): array
    {
        return $this->callWithRetry(function () use ($stateFilter, $tag) {
            return $this->mgr->list($stateFilter, $tag);
        }, []);
    }

    /**
     * 查询事件日志
     *
     * @param int         $sinceTs 起始时间戳
     * @param string|null $taskId  任务 ID 过滤
     *
     * @return array
     */
    public function events(int $sinceTs = 0, ?string $taskId = null): array
    {
        return $this->callWithRetry(function () use ($sinceTs, $taskId) {
            return $this->mgr->logs($taskId ?? '', $sinceTs);
        }, []);
    }

    // -----------------------------------------------------------------
    // 控制操作
    // -----------------------------------------------------------------

    /**
     * 取消任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function cancel(string $id): bool
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->stop($id);
        }, false);
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
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->pause($id);
        }, false);
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
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->resume($id);
        }, false);
    }

    /**
     * 重新入队任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function requeue(string $id): bool
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->restart($id);
        }, false);
    }

    /**
     * 重新调度任务
     *
     * @param string $id   任务 ID
     * @param string $cron 新 cron 表达式
     *
     * @return bool
     */
    public function reschedule(string $id, string $cron): bool
    {
        return $this->callWithRetry(function () use ($id, $cron) {
            return $this->mgr->reschedule($id, $cron);
        }, false);
    }

    /**
     * 删除任务
     *
     * @param string $id 任务 ID
     *
     * @return bool
     */
    public function remove(string $id): bool
    {
        return $this->callWithRetry(function () use ($id) {
            return $this->mgr->remove($id);
        }, false);
    }

    // -----------------------------------------------------------------
    // 链 / 组状态
    // -----------------------------------------------------------------

    /**
     * 查询任务链状态
     *
     * @param string $chainId 链 ID
     *
     * @return array|null
     */
    public function chainState(string $chainId): ?array
    {
        return $this->callWithRetry(function () use ($chainId) {
            return $this->mgr->chainState($chainId);
        }, null);
    }

    /**
     * 查询任务组状态
     *
     * @param string $groupId 组 ID
     *
     * @return array|null
     */
    public function groupState(string $groupId): ?array
    {
        return $this->callWithRetry(function () use ($groupId) {
            return $this->mgr->groupState($groupId);
        }, null);
    }

    // -----------------------------------------------------------------
    // 内部：带重试的调用
    // -----------------------------------------------------------------

    /**
     * 调用闭包，失败时按 retry 配置自动重试
     *
     * 注意：业务级错误（InvalidTaskConfigException，如配置非法）不重试；
     *      仅对 ServiceNotRunningException 等连接 / IPC 错误重试。
     *      setTimeout 设置的整体超时软约束在此生效：跨重试的累计耗时
     *      超过阈值后停止重试并抛出 ServiceNotRunningException。
     *
     * @param callable    $fn           待调用闭包
     * @param mixed|null  $fallback     所有重试失败后的兜底返回值
     *
     * @return mixed
     *
     * @throws InvalidTaskConfigException    业务级错误透传
     * @throws ServiceNotRunningException    超时或重试耗尽
     */
    private function callWithRetry(callable $fn, $fallback = null)
    {
        $attempts = 0;
        $maxAttempts = $this->retries + 1;
        $lastException = null;
        $deadline = $this->timeoutSec > 0 ? microtime(true) + $this->timeoutSec : 0;
        while ($attempts < $maxAttempts) {
            try {
                return $fn();
            } catch (InvalidTaskConfigException $e) {
                // 配置非法不重试，直接抛出
                throw $e;
            } catch (\Throwable $e) {
                $lastException = $e;
                $attempts++;
                // setTimeout 软约束：跨重试累计耗时超过阈值则提前终止
                if ($deadline > 0 && microtime(true) >= $deadline) {
                    throw new ServiceNotRunningException(
                        '操作超时（setTimeout=' . $this->timeoutSec . 's）：'
                        . $e->getMessage()
                    );
                }
                if ($attempts < $maxAttempts) {
                    usleep($this->retryDelayMs * 1000);
                }
            }
        }
        // 重试用尽：若调用方提供了 fallback 则返回，否则抛出最后一个异常
        if ($lastException !== null && $fallback === null && func_num_args() < 2) {
            throw $lastException;
        }
        return $fallback;
    }
}
