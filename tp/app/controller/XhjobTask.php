<?php
// +----------------------------------------------------------------------
// | Xhjob 定时任务演示控制器
// +----------------------------------------------------------------------
// | 演示 Xhjob 扩展的 daemon 生命周期管理 + 任务 CRUD + 链 / 组 / 事件
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace app\controller;

use app\BaseController;
use think\Response;
use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;
use Xhjob\facade\Xhjob;

/**
 * Xhjob 定时任务演示控制器
 *
 * 路由前缀：/xhjob
 *
 * 注意：stop / restart 方法同时承担「停止 / 重启 daemon」与
 *      「停止 / 重启单个任务」两种语义，通过显式 ?scope= 参数区分：
 *        - scope=daemon：操作 daemon（需显式传入，避免 falsy id 误伤 daemon）
 *        - scope=task（默认）或未传 scope：操作单个任务，必须传 id，否则 400
 *      所有写方法（start / stop / restart / create系列 / update / pause / resume /
 *      reschedule / delete）均包裹 try/catch：HttpException 原样抛出，
 *      其他 Throwable 返回 ['ok'=>false,'error'=>...]（500），
 *      逻辑失败（底层返回 false）返回 ['ok'=>false,'error'=>...]。
 */
class XhjobTask extends BaseController
{
    /**
     * 输出 JSON 响应
     *
     * @param mixed $data 数据
     * @param int   $code HTTP 状态码
     *
     * @return Response
     */
    protected function json($data, int $code = 200): Response
    {
        return Response::create($data, 'json', $code);
    }

    // -----------------------------------------------------------------
    // daemon 生命周期
    // -----------------------------------------------------------------

    /**
     * GET /xhjob/index — 首页 / 状态
     *
     * @return Response
     */
    public function index()
    {
        $svc = new XhjobService();
        return $this->json([
            'status' => $svc->status(),
            'health' => $svc->healthCheck(),
        ]);
    }

