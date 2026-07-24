<?php
declare(strict_types=1);

namespace app\middleware;

use think\Request;
use think\Response;
use think\exception\HttpException;

/**
 * Xhjob API Token 鉴权中间件。
 * 对 /xhjob/* 路由强制校验 X-Xhjob-Token header（仅 header，不再接受 ?token= query）。
 * token 必须配置（非 null 且非空串）才能访问 HTTP 网关；未配置时 fail closed，
 * 拒绝所有请求（含 createShell 等 RCE 入口），避免无鉴权放行。
 * 不再从 query 读取 token，避免泄露到 web server access logs / Referer / 浏览器历史。
 */
class XhjobAuth
{
    public function handle(Request $request, \Closure $next)
    {
        $expectedToken = config('xhjob.api_token');
        // 未配置 token 时 fail closed（拒绝所有请求），避免无鉴权 RCE。
        // 不使用 empty()：字符串 "0" 是合法的已配置 token，empty() 会误判跳过。
        if ($expectedToken === null || $expectedToken === '') {
            throw new HttpException(500, 'Xhjob API token not configured: set XHJOB_API_TOKEN env var');
        }
        // 仅从 header 读取 token（不再接受 ?token= query，避免日志/Referer 泄露）
        $token = $request->header('X-Xhjob-Token', '');
        if (!hash_equals((string)$expectedToken, (string)$token)) {
            throw new HttpException(401, 'Unauthorized: invalid or missing Xhjob token');
        }
        return $next($request);
    }
}
