<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 非法任务配置异常
// +----------------------------------------------------------------------
// | 当 TaskBuilder 配置不合法，或 daemon 返回 error: 前缀响应时抛出
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob\Exception;

/**
 * 非法任务配置异常
 *
 * 当 TaskBuilder 构造的配置缺少必填字段（如 shell 缺 cmd、http 缺 url），
 * 或 daemon 在 dispatch / chain / group / list 等接口返回
 * "error: ..." 前缀字符串时抛出。
 */
class InvalidTaskConfigException extends XhjobException
{
}
