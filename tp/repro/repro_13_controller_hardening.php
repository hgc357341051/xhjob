<?php
// +----------------------------------------------------------------------
// | repro_13: 控制器硬化（Task 10 / SubTask 10.13）
// +----------------------------------------------------------------------
// | 场景：直接实例化 app\controller\XhjobTask（绕过构造，反射注入 Request）
// |       验证四处安全不变量（不启动 HTTP daemon，不触发真实 service 调用）。
// |
// | Bug A（High）：stop()/restart() 用 if($id) 区分 daemon/task → falsy id
// |   静默命中 daemon 路径，杀掉整个 daemon + 所有 in-flight 任务。
// |   修复：?scope=daemon 显式区分；默认 scope=task 时空 id 返回 400。
// | Bug B（High）：createHttp() 无 url scheme / method 校验 → SSRF + method 注入。
// |   修复：仅允许 http/https scheme + method 白名单。
// | Bug C（Medium）：写方法无 try/catch，异常逃逸到 HTML 渲染器；逻辑失败仍返回成功。
// |   修复：try/catch 包裹，HttpException 原样抛出，其他 Throwable 返回
// |         ['ok'=>false,'error'=>...]（500），逻辑失败返回 ['ok'=>false,...]。
// | Bug D（Medium）：GET /xhjob/demo 状态变更（爬虫/预取触发 daemon 生命周期）。
// |   修复：路由改 POST；demo() 用 try/finally 确保 $svc->stop() 始终执行。
// |
// | 测试策略：
// |   - 400 抛出路径（校验失败）在到达 new TaskManager()/new XhjobService() 之前
// |     抛出，无 daemon 依赖，可直接对真实控制器调用验证（最强测试）。
// |   - daemon/task 分支选择（scope=daemon / id 存在）会触达真实 service，
// |     有副作用，改用复制控制器 EXACT 判别逻辑的纯函数验证（回归守卫）。
// |   - try/catch 包装模式用复制 EXACT 模式的 wrap_write() 验证四条路径。
// |   - demo 路由读 route/app.php 断言为 Route::post。
// +----------------------------------------------------------------------

require __DIR__ . '/_bootstrap.php';

// framework helper（config() 等助手函数）未在 composer autoload_files 中注册，
// 显式加载一次（helper.php 内部有 function_exists 守卫，重复加载安全）。
$helperFile = __DIR__ . '/../vendor/topthink/framework/src/helper.php';
if (is_file($helperFile)) {
    require_once $helperFile;
}

use think\Request;
use think\exception\HttpException;
use app\controller\XhjobTask;

repro_header(13, '控制器硬化（scope 显式区分 / SSRF+method 校验 / try-catch / demo POST 路由）');

