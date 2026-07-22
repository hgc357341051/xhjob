<?php
/**
 * xhjob HTTP API 层
 *
 * 将 HTTP 请求路由到 TaskManager / XhjobService，提供 RESTful 风格的任务管理与
 * daemon 生命周期控制接口。部署到 php-fpm 后通过 HTTP 暴露全部框架操作。
 *
 * 鉴权：通过 X-Xhjob-Token header 校验，token 从 XHJOB_API_TOKEN 环境变量读取。
 * 若环境变量未设置则跳过鉴权（开发模式）。
 *
 * 响应格式：
 *   成功：{"ok":true,"data":...} + HTTP 200/201
 *   失败：{"ok":false,"error":"..."} + HTTP 4xx/5xx
 *
 * 路由规则：
 *   POST   /tasks              -> create（body = task JSON）
 *   GET    /tasks              -> list（?state=&tag=）
 *   GET    /tasks/{id}         -> get
 *   DELETE /tasks/{id}         -> remove
 *   POST   /tasks/{id}/restart -> restart
 *   POST   /tasks/{id}/stop    -> stop
 *   POST   /tasks/{id}/pause   -> pause
 *   POST   /tasks/{id}/resume  -> resume
 *   GET    /tasks/{id}/state   -> state
 *   GET    /tasks/{id}/result  -> result
 *   GET    /tasks/{id}/logs    -> logs（?since_ts=）
 *   POST   /tasks/{id}/reschedule -> reschedule（body = {cron: "..."}）
 *   POST   /tasks/chain        -> createChain（body = [task1, task2, ...]）
 *   POST   /tasks/group        -> createGroup
 *   GET    /tasks/{id}/chain-state -> chainState
 *   GET    /tasks/{id}/group-state -> groupState
 *   POST   /service/start      -> start daemon
 *   POST   /service/stop       -> stop daemon
 *   POST   /service/restart   -> restart daemon
 *   GET    /service/status      -> status
 *   GET    /service/health      -> healthCheck
 *
 * 用法：
 *   require_once __DIR__ . '/framework/autoload.php';
 *   $api = new HttpApi('default', null, 'my-secret-token');
 *   $api->handle();
 */

/**
 * HTTP API 层
 *
 * 解析 HTTP 请求并路由到 TaskManager / XhjobService，统一输出 JSON 响应。
 */
class HttpApi
{
    /** @var TaskManager 任务管理器 */
    private TaskManager $manager;

    /** @var XhjobService daemon 服务管理器 */
    private XhjobService $service;

    /** @var string|null 鉴权 token（null 表示开发模式，跳过鉴权） */
    private ?string $token;

    /** @var string 服务名 */
    private string $name;

    /** @var string|null 数据目录 */
    private ?string $dataDir;

    /**
     * 构造函数
     *
     * @param string      $name    服务名，默认 "default"
     * @param string|null $dataDir 数据目录，null 表示使用平台默认路径
     * @param string|null $token    鉴权 token，null 时从 XHJOB_API_TOKEN 环境变量读取
     */
    public function __construct(string $name = 'default', ?string $dataDir = null, ?string $token = null)
    {
        $this->name = $name;
        $this->dataDir = ($dataDir === '') ? null : $dataDir;
        $this->manager = new TaskManager($name, $dataDir);
        $this->service = new XhjobService($name, $dataDir);
        $this->token = $token ?? (getenv('XHJOB_API_TOKEN') ?: null);
    }

    /**
     * 主入口：解析 $_SERVER 并返回响应
     *
     * 执行鉴权 → 解析 method + path → 路由分发 → 输出 JSON 响应。
     * 异常按类型映射到不同 HTTP 状态码。
     *
     * @return void
     */
    public function handle(): void
    {
        // 鉴权
        if (!$this->authenticate()) {
            $this->respondError('unauthorized', 401);
            return;
        }

        $method = $_SERVER['REQUEST_METHOD'] ?? 'GET';
        $path = $this->parsePath();
        $segments = $this->splitPath($path);

        try {
            $this->route($method, $segments);
        } catch (InvalidTaskConfigException $e) {
            $this->respondError($e->getMessage(), 400);
        } catch (TaskNotFoundException $e) {
            $this->respondError($e->getMessage(), 404);
        } catch (ServiceNotRunningException $e) {
            $this->respondError($e->getMessage(), 503);
        } catch (IpcException $e) {
            $this->respondError($e->getMessage(), 503);
        } catch (Throwable $e) {
            $this->respondError($e->getMessage(), 500);
        }
    }