    /**
     * GET /xhjob/diag — 环境诊断
     *
     * 返回 daemon 启动相关的关键路径与权限状态，用于排查启动失败。
     * 可选 query 参数：?name=aaaa&data_dir=/path
     *
     * @return Response
     */
    public function diag()
    {
        try {
            $name     = $this->request->get('name', 'default');
            $dataDir  = $this->request->get('data_dir');
            $dataDir  = ($dataDir === '' || $dataDir === null) ? null : (string) $dataDir;
            return $this->json(XhjobService::diag((string) $name, $dataDir));
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/start — 启动 daemon
     *
     * @return Response
     */
    public function start()
    {
        try {
            $svc = new XhjobService();
            $pid = $svc->start();
            return $this->json(['pid' => $pid, 'started' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/stop — 停止 daemon 或停止任务
     *
     * - ?scope=daemon：停止 daemon（显式传参，避免 falsy id 误伤整个 daemon）
     * - 默认（scope=task）或未传 scope：停止（取消）单个任务，必须传 id，否则 400
     *
     * @return Response
     */
    public function stop()
    {
        try {
            $scope = $this->request->param('scope', 'task');
            $id    = $this->request->param('id');
            if ($scope === 'daemon') {
                $svc = new XhjobService();
                $ok  = $svc->stop();
                if (!$ok) {
                    return $this->json(['ok' => false, 'error' => 'Failed to stop daemon'], 500);
                }
                return $this->json(['stopped' => true]);
            }
            // task-level：必须传 id，避免 falsy id 静默命中 daemon 路径
            if (empty($id)) {
                throw new \think\exception\HttpException(400, 'Missing required param: id (or pass scope=daemon for daemon-level operation)');
            }
            $mgr = new TaskManager();
            $ok  = $mgr->stop((string) $id);
            if (!$ok) {
                return $this->json(['ok' => false, 'error' => 'Failed to stop task (id=' . $id . ')'], 500);
            }
            return $this->json(['stopped' => true, 'id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/restart — 重启 daemon 或重新入队任务
     *
     * - ?scope=daemon：重启 daemon（显式传参，避免 falsy id 误伤整个 daemon）
     * - 默认（scope=task）或未传 scope：重新入队单个任务，必须传 id，否则 400
     *
     * @return Response
     */
    public function restart()
    {
        try {
            $scope = $this->request->param('scope', 'task');
            $id    = $this->request->param('id');
            if ($scope === 'daemon') {
                $svc = new XhjobService();
                $pid = $svc->restart();
                return $this->json(['pid' => $pid, 'restarted' => true]);
            }
            // task-level：必须传 id，避免 falsy id 静默命中 daemon 路径
            if (empty($id)) {
                throw new \think\exception\HttpException(400, 'Missing required param: id (or pass scope=daemon for daemon-level operation)');
            }
            $mgr = new TaskManager();
            $ok  = $mgr->restart((string) $id);
            if (!$ok) {
                return $this->json(['ok' => false, 'error' => 'Failed to restart task (id=' . $id . ')'], 500);
            }
            return $this->json(['restarted' => true, 'id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * GET /xhjob/status — daemon 状态
     *
     * @return Response
     */
    public function status()
    {
        $svc = new XhjobService();
        return $this->json($svc->status());
    }

    /**
     * GET /xhjob/health — 健康检查
     *
     * @return Response
     */
    public function health()
    {
        $svc = new XhjobService();
        return $this->json($svc->healthCheck());
    }

    // -----------------------------------------------------------------
    // 任务查询
    // -----------------------------------------------------------------

    /**
     * GET /xhjob/list — 任务列表
     *
     * @return Response
     */
    public function list()
    {
        $state = $this->request->param('state');
        $tag   = $this->request->param('tag');
        $mgr   = new TaskManager();
        return $this->json($mgr->list($state, $tag));
    }

    /**
     * GET /xhjob/get?id=xxx — 任务详情
     *
     * @return Response
     */
    public function get()
    {
        $id  = $this->request->param('id');
        $mgr = new TaskManager();
        return $this->json($mgr->get((string) $id));
    }

    /**
     * GET /xhjob/state?id=xxx — 任务状态
     *
     * @return Response
     */
    public function state()
    {
        $id  = $this->request->param('id');
        $mgr = new TaskManager();
        return $this->json($mgr->state((string) $id));
    }

    /**
     * GET /xhjob/result?id=xxx — 任务结果
     *
     * @return Response
     */
    public function result()
    {
        $id  = $this->request->param('id');
        $mgr = new TaskManager();
        return $this->json($mgr->result((string) $id));
    }

    /**
     * GET /xhjob/logs?id=xxx — 任务日志（事件流）
     *
     * @return Response
     */
    public function logs()
    {
        $id       = $this->request->param('id');
        $sinceTs  = intval($this->request->param('since_ts', 0));
        $mgr      = new TaskManager();
        return $this->json($mgr->logs((string) $id, $sinceTs));
    }

    // -----------------------------------------------------------------
    // 任务创建
    // -----------------------------------------------------------------

    /**
     * POST /xhjob/create — 创建任务（body = task JSON）
     *
     * @return Response
     */
    public function create()
    {
        $json = $this->request->getContent();
        $mgr  = new TaskManager();
        $id   = $mgr->create(TaskBuilder::fromJson((string) $json));
        return $this->json(['task_id' => $id], 201);
    }

    /**
     * POST /xhjob/createShell — 创建 shell 任务（演示 TaskBuilder）
     *
     * @return Response
     */
    public function createShell()
    {
        try {
            $cmd  = $this->request->param('cmd', 'echo hello-xhjob');
            $cron = $this->request->param('cron');
            $mgr  = new TaskManager();
            $b    = TaskBuilder::shell((string) $cmd);
            if ($cron) {
                $b->cron((string) $cron);
            }
            $id = $mgr->create($b);
            return $this->json(['task_id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/createHttp — 创建 HTTP 任务
     *
     * @return Response
     */
    public function createHttp()
    {
        try {
            $url    = (string) $this->request->param('url');
            $method = strtoupper((string) $this->request->param('method', 'GET'));
            $body   = $this->request->param('body');
            if ($url === '') {
                throw new \think\exception\HttpException(400, 'Missing required param: url');
            }
            // SSRF 防护：仅允许 http/https scheme，阻断 file:// / ftp:// / gopher://
            // 等非 http scheme 这一最严重 SSRF 向量。
            // 注意：完整的 SSRF 防护还需增加内网 IP 黑名单（如 127.0.0.1 / 10.x /
            // 169.254.169.254 / ::1 等），此处仅做 scheme + method 校验，内网 IP
            // 阻断留待后续完善。
            $parsed = parse_url($url);
            $scheme = strtolower($parsed['scheme'] ?? '');
            if (!in_array($scheme, ['http', 'https'], true)) {
                throw new \think\exception\HttpException(400, 'Invalid url: only http/https schemes allowed');
            }
            if (!in_array($method, ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'], true)) {
                throw new \think\exception\HttpException(400, 'Invalid method');
            }
            $mgr = new TaskManager();
            $b   = TaskBuilder::http((string) $method, (string) $url);
            if ($body) {
                $b->withBody((string) $body);
            }
            $id = $mgr->create($b);
            return $this->json(['task_id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/createCron — 创建 cron 任务 + maxExecutions
     *
     * @return Response
     */
    public function createCron()
    {
        try {
            $cmd     = $this->request->param('cmd', 'echo cron-tick');
            $cron    = $this->request->param('cron', '* * * * *');
            $maxExec = intval($this->request->param('max_executions', 0));
            $mgr     = new TaskManager();
            $id      = $mgr->create(
                TaskBuilder::shell((string) $cmd)
                    ->cron((string) $cron)
                    ->maxExecutions($maxExec)
            );
            return $this->json(['task_id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/createChain — 创建任务链
     *
     * @return Response
     */
    public function createChain()
    {
        try {
            $steps    = $this->request->param('steps', ['echo step1', 'echo step2']);
            $builders = array_map(function ($cmd) {
                return TaskBuilder::shell((string) $cmd);
            }, (array) $steps);
            $mgr = new TaskManager();
            $id  = $mgr->createChain($builders);
            return $this->json(['chain_id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/createGroup — 创建任务组
     *
     * @return Response
     */
    public function createGroup()
    {
        try {
            $tasks    = $this->request->param('tasks', ['echo g1', 'echo g2', 'echo g3']);
            $builders = array_map(function ($cmd) {
                return TaskBuilder::shell((string) $cmd);
            }, (array) $tasks);
            $mgr = new TaskManager();
            $id  = $mgr->createGroup($builders);
            return $this->json(['group_id' => $id]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    // -----------------------------------------------------------------
    // 任务控制
    // -----------------------------------------------------------------

    /**
     * POST /xhjob/update?id=xxx — 编辑任务（重建）
     *
     * @return Response
     */
    public function update()
    {
        try {
            $id    = $this->request->param('id');
            $cmd   = $this->request->param('cmd', 'echo updated');
            $cron  = $this->request->param('cron', '*/5 * * * *');
            $mgr   = new TaskManager();
            $newId = $mgr->update((string) $id, TaskBuilder::shell((string) $cmd)->cron((string) $cron));
            return $this->json(['task_id' => $newId, 'updated' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/pause?id=xxx — 暂停任务
     *
     * @return Response
     */
    public function pause()
    {
        try {
            $id  = $this->request->param('id');
            $mgr = new TaskManager();
            $ok  = $mgr->pause((string) $id);
            if (!$ok) {
                return $this->json(['ok' => false, 'error' => 'Failed to pause task (id=' . $id . ')'], 500);
            }
            return $this->json(['paused' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/resume?id=xxx — 恢复任务
     *
     * @return Response
     */
    public function resume()
    {
        try {
            $id  = $this->request->param('id');
            $mgr = new TaskManager();
            $ok  = $mgr->resume((string) $id);
            if (!$ok) {
                return $this->json(['ok' => false, 'error' => 'Failed to resume task (id=' . $id . ')'], 500);
            }
            return $this->json(['resumed' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * POST /xhjob/reschedule?id=xxx&cron=xxx — 重新调度
     *
     * @return Response
     */
    public function reschedule()
    {
        try {
            $id   = $this->request->param('id');
            $cron = $this->request->param('cron');
            $mgr  = new TaskManager();
            $ok   = $mgr->reschedule((string) $id, (string) $cron);
            if (!$ok) {
                return $this->json(['ok' => false, 'error' => 'Failed to reschedule task (id=' . $id . ')'], 500);
            }
            return $this->json(['rescheduled' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    /**
     * DELETE /xhjob/delete?id=xxx — 删除任务
     *
     * @return Response
     */
    public function delete()
    {
        try {
            $id  = $this->request->param('id');
            $mgr = new TaskManager();
            $mgr->remove((string) $id);
            return $this->json(['removed' => true]);
        } catch (\think\exception\HttpException $e) {
            throw $e;
        } catch (\Throwable $e) {
            return $this->json(['ok' => false, 'error' => $e->getMessage()], 500);
        }
    }

    // -----------------------------------------------------------------
    // 链 / 组状态
    // -----------------------------------------------------------------

    /**
     * GET /xhjob/chainState?id=xxx — 链状态
     *
     * @return Response
     */
    public function chainState()
    {
        $id  = $this->request->param('id');
        $mgr = new TaskManager();
        return $this->json($mgr->chainState((string) $id));
    }

    /**
     * GET /xhjob/groupState?id=xxx — 组状态
     *
     * @return Response
     */
    public function groupState()
    {
        $id  = $this->request->param('id');
        $mgr = new TaskManager();
        return $this->json($mgr->groupState((string) $id));
    }

    // -----------------------------------------------------------------
    // 综合演示
    // -----------------------------------------------------------------

    /**
     * POST /xhjob/demo — 完整演示（一次性跑完所有功能）
     *
     * 使用独立的 'tp-demo' 服务实例，避免污染默认服务。
     * 改为 POST 路由：demo 会起停 daemon + 派发任务，属状态变更操作，
     * 不应暴露为 GET（避免爬虫 / 预取 / 预检触发 daemon 生命周期）。
     * 方法体用 try/finally 包裹，确保即使中间步骤抛异常也始终停止 daemon。
     *
     * @return Response
     */
    public function demo()
    {
        $results = [];
        $svc     = new XhjobService('tp-demo', '/tmp/xhjob-tp-demo');
        $mgr     = new TaskManager('tp-demo', '/tmp/xhjob-tp-demo');

        try {
            // 1. 启动 daemon
            $svc->ensureStopped();
            $pid = $svc->start();
            $results['start'] = ['pid' => $pid];

            // 2. shell 任务
            $id1 = $mgr->create(TaskBuilder::shell('echo hello-thinkphp')->withRetry(0, 0));
            $mgr->waitForState($id1, 'success', 15);
            $r1 = $mgr->result($id1);
            $results['shell'] = ['id' => $id1, 'stdout' => $r1['stdout'] ?? ''];

            // 3. cron 任务
            $id2 = $mgr->create(
                TaskBuilder::shell('echo cron-job')
                    ->cron('* * * * *')
                    ->maxExecutions(2)
                    ->withRetry(0, 0)
            );
            $results['cron'] = ['id' => $id2];

            // 4. chain
            $chainId = $mgr->createChain([
                TaskBuilder::shell('echo chain-1'),
                TaskBuilder::shell('echo chain-2'),
            ]);
            $results['chain'] = ['id' => $chainId];

            // 5. group
            $groupId = $mgr->createGroup([
                TaskBuilder::shell('echo group-1'),
                TaskBuilder::shell('echo group-2'),
            ]);
            $results['group'] = ['id' => $groupId];

            // 6. list
            $list = $mgr->list();
            $results['list'] = ['count' => count($list)];

            // 7. events
            $events = $mgr->logs($id1, 0);
            $results['events'] = ['count' => count($events)];

            // 8. Facade 演示（使用容器默认服务，此处仅展示调用方式，注释以防干扰）
            // $fid = Xhjob::create(TaskBuilder::shell('echo facade-demo'));
        } finally {
            // 9. 停止 daemon（try/finally 确保中间步骤抛异常时也始终停止 daemon，
            //    避免 demo 失败残留 tp-demo daemon 占用资源）
            $svc->stop();
            $results['stop'] = ['stopped' => true];
        }

        return $this->json($results);
    }
}