try {
    // ==================================================================
    // Bug A: stop()/restart() scope 显式区分，falsy id 不再命中 daemon
    // ==================================================================

    // A1: 真实控制器 stop() 空 id + 默认 scope → HttpException(400)
    // 修复前：if($id) 为假 → 走 daemon 路径 → 杀掉整个 daemon。
    // 修复后：scope 默认 task + empty(id) → 抛 400，不触达 service。
    repro_step('Bug A1: stop() 空 id + 默认 scope → 400（real controller，不触达 daemon）', function () {
        $ctrl = build_controller([]);
        $r = invoke(function () use ($ctrl) { return $ctrl->stop(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'], 'stop() 空 id 应抛异常，实际未抛（可能误走 daemon 路径）');
        repro_assert($r['exc'] === 'HttpException', '应抛 HttpException，实际: ' . $r['exc']);
        repro_assert($r['code'] === 400, "应 400，实际 {$r['code']}");
    });

    // A2: 真实控制器 restart() 空 id + 默认 scope → HttpException(400)
    repro_step('Bug A2: restart() 空 id + 默认 scope → 400（real controller）', function () {
        $ctrl = build_controller([]);
        $r = invoke(function () use ($ctrl) { return $ctrl->restart(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'], 'restart() 空 id 应抛异常，实际未抛');
        repro_assert($r['exc'] === 'HttpException', '应抛 HttpException，实际: ' . $r['exc']);
        repro_assert($r['code'] === 400, "应 400，实际 {$r['code']}");
    });

    // A3: 真实控制器 stop() id='' （?id= 空串）+ scope=task → 400
    repro_step('Bug A3: stop() id=空串 + scope=task → 400（?id= 不再静默走 daemon）', function () {
        $ctrl = build_controller(['scope' => 'task', 'id' => '']);
        $r = invoke(function () use ($ctrl) { return $ctrl->stop(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "应 400，实际 threw={$r['threw']} code={$r['code']}");
    });

    // A4: 真实控制器 stop() id='0' （PHP falsy）+ 默认 scope → 400
    // 这是核心回归点：修复前 if('0')===false → 走 daemon 路径杀整个 daemon；
    // 修复后 empty('0')===true → 抛 400。
    repro_step('Bug A4: stop() id="0"（falsy）+ 默认 scope → 400（falsy id 不再杀 daemon）', function () {
        $ctrl = build_controller(['id' => '0']);
        $r = invoke(function () use ($ctrl) { return $ctrl->stop(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "id='0' 应 400（修复前会误杀 daemon），实际 threw={$r['threw']} code={$r['code']}");
        // 关键：不应返回 daemon 级成功响应 ['stopped'=>true]（无 id 键）
        repro_assert(!$r['threw'] || $r['return'] === null, 'id=0 时不应返回 daemon 成功响应');
    });

    // A5: 复制控制器 EXACT 判别逻辑，验证所有分支（daemon 路径有副作用，用纯函数守卫）
    repro_step('Bug A5: scope 判别逻辑（scope=daemon→daemon / task+空id→400 / task+id→task）', function () {
        // 复制 XhjobTask::stop()/restart() 的 EXACT 判别逻辑
        $cases = [
            // [scope, id, expected]
            ['task',    '',    '400'],     // 默认 + 空 id → 400
            ['task',    '0',   '400'],     // falsy id → 400（核心修复）
            ['task',    null,  '400'],     // id 缺失 → 400
            ['task',    'abc', 'task'],    // 有 id → task 路径
            ['daemon',  '',    'daemon'],  // 显式 daemon → daemon 路径
            ['daemon',  'abc', 'daemon'],  // daemon 优先于 id
            [null,      '',    '400'],     // 未传 scope（默认 task）+ 空 id → 400
        ];
        foreach ($cases as $i => [$scope, $id, $expected]) {
            $got = discriminate_stop_restart($scope, $id);
            echo "  case {$i}: scope=" . var_export($scope, true) . " id=" . var_export($id, true)
                . " → {$got} (expect {$expected})\n";
            repro_assert($got === $expected, "case {$i} 期望 {$expected} 实际 {$got}");
        }
    });

    // ==================================================================
    // Bug B: createHttp() url scheme + method 校验（SSRF + method 注入）
    // ==================================================================

    // B1: 真实控制器 createHttp() url 缺失 → 400 'Missing required param: url'
    repro_step('Bug B1: createHttp() url 空 → 400 Missing required param: url（real controller）', function () {
        $ctrl = build_controller([]);
        $r = invoke(function () use ($ctrl) { return $ctrl->createHttp(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "应 400，实际 threw={$r['threw']} code={$r['code']}");
        repro_assert(stripos($r['msg'], 'url') !== false, "错误信息应提及 url，实际: {$r['msg']}");
    });

    // B2: 真实控制器 createHttp() url='ftp://evil' → 400 'Invalid url'
    repro_step('Bug B2: createHttp() url=ftp:// → 400 Invalid url（阻断非 http scheme SSRF）', function () {
        $ctrl = build_controller(['url' => 'ftp://evil.example.com/x']);
        $r = invoke(function () use ($ctrl) { return $ctrl->createHttp(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "应 400，实际 threw={$r['threw']} code={$r['code']}");
        repro_assert(stripos($r['msg'], 'scheme') !== false || stripos($r['msg'], 'url') !== false,
            "错误信息应提及 scheme/url，实际: {$r['msg']}");
    });

    // B3: 真实控制器 createHttp() url='file:///etc/passwd' → 400
    repro_step('Bug B3: createHttp() url=file:/// → 400（阻断 file:// 读取本地文件）', function () {
        $ctrl = build_controller(['url' => 'file:///etc/passwd']);
        $r = invoke(function () use ($ctrl) { return $ctrl->createHttp(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "应 400，实际 threw={$r['threw']} code={$r['code']}");
    });

    // B4: 真实控制器 createHttp() 合法 url + 非法 method → 400 'Invalid method'
    repro_step('Bug B4: createHttp() url=http://x + method=BOGUS → 400 Invalid method（method 注入）', function () {
        $ctrl = build_controller(['url' => 'http://x.example.com/x', 'method' => 'BOGUS']);
        $r = invoke(function () use ($ctrl) { return $ctrl->createHttp(); });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'] && $r['code'] === 400, "应 400，实际 threw={$r['threw']} code={$r['code']}");
        repro_assert(stripos($r['msg'], 'method') !== false, "错误信息应提及 method，实际: {$r['msg']}");
    });

    // B5: 复制 EXACT 校验逻辑，验证更多 scheme/method 边界（合法路径不触达 service）
    repro_step('Bug B5: url/method 校验逻辑（gopher/无 scheme 拒绝；小写 method 归一化通过）', function () {
        $cases = [
            // [url, method, expect_error|null]
            ['ftp://x',            'GET',    'Invalid url'],
            ['file:///etc/passwd', 'GET',    'Invalid url'],
            ['gopher://x',         'GET',    'Invalid url'],
            ['dict://x:6379',      'GET',    'Invalid url'],
            ['example.com',        'GET',    'Invalid url'],   // 无 scheme
            ['',                   'GET',    'Missing required param: url'],
            ['http://x',           'BOGUS',  'Invalid method'],
            ['http://x',           'TRACE',  'Invalid method'], // 不在白名单
            ['http://x',           'get',    null],             // 小写归一化 → GET 通过
            ['https://x',          'post',   null],             // 小写归一化 → POST 通过
            ['http://x',           'PATCH',  null],             // 白名单大写通过
        ];
        foreach ($cases as $i => [$url, $method, $expect]) {
            $err = validate_http_input($url, $method);
            echo "  case {$i}: url={$url} method={$method} → " . ($err ?? 'OK') . "\n";
            repro_assert($err === $expect, "case {$i} 期望 " . ($expect ?? 'OK') . " 实际 " . ($err ?? 'OK'));
        }
    });

    // ==================================================================
    // Bug C: 写方法 try/catch 包装（HttpException 原样抛 / Throwable→500 JSON / 逻辑失败→ok=false）
    // ==================================================================

    // C1: 成功路径透传（不改变成功响应形状，向后兼容）
    repro_step('Bug C1: try/catch 包装成功路径透传（保持 paused=true 成功形状）', function () {
        $r = wrap_write(function () {
            return ['paused' => true]; // 模拟 pause() 成功返回
        });
        echo "  result=" . json_encode($r) . "\n";
        repro_assert(isset($r['paused']) && $r['paused'] === true, '成功响应应透传 paused=true');
        repro_assert(!isset($r['ok']) || $r['ok'] !== false, '成功响应不应带 ok=false');
    });

    // C2: 逻辑失败（底层返回 false）→ ['ok'=>false,'error'=>...]，不再返回成功
    repro_step('Bug C2: 逻辑失败（底层返回 false）→ ok=false（不再误报成功）', function () {
        $r = wrap_write(function () {
            // 模拟控制器内 if(!$ok) return $this->json(['ok'=>false,'error'=>...],500)
            return ['ok' => false, 'error' => 'Failed to pause task (id=x)'];
        });
        echo "  result=" . json_encode($r) . "\n";
        repro_assert($r['ok'] === false, '逻辑失败应 ok=false');
        repro_assert(isset($r['error']) && stripos($r['error'], 'Failed to pause') !== false, '应带描述性 error');
    });

    // C3: Throwable（非 HttpException）→ ['ok'=>false,'error'=>...] + 500，不逃逸到 HTML 渲染器
    repro_step('Bug C3: Throwable → ok=false + 500（不逃逸到 HTML 渲染器）', function () {
        $r = wrap_write(function () {
            throw new \RuntimeException('daemon 不可达: connection refused');
        });
        echo "  result=" . json_encode($r) . "\n";
        repro_assert($r['ok'] === false, 'Throwable 应转为 ok=false');
        repro_assert($r['error'] === 'daemon 不可达: connection refused', 'error 应含异常消息');
        repro_assert(($r['_status'] ?? null) === 500, '应标记 500 状态码');
    });

    // C4: HttpException 原样抛出（保留 400/401 等，不被吞为 500）
    repro_step('Bug C4: HttpException(401) 原样抛出（保留状态码，不吞为 500）', function () {
        $r = invoke(function () {
            wrap_write(function () {
                throw new HttpException(401, 'Unauthorized: invalid or missing Xhjob token');
            });
        });
        echo "  threw=" . ($r['threw'] ? 'true' : 'false') . " code={$r['code']} msg={$r['msg']}\n";
        repro_assert($r['threw'], 'HttpException 应被重新抛出，实际未抛');
        repro_assert($r['exc'] === 'HttpException', '应抛 HttpException，实际: ' . $r['exc']);
        repro_assert($r['code'] === 401, "应保留 401，实际 {$r['code']}");
    });

    // ==================================================================
    // Bug D: demo 路由改 POST（状态变更不应暴露为 GET）
    // ==================================================================

    // D1: route/app.php 中 demo 行为 Route::post，不再是 Route::get
    repro_step('Bug D1: route/app.php demo 路由为 Route::post（不再 GET，防爬虫/预取触发）', function () {
        $routeFile = __DIR__ . '/../route/app.php';
        repro_assert(is_file($routeFile), "路由文件不存在: {$routeFile}");
        $content = file_get_contents($routeFile);
        $hasPost = (bool) preg_match("/Route::post\(\s*['\"]demo['\"]\s*,\s*['\"]XhjobTask\/demo['\"]\s*\)/", $content);
        $hasGet  = (bool) preg_match("/Route::get\(\s*['\"]demo['\"]\s*,\s*['\"]XhjobTask\/demo['\"]\s*\)/", $content);
        echo "  Route::post(demo)=" . ($hasPost ? 'yes' : 'no') . " Route::get(demo)=" . ($hasGet ? 'yes' : 'no') . "\n";
        repro_assert($hasPost, 'demo 路由应为 Route::post');
        repro_assert(!$hasGet, 'demo 路由不应再为 Route::get（修复前为 GET）');
    });

    // D2: demo() 方法体用 try/finally 包裹（读源码断言结构）
    repro_step('Bug D2: demo() 方法体用 try/finally 包裹（确保 $svc->stop() 始终执行）', function () {
        $ctrlFile = __DIR__ . '/../app/controller/XhjobTask.php';
        $src = file_get_contents($ctrlFile);
        // 提取 demo() 方法体
        repro_assert(strpos($src, 'public function demo()') !== false, '未找到 demo() 方法');
        $start = strpos($src, 'public function demo()');
        $slice = substr($src, $start);
        // 截断到下一个方法（demo 是最后一个 public 方法，截到类尾）
        repro_assert(strpos($slice, 'try {') !== false, 'demo() 应含 try {');
        repro_assert(strpos($slice, '} finally {') !== false, 'demo() 应含 } finally {');
        // finally 块内应调用 $svc->stop()
        $finallyPos = strpos($slice, '} finally {');
        $finallySlice = substr($slice, $finallyPos, 400);
        repro_assert(strpos($finallySlice, '$svc->stop()') !== false, 'finally 块应调用 $svc->stop()');
        echo "  demo() 含 try/finally 且 finally 调用 \$svc->stop()\n";
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
 * 构造一个 XhjobTask 控制器实例（绕过构造，避免 boot App），反射注入受控 Request。
 * 仅用于校验失败抛出路径（不触达 TaskManager/XhjobService，无 daemon 依赖）。
 *
 * @param array $params 模拟的请求参数（注入 GET，param() 可读）
 * @return XhjobTask
 */
function build_controller(array $params = []): XhjobTask
{
    $request = new Request();
    if (!empty($params)) {
        $request->withGet($params);
    }
    $request->setMethod('POST');
    // 绕过 BaseController::__construct（需要 App），反射创建实例后注入 request
    $ctrl = (new \ReflectionClass(XhjobTask::class))->newInstanceWithoutConstructor();
    $prop = (new \ReflectionClass(XhjobTask::class))->getProperty('request');
    $prop->setAccessible(true);
    $prop->setValue($ctrl, $request);
    return $ctrl;
}

/**
 * 调用 $fn，捕获 HttpException / 其他 Throwable，返回结构化结果。
 *
 * @return array{threw: bool, code: int|null, msg: string, return: mixed, exc: string|null}
 */
function invoke(callable $fn): array
{
    try {
        $r = $fn();
        return ['threw' => false, 'code' => null, 'msg' => '', 'return' => $r, 'exc' => null];
    } catch (HttpException $e) {
        return ['threw' => true, 'code' => $e->getStatusCode(), 'msg' => $e->getMessage(), 'return' => null, 'exc' => 'HttpException'];
    } catch (\Throwable $e) {
        return ['threw' => true, 'code' => -1, 'msg' => get_class($e) . ': ' . $e->getMessage(), 'return' => null, 'exc' => get_class($e)];
    }
}

/**
 * 复制 XhjobTask::stop()/restart() 的 EXACT scope 判别逻辑。
 * 返回 'daemon' / 'task' / '400'。
 *
 * @param string|null $scope
 * @param mixed       $id
 * @return string
 */
function discriminate_stop_restart($scope, $id): string
{
    // 与控制器一致：param('scope','task') —— 未传（null）视为 'task'
    $scope = ($scope === null) ? 'task' : $scope;
    if ($scope === 'daemon') {
        return 'daemon';
    }
    if (empty($id)) {
        return '400';
    }
    return 'task';
}

/**
 * 复制 XhjobTask::createHttp() 的 EXACT url/method 校验逻辑。
 * 返回错误消息字符串，或 null 表示校验通过。
 *
 * @param string $url
 * @param string $method
 * @return string|null
 */
function validate_http_input(string $url, string $method): ?string
{
    $method = strtoupper((string) $method);
    if ($url === '') {
        return 'Missing required param: url';
    }
    $parsed = parse_url($url);
    $scheme = strtolower($parsed['scheme'] ?? '');
    if (!in_array($scheme, ['http', 'https'], true)) {
        return 'Invalid url';
    }
    if (!in_array($method, ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'], true)) {
        return 'Invalid method';
    }
    return null;
}

/**
 * 复制 XhjobTask 写方法的 EXACT try/catch 包装模式。
 *   - body 返回成功形状 → 透传
 *   - body 返回 ['ok'=>false,...]（逻辑失败）→ 透传
 *   - body 抛 HttpException → 原样抛出
 *   - body 抛其他 Throwable → ['ok'=>false,'error'=>msg,'_status'=>500]
 *
 * @param callable $body
 * @return array
 * @throws HttpException
 */
function wrap_write(callable $body): array
{
    try {
        return $body();
    } catch (HttpException $e) {
        throw $e;
    } catch (\Throwable $e) {
        return ['ok' => false, 'error' => $e->getMessage(), '_status' => 500];
    }
}