    // =====================================================================
    // 路由分发
    // =====================================================================

    /**
     * 路由分发：根据 method + path segments 分发到对应处理器
     *
     * @param string $method   HTTP 方法（GET / POST / DELETE 等）
     * @param array  $segments 路径分段数组
     * @return void
     */
    private function route(string $method, array $segments): void
    {
        if (empty($segments)) {
            // 根路径返回 API 基本信息
            $this->respondOk([
                'service' => 'xhjob-http-api',
                'name'    => $this->name,
            ]);
            return;
        }

        $root = $segments[0];

        if ($root === 'tasks') {
            $this->routeTasks($method, $segments);
            return;
        }

        if ($root === 'service') {
            $this->routeService($method, $segments);
            return;
        }

        $this->respondError('not found: /' . implode('/', $segments), 404);
    }

    /**
     * 任务相关路由（/tasks/*）
     *
     * @param string $method   HTTP 方法
     * @param array  $segments 路径分段（首段为 "tasks"）
     * @return void
     * @throws TaskNotFoundException     任务不存在
     * @throws InvalidTaskConfigException 配置无效
     */
    private function routeTasks(string $method, array $segments): void
    {
        $count = count($segments);

        // /tasks
        if ($count === 1) {
            if ($method === 'GET') {
                $state = $_GET['state'] ?? null;
                $tag = $_GET['tag'] ?? null;
                $this->respondOk($this->manager->list(
                    $state !== '' ? $state : null,
                    $tag !== '' ? $tag : null
                ));
                return;
            }
            if ($method === 'POST') {
                $this->handleCreate();
                return;
            }
            $this->respondError('method not allowed: expected GET or POST', 405);
            return;
        }

        $second = $segments[1];

        // /tasks/chain 和 /tasks/group 必须在 /tasks/{id} 之前匹配
        if ($count === 2 && $second === 'chain') {
            if (!$this->methodAllowed($method, 'POST')) {
                return;
            }
            $this->handleCreateChain();
            return;
        }
        if ($count === 2 && $second === 'group') {
            if (!$this->methodAllowed($method, 'POST')) {
                return;
            }
            $this->handleCreateGroup();
            return;
        }

        // /tasks/{id}
        if ($count === 2) {
            $id = $second;
            if ($method === 'GET') {
                $task = $this->manager->get($id);
                if ($task === null) {
                    throw new TaskNotFoundException("任务不存在: {$id}");
                }
                $this->respondOk($task);
                return;
            }
            if ($method === 'DELETE') {
                $this->respondOk(['removed' => $this->manager->remove($id), 'id' => $id]);
                return;
            }
            $this->respondError('method not allowed: expected GET or DELETE', 405);
            return;
        }

        // /tasks/{id}/{action}
        if ($count === 3) {
            $id = $second;
            $action = $segments[2];
            $this->routeTaskAction($method, $id, $action);
            return;
        }

        $this->respondError('not found: /' . implode('/', $segments), 404);
    }

    /**
     * 任务操作路由（/tasks/{id}/{action}）
     *
     * @param string $method HTTP 方法
     * @param string $id     任务 ID
     * @param string $action 操作名（restart / stop / pause / resume / state / result / logs / reschedule / chain-state / group-state）
     * @return void
     * @throws TaskNotFoundException     任务或链/组不存在
     * @throws InvalidTaskConfigException 配置无效
     */
    private function routeTaskAction(string $method, string $id, string $action): void
    {
        switch ($action) {
            case 'restart':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->respondOk(['restarted' => $this->manager->restart($id), 'id' => $id]);
                return;

            case 'stop':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->respondOk(['stopped' => $this->manager->stop($id), 'id' => $id]);
                return;

            case 'pause':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->respondOk(['paused' => $this->manager->pause($id), 'id' => $id]);
                return;

            case 'resume':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->respondOk(['resumed' => $this->manager->resume($id), 'id' => $id]);
                return;

            case 'state':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $this->respondOk($this->manager->state($id));
                return;

            case 'result':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $this->respondOk($this->manager->result($id));
                return;

            case 'logs':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $sinceTs = isset($_GET['since_ts']) ? (int)$_GET['since_ts'] : 0;
                $this->respondOk($this->manager->logs($id, $sinceTs));
                return;

            case 'reschedule':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->handleReschedule($id);
                return;

            case 'chain-state':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $state = $this->manager->chainState($id);
                if ($state === null) {
                    throw new TaskNotFoundException("任务链不存在: {$id}");
                }
                $this->respondOk($state);
                return;

            case 'group-state':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $state = $this->manager->groupState($id);
                if ($state === null) {
                    throw new TaskNotFoundException("任务组不存在: {$id}");
                }
                $this->respondOk($state);
                return;

            default:
                $this->respondError("unknown action: {$action}", 404);
                return;
        }
    }

