<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - ThinkPHP Facade
// +----------------------------------------------------------------------
// | 通过静态方式调用 TaskManager：
// |   \Xhjob\facade\Xhjob::create(TaskBuilder::shell('echo hi'));
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob\facade;

use think\Facade;

/**
 * Xhjob Facade
 *
 * 把容器中的 'xhjob.manager'（TaskManager 实例）以静态方法形式暴露。
 *
 * 用法：
 *   use Xhjob\facade\Xhjob;
 *   use Xhjob\TaskBuilder;
 *
 *   $id = Xhjob::create(TaskBuilder::shell('echo hi'));
 *   $state = Xhjob::state($id);
 *   $list  = Xhjob::list();
 *
 * @method static string create(TaskBuilder $b)
 * @method static string createChain(array $builders)
 * @method static string createGroup(array $builders)
 * @method static string createChord(array $headerBuilders, TaskBuilder $callback)
 * @method static string update(string $id, TaskBuilder $b)
 * @method static array|null get(string $id)
 * @method static array list(?string $stateFilter = null, ?string $tag = null)
 * @method static array state(string $id)
 * @method static array result(string $id)
 * @method static bool stop(string $id)
 * @method static bool restart(string $id)
 * @method static bool pause(string $id)
 * @method static bool resume(string $id)
 * @method static bool remove(string $id)
 * @method static array logs(string $id, int $sinceTs = 0)
 * @method static bool reschedule(string $id, string $cron)
 * @method static array|null chainState(string $chainId)
 * @method static array|null groupState(string $groupId)
 * @method static array|null chordState(string $chordId)
 * @method static bool reportProgress(string $id, int $percent, ?string $meta = null)
 * @method static array pullEvents(int $sinceTs = 0, ?string $eventType = null)
 * @method static array inspect(string $mode = 'stats')
 * @method static bool waitForState(string $id, string $expectedState, int $timeoutSec = 30)
 * @method static array|null waitForResult(string $id, int $timeoutSec = 30)
 */
class Xhjob extends Facade
{
    /**
     * 绑定到容器中的 'xhjob.manager' 标识
     *
     * @return string
     */
    protected static function getFacadeClass(): string
    {
        return 'xhjob.manager';
    }
}
