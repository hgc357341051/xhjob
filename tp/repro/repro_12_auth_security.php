<?php
// +----------------------------------------------------------------------
// | repro_12: XhjobAuth 中间件鉴权安全（Task 10 / SubTask 10.12）
// +----------------------------------------------------------------------
// | 场景：直接实例化 app\middleware\XhjobAuth 并构造 think\Request 调用 handle()，
// |       验证鉴权中间件的三处安全不变量（不启动 HTTP daemon）。
// |
// | 修复前（高危）：
// |   1. empty($expectedToken) 为真（api_token=null）→ return $next($request)
// |      → /xhjob/* 全部无鉴权放行，含 createShell（任意命令执行）→ 未鉴权 RCE。
// |   2. $token 同时接受 ?token= query → 泄露到 access logs / Referer / 浏览器历史。
// |   3. empty("0") === true → set-but-falsy token 静默跳过鉴权。
// |
// | 预期（修复后）：
// |   1. api_token=null 且无 header → 抛 HttpException(500) fail closed（不再放行）。
// |   2. api_token="secret" + header X-Xhjob-Token: secret → 放行；
// |      header 错误 → 抛 HttpException(401)。
// |   3. api_token="secret" + ?token=secret query 但无 header → 抛 HttpException(401)
// |      （query 参数不再被接受）。
// |   4. api_token="0" 不被 empty() 误判跳过（进入校验，错误 header → 401）。
// |
// | 实现：config() 助手函数通过显式 require framework helper.php + 绑定可控
// |       think\Config 实例到 Container 提供（不 boot 完整 App，无副作用）。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

// framework helper（config() 等助手函数）未在 composer autoload_files 中注册，
// 显式加载一次（helper.php 内部有 function_exists 守卫，重复加载安全）。
$helperFile = __DIR__ . '/../vendor/topthink/framework/src/helper.php';
if (is_file($helperFile)) {
    require_once $helperFile;
}

use think\Container;
use think\Config;
use think\Request;
use think\exception\HttpException;
use app\middleware\XhjobAuth;

// ----------------------------------------------------------------------
// 设置最小 Container：绑定一个可控的 Config 实例，让 config() 助手函数可用。
// ----------------------------------------------------------------------
$container = Container::getInstance();
$config = new Config();
$container->instance('config', $config);

repro_header(12, 'XhjobAuth 鉴权安全（fail closed + header-only + 无 query 旁路）');

try {
    // 不变量 1：api_token 未配置（null）时拒绝所有请求（fail closed）。
    // 修复前：empty(null)=true → return $next($request) → 无鉴权放行（RCE）。
    // 修复后：=== null → 抛 HttpException(500)。
    repro_step('不变量1: api_token=null 且无 header → 抛 HttpException(500) fail closed', function () {
        set_api_token(null);
        $req = build_request([]); // 无 header
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert(!$r['ok'], 'api_token=null 时中间件应拒绝（fail closed），实际放行（fail open 未修复）');
        repro_assert($r['code'] === 500, "应返回 500（token 未配置），实际 code={$r['code']}");
    });

    // 不变量 2a：api_token="secret" + 正确 header → 放行（调用 $next）。
    repro_step('不变量2a: api_token=secret + header X-Xhjob-Token: secret → 放行', function () {
        set_api_token('secret');
        $req = build_request(['X-Xhjob-Token' => 'secret']);
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['ok'], '正确 header 应放行，实际被拒');
        repro_assert($r['msg'] === 'PASSED', '应调用 $next 返回 PASSED，实际: ' . $r['msg']);
    });

    // 不变量 2b：api_token="secret" + 错误 header → 抛 401。
    repro_step('不变量2b: api_token=secret + header X-Xhjob-Token: wrong → 抛 HttpException(401)', function () {
        set_api_token('secret');
        $req = build_request(['X-Xhjob-Token' => 'wrong']);
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert(!$r['ok'], '错误 header 应被拒，实际放行');
        repro_assert($r['code'] === 401, "应返回 401，实际 code={$r['code']}");
    });

    // 不变量 3：api_token="secret" + ?token=secret query 但无 header → 抛 401。
    // 修复前：$request->param('token', '') 返回 'secret' → hash_equals 通过 → 放行（query 旁路）。
    // 修复后：仅读 header，header 缺失 → '' → hash_equals 失败 → 抛 401。
    repro_step('不变量3: api_token=secret + ?token=secret query 无 header → 抛 HttpException(401)', function () {
        set_api_token('secret');
        $req = build_request([], ['token' => 'secret']); // 有 query，无 header
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert(!$r['ok'], '?token= query 不应再被接受（应拒），实际放行（query 旁路未修复）');
        repro_assert($r['code'] === 401, "应返回 401，实际 code={$r['code']}");
    });

    // 辅助断言：api_token="0"（set-but-falsy）应被视为合法已配置 token，不被 empty() 误判跳过。
    // 修复前：empty("0")=true → 跳过校验（放行）。
    // 修复后：==="0" 既非 null 也非 '' → 进入 hash_equals 校验。
    repro_step('辅助: api_token="0" 不被当作未配置跳过（错误 header → 401）', function () {
        set_api_token('0');
        $req = build_request(['X-Xhjob-Token' => 'wrong']);
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert(!$r['ok'], 'api_token="0" 应进入校验（错误 header 应被拒），实际放行（empty() 误判未修复）');
        repro_assert($r['code'] === 401, "应返回 401，实际 code={$r['code']}");
    });

    // 辅助断言：api_token="0" + 正确 header "0" → 放行（"0" 是合法 token）。
    repro_step('辅助: api_token="0" + header "0" → 放行（"0" 是合法已配置 token）', function () {
        set_api_token('0');
        $req = build_request(['X-Xhjob-Token' => '0']);
        $r = invoke_middleware($req);
        echo "  result: ok=" . ($r['ok'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['ok'], 'api_token="0" + 正确 header "0" 应放行，实际被拒');
    });
} catch (\Throwable $e) {
    echo "[ERROR] 未捕获异常: " . $e->getMessage() . "\n";
    echo "  " . $e->getFile() . ":" . $e->getLine() . "\n";
}

repro_summary();

// -----------------------------------------------------------------
// 辅助函数
// -----------------------------------------------------------------

/**
 * 设置 xhjob.api_token 配置值（通过 facade 写入绑定的 Config 实例）。
 */
function set_api_token($value): void
{
    \think\facade\Config::set(['api_token' => $value], 'xhjob');
}

/**
 * 构造一个 think\Request，可指定 header 与 query（?token=）。
 */
function build_request(array $headers = [], array $get = []): Request
{
    $request = new Request();
    if (!empty($headers)) {
        $request->withHeader($headers); // withHeader 会 lowercase key
    }
    if (!empty($get)) {
        $request->withGet($get);
    }
    $request->setMethod('GET');
    return $request;
}

/**
 * 调用 XhjobAuth::handle()，捕获 HttpException，返回结构化结果。
 *
 * @return array{ok: bool, code: int|null, msg: string}
 *   ok=true 表示放行（$next 被调用）；ok=false 表示抛了 HttpException。
 */
function invoke_middleware(Request $request): array
{
    $mw = new XhjobAuth();
    $next = function ($req) {
        return 'PASSED';
    };
    try {
        $result = $mw->handle($request, $next);
        return ['ok' => true, 'code' => null, 'msg' => is_string($result) ? $result : 'next-invoked'];
    } catch (HttpException $e) {
        return ['ok' => false, 'code' => $e->getStatusCode(), 'msg' => $e->getMessage()];
    }
}
