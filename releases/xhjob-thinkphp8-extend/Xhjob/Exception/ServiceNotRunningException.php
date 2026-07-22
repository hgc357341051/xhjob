<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - Daemon 未运行异常
// +----------------------------------------------------------------------
// | 当调用任务相关接口但 daemon 未启动时抛出
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob\Exception;

/**
 * Daemon 未运行异常
 *
 * 当通过 TaskManager 调用任务接口而 daemon 进程未启动，
 * 或 IPC 通道不可达时抛出本异常。
 */
class ServiceNotRunningException extends XhjobException
{
}
