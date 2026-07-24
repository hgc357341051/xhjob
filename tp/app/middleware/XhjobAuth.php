<?php
declare(strict_types=1);

namespace app\middleware;

use think\Request;
use think\Response;
use think\exception\HttpException;

/**
 * Xhjob API Token 鉴权中间件。
 * 对 /xhjob/* 路由校验 X-Xhjob-Token header 或 ?token= query。
 * config/xhjob.php 的 api_token 为空时跳过校验（向后兼容）。
 */
class XhjobAuth
{
    public function handle(Request $request, \Closure $next)
    {
        $expectedToken = config('xhjob.api_token');
        // api_token 为空时放行（向后兼容）
        if (empty($expectedToken)) {
            return $next($request);
        }
        // 优先 header，其次 query
        $token = $request->header('X-Xhjob-Token', $request->param('token', ''));
        if (!hash_equals($expectedToken, (string)$token)) {
            throw new HttpException(401, 'Unauthorized: invalid or missing Xhjob token');
        }
        return $next($request);
    }
}