    /**
     * 服务相关路由（/service/*）
     *
     * @param string $method   HTTP 方法
     * @param array  $segments 路径分段（首段为 "service"）
     * @return void
     * @throws ServiceNotRunningException daemon 未运行或操作失败
     */
    private function routeService(string $method, array $segments): void
    {
        if (count($segments) !== 2) {
            $this->respondError('not found: /' . implode('/', $segments), 404);
            return;
        }

        $action = $segments[1];

        switch ($action) {
            case 'start':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $pid = $this->service->start();
                $this->respondOk(['pid' => $pid, 'name' => $this->name]);
                return;

            case 'stop':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $this->respondOk(['stopped' => $this->service->stop(), 'name' => $this->name]);
                return;

            case 'restart':
                if (!$this->methodAllowed($method, 'POST')) {
                    return;
                }
                $pid = $this->service->restart();
                $this->respondOk(['pid' => $pid, 'name' => $this->name]);
                return;

            case 'status':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $this->respondOk($this->service->status());
                return;

            case 'health':
                if (!$this->methodAllowed($method, 'GET')) {
                    return;
                }
                $this->respondOk($this->service->healthCheck());
                return;

            default:
                $this->respondError("unknown service action: {$action}", 404);
                return;
        }
    }

    // =====================================================================
    // 请求处理器
    // =====================================================================

    /**
     * 创建单个任务（POST /tasks）
     *
     * 最简方案：将 body JSON 直接传给 xhjob_dispatch，返回 task_id。
     *
     * @return void
     */
    private function handleCreate(): void
    {
        $body = file_get_contents('php://input');
        if ($body === false || $body === '') {
            $this->respondError('empty request body', 400);
            return;
        }

        // 验证 JSON 可解析
        json_decode($body, true);
        if (json_last_error() !== JSON_ERROR_NONE) {
            $this->respondError('invalid JSON body: ' . json_last_error_msg(), 400);
            return;
        }

        // 直接传给 xhjob_dispatch
        $result = xhjob_dispatch($body, $this->name, $this->dataDir);
        if (is_string($result) && strpos($result, 'error:') === 0) {
            $msg = trim(substr($result, 6));
            $this->respondError($msg, 400);
            return;
        }

        $this->respondOk(['task_id' => $result], 201);
    }

    /**
     * 创建任务链（POST /tasks/chain）
     *
     * body 为任务配置数组的 JSON，直接传给 xhjob_chain。
     *
     * @return void
     */
    private function handleCreateChain(): void
    {
        $body = file_get_contents('php://input');
        if ($body === false || $body === '') {
            $this->respondError('empty request body', 400);
            return;
        }

        json_decode($body, true);
        if (json_last_error() !== JSON_ERROR_NONE) {
            $this->respondError('invalid JSON body: ' . json_last_error_msg(), 400);
            return;
        }

        $result = xhjob_chain($body, $this->name, $this->dataDir);
        if (is_string($result) && strpos($result, 'error:') === 0) {
            $msg = trim(substr($result, 6));
            $this->respondError($msg, 400);
            return;
        }

        $this->respondOk(['chain_id' => $result], 201);
    }

