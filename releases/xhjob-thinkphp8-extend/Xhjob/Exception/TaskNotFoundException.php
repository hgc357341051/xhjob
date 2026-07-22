<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 任务未找到异常
// +----------------------------------------------------------------------
// | 当通过 task_id 查询的任务不存在时抛出
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob\Exception;

/**
 * 任务未找到异常
 *
 * 当通过 task_id 调用 get / state / result / cancel 等接口，
 * 而 daemon 中不存在该任务时抛出。
 */
class TaskNotFoundException extends XhjobException
{
}