    /**
     * 创建任务组（POST /tasks/group）
     *
     * body 为任务配置数组的 JSON，直接传给 xhjob_group。
     *
     * @return void
     */
    private function handleCreateGroup(): void
    {
        $body = file_get_contents('php://input');
        if ($body === false || $body === '') {
            $this->respondError('empty request body', 400);
            return;
        }

        json_decode($body, true);
        if (json_last_error() !== JSON_ERROR_NONE) {
            $this->respondError('invalid JSON body: ' . json_last_error_msg(), 400);
            return;
        }

        $result = xhjob_group($body, $this->name, $this->dataDir);
        if (is_string($result) && strpos($result, 'error:') === 0) {
            $msg = trim(substr($result, 6));
            $this->respondError($msg, 400);
            return;
        }

        $this->respondOk(['group_id' => $result], 201);
    }

    /**
     * 重新调度 cron 任务（POST /tasks/{id}/reschedule）
     *
     * body 格式：{"cron": "新的 cron 表达式"}
     *
     * @param string $id 任务 ID
     * @return void
     */
    private function handleReschedule(string $id): void
    {
        $body = file_get_contents('php://input');
        $data = json_decode($body, true);
        if (!is_array($data) || !isset($data['cron']) || !is_string($data['cron'])) {
            $this->respondError('missing or invalid "cron" field in request body', 400);
            return;
        }

        $ok = $this->manager->reschedule($id, $data['cron']);
        $this->respondOk(['rescheduled' => $ok, 'id' => $id, 'cron' => $data['cron']]);
    }

    // =====================================================================
    // 辅助方法
    // =====================================================================

    /**
     * 鉴权检查
     *
     * 从 X-Xhjob-Token header 读取 token，与配置的 token 比较。
     * 若未配置 token（XHJOB_API_TOKEN 环境变量未设置），跳过鉴权（开发模式）。
     *
     * @return bool 鉴权通过返回 true
     */
    private function authenticate(): bool
    {
        // 未配置 token 则跳过鉴权（开发模式）
        if ($this->token === null || $this->token === '') {
            return true;
        }

        // 从 header 读取（Apache mod_rewrite 会转为 REDIRECT_HTTP_X_XHJOB_TOKEN）
        $header = $_SERVER['HTTP_X_XHJOB_TOKEN']
            ?? $_SERVER['REDIRECT_HTTP_X_XHJOB_TOKEN']
            ?? '';

        return hash_equals($this->token, $header);
    }

    /**
     * 解析请求路径
     *
     * 从 REQUEST_URI 中提取路径部分（去除 query string）。
     *
     * @return string 标准化路径
     */
    private function parsePath(): string
    {
        $uri = $_SERVER['REQUEST_URI'] ?? '/';
        $path = parse_url($uri, PHP_URL_PATH);
        if (!is_string($path) || $path === '') {
            $path = '/';
        }
        return $path;
    }

    /**
     * 将路径拆分为分段数组
     *
     * @param string $path 请求路径
     * @return array 非空分段数组
     */
    private function splitPath(string $path): array
    {
        $parts = explode('/', $path);
        $segments = [];
        foreach ($parts as $p) {
            $p = trim($p);
            if ($p !== '') {
                $segments[] = $p;
            }
        }
        return $segments;
    }

    /**
     * 检查 HTTP 方法是否允许
     *
     * 不匹配时输出 405 响应。
     *
     * @param string $actual   实际方法
     * @param string $expected 期望方法
     * @return bool 允许返回 true，不允许返回 false（已输出 405 响应）
     */
    private function methodAllowed(string $actual, string $expected): bool
    {
        if ($actual !== $expected) {
            $this->respondError("method not allowed: expected {$expected}", 405);
            return false;
        }
        return true;
    }

    /**
     * 输出成功 JSON 响应
     *
     * @param mixed $data 响应数据
     * @param int   $code HTTP 状态码（默认 200，创建资源用 201）
     * @return void
     */
    private function respondOk($data, int $code = 200): void
    {
        http_response_code($code);
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(
            ['ok' => true, 'data' => $data],
            JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
        );
    }

    /**
     * 输出错误 JSON 响应
     *
     * @param string $error 错误消息
     * @param int    $code  HTTP 状态码
     * @return void
     */
    private function respondError(string $error, int $code): void
    {
        http_response_code($code);
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(
            ['ok' => false, 'error' => $error],
            JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
        );
    }
}
