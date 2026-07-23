<?php
// +----------------------------------------------------------------------
// | Xhjob 生产环境业务模拟 HTTP 触发控制器
// +----------------------------------------------------------------------
// | 将生产模拟测试的 8 个场景通过 HTTP 接口按场景触发，
// | 使用独立服务实例 tp-prod-http，避免与 CLI 测试冲突。
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace app\controller;

use app\BaseController;
use think\Response;
use Xhjob\TaskBuilder;
use Xhjob\TaskManager;
use Xhjob\XhjobService;

/**
 * Xhjob 生产环境业务模拟 HTTP 触发控制器
 *
 * 路由前缀：/xhjob_prod
 *
 * 使用独立服务名 tp-prod-http + 数据目录 /tmp/xhjob-prod-http，
 * 与 CLI 测试脚本（tp-production）隔离，互不干扰。
 */
class XhjobProduction extends BaseController
{
    /** @var string 服务名（独立实例，避免与 CLI 测试冲突） */
    private const SERVICE = 'tp-prod-http';

    /** @var string 数据目录 */
    private const DATA_DIR = '/tmp/xhjob-prod-http';

    /** @var string HTTP 触发场景输出文件前缀 */
    private const FILE_PREFIX = '/tmp/xhjob_prod_http_';

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

    /**
     * 创建 daemon 服务实例
     *
     * @return XhjobService
     */
    private function svc(): XhjobService
    {
        return new XhjobService(self::SERVICE, self::DATA_DIR);
    }

    /**
     * 创建任务管理器实例
     *
     * @return TaskManager
     */
    private function mgr(): TaskManager
    {
        return new TaskManager(self::SERVICE, self::DATA_DIR);
    }

    /**
     * 确保 daemon 处于运行状态（未运行则自动启动）
     *
     * @return XhjobService
     */
    private function ensure(): XhjobService
    {
        $svc = $this->svc();
        $svc->ensureRunning();
        return $svc;
    }

    /**
     * 返回 JSON 错误响应（HTTP 500）
     *
     * @param \Throwable $e 异常
     *
     * @return Response
     */
    private function fail(\Throwable $e): Response
    {
        return $this->json([
            'error'   => true,
            'message' => $e->getMessage(),
            'type'    => get_class($e),
        ], 500);
    }

    // -----------------------------------------------------------------
    // 场景列表与状态
    // -----------------------------------------------------------------

    /**
     * GET /xhjob_prod/index — 场景列表与 daemon 状态
     *
     * @return Response
     */
    public function index()
    {
        try {
            $svc = $this->ensure();
            return $this->json([
                'service'  => self::SERVICE,
                'data_dir' => self::DATA_DIR,
                'status'   => $svc->status(),
                'health'   => $svc->healthCheck(),
                'scenarios' => [
                    ['method' => 'POST', 'path' => '/xhjob_prod/order',     'desc' => '场景1 订单异步处理（chain 流水线）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/report',    'desc' => '场景2 定时报表生成（cron + progress）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/etl',       'desc' => '场景3 批量数据 ETL（chord 汇总）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/cleanup',   'desc' => '场景4 定时清理任务（cron + retry 退避）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/notify',    'desc' => '场景5 通知发送（HTTP + idempotent + 重试）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/delayed',   'desc' => '场景6 延迟任务（countdown）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/rateLimit', 'desc' => '场景7 限流与并发控制（rateLimit）'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/persist',   'desc' => '场景8 持久化与崩溃恢复'],
                ],
                'queries' => [
                    ['method' => 'GET', 'path' => '/xhjob_prod/state?id=xxx',      'desc' => '查询任务状态'],
                    ['method' => 'GET', 'path' => '/xhjob_prod/result?id=xxx',     'desc' => '查询任务结果'],
                    ['method' => 'GET', 'path' => '/xhjob_prod/chainState?id=xxx', 'desc' => '查询 chain 状态'],
                    ['method' => 'GET', 'path' => '/xhjob_prod/chordState?id=xxx', 'desc' => '查询 chord 状态'],
                ],
                'control' => [
                    ['method' => 'POST', 'path' => '/xhjob_prod/restart', 'desc' => '重启 daemon'],
                    ['method' => 'POST', 'path' => '/xhjob_prod/stop',    'desc' => '停止 daemon'],
                ],
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景1：订单异步处理（chain 流水线）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/order — 场景1 订单异步处理（chain 流水线）
     *
     * 创建 3 步 chain：下单 → 发货单 → 通知，依次串联写入文件。
     *
     * @return Response
     */
    public function order()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $chainId = $mgr->createChain([
                TaskBuilder::shell('echo "ORDER:$(date +%s)" > ' . $f . 'order.txt')->withRetry(0, 0),
                TaskBuilder::shell('echo "INVOICE:$(cat ' . $f . 'order.txt)" > ' . $f . 'invoice.txt')->withRetry(0, 0),
                TaskBuilder::shell('echo "NOTIFY:$(cat ' . $f . 'invoice.txt)" > ' . $f . 'notify.txt')->withRetry(0, 0),
            ]);
            return $this->json([
                'chain_id' => $chainId,
                'scenario' => '订单异步处理（chain 流水线）',
                'hint'     => '使用 GET /xhjob_prod/chainState?id=' . $chainId . ' 查询链状态',
                'files'    => [$f . 'order.txt', $f . 'invoice.txt', $f . 'notify.txt'],
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景2：定时报表生成（cron + maxExecutions + progress）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/report — 场景2 定时报表生成
     *
     * 创建 cron('* * * * *') + maxExecutions(1) 的 shell 任务，
     * 并通过 reportProgress 上报 50。
     *
     * @return Response
     */
    public function report()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $id  = $mgr->create(
                TaskBuilder::shell('echo "REPORT:$(date +%s)" >> ' . $f . 'report.log')
                    ->cron('* * * * *')
                    ->maxExecutions(1)
                    ->withRetry(0, 0)
            );
            $mgr->reportProgress($id, 50);
            return $this->json([
                'task_id'  => $id,
                'scenario' => '定时报表生成（cron + maxExecutions + progress）',
                'progress' => 50,
                'hint'     => '使用 GET /xhjob_prod/state?id=' . $id . ' 查询任务状态',
                'file'     => $f . 'report.log',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景3：批量数据 ETL（chord 汇总）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/etl — 场景3 批量数据 ETL（chord 汇总）
     *
     * 创建 chord：3 个 header shell 并行抽取 + 1 个 callback shell 汇总。
     *
     * @return Response
     */
    public function etl()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $headers = [
                TaskBuilder::shell('echo "DATA_A:1" > ' . $f . 'part_1.txt')->withRetry(0, 0),
                TaskBuilder::shell('echo "DATA_A:2" > ' . $f . 'part_2.txt')->withRetry(0, 0),
                TaskBuilder::shell('echo "DATA_A:3" > ' . $f . 'part_3.txt')->withRetry(0, 0),
            ];
            $callback = TaskBuilder::shell('cat ' . $f . 'part_*.txt > ' . $f . 'merged.txt')->withRetry(0, 0);
            $chordId  = $mgr->createChord($headers, $callback);
            return $this->json([
                'chord_id' => $chordId,
                'scenario' => '批量数据 ETL（chord 汇总）',
                'hint'     => '使用 GET /xhjob_prod/chordState?id=' . $chordId . ' 查询 chord 状态',
                'files'    => [$f . 'part_1.txt', $f . 'part_2.txt', $f . 'part_3.txt', $f . 'merged.txt'],
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景4：定时清理任务（cron + retry 退避）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/cleanup — 场景4 定时清理任务
     *
     * 创建 cron + maxExecutions(1) + withRetry(2,1) + retryBackoff 的 shell 任务。
     * 首次执行创建 trigger 文件并失败，重试时清理 trigger 并成功。
     *
     * @return Response
     */
    public function cleanup()
    {
        try {
            $this->ensure();
            $mgr     = $this->mgr();
            $trigger = self::FILE_PREFIX . 'clean_trigger';
            $cmd     = "sh -c 'test -f {$trigger} && rm {$trigger} || (touch {$trigger} && exit 1)'";
            $id      = $mgr->create(
                TaskBuilder::shell($cmd)
                    ->cron('* * * * *')
                    ->maxExecutions(1)
                    ->withRetry(2, 1)
                    ->retryBackoff(true)
            );
            return $this->json([
                'task_id'  => $id,
                'scenario' => '定时清理任务（cron + retry 退避）',
                'hint'     => '使用 GET /xhjob_prod/state?id=' . $id . ' 查询任务状态',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景5：通知发送（HTTP + idempotent + 重试）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/notify — 场景5 通知发送
     *
     * 创建 HTTP POST 任务（url 由参数传入，默认 https://httpbin.org/post）
     * + idempotent(true) + withRetry(3,1)。
     *
     * @return Response
     */
    public function notify()
    {
        try {
            $this->ensure();
            $mgr  = $this->mgr();
            $url  = (string) $this->request->param('url', 'https://httpbin.org/post');
            $body = (string) $this->request->param('body', json_encode([
                'event'    => 'order_completed',
                'order_id' => 'PROD-HTTP-001',
            ]));
            $id = $mgr->create(
                TaskBuilder::http('POST', $url)
                    ->withBody($body)
                    ->withRetry(3, 1)
                    ->idempotent(true)
            );
            return $this->json([
                'task_id'  => $id,
                'scenario' => '通知发送（HTTP + idempotent + 重试）',
                'url'      => $url,
                'hint'     => '使用 GET /xhjob_prod/state?id=' . $id . ' 查询任务状态',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景6：延迟任务（countdown）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/delayed — 场景6 延迟任务
     *
     * 创建 countdown(5) 的 shell 任务。
     *
     * @return Response
     */
    public function delayed()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $id  = $mgr->create(
                TaskBuilder::shell('echo DELAYED > ' . $f . 'delayed.txt')
                    ->countdown(5)
                    ->withRetry(0, 0)
            );
            return $this->json([
                'task_id'   => $id,
                'scenario'  => '延迟任务（countdown 5s）',
                'countdown' => 5,
                'hint'      => '使用 GET /xhjob_prod/state?id=' . $id . ' 查询任务状态',
                'file'      => $f . 'delayed.txt',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景7：限流与并发控制（rateLimit）
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/rateLimit — 场景7 限流与并发控制
     *
     * 创建 5 个带 rateLimit(2,10) 的 shell 任务。
     *
     * @return Response
     */
    public function rateLimit()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $ids = [];
            for ($i = 1; $i <= 5; $i++) {
                $ids[] = $mgr->create(
                    TaskBuilder::shell('echo "RATE:' . $i . '" >> ' . $f . 'rate.log')
                        ->rateLimit(2, 10)
                        ->maxInstances(1)
                        ->withRetry(0, 0)
                );
            }
            return $this->json([
                'task_ids' => $ids,
                'scenario' => '限流与并发控制（rateLimit 2/10s + maxInstances 1）',
                'count'    => count($ids),
                'hint'     => '使用 GET /xhjob_prod/state?id=xxx 查询单个任务状态',
                'file'     => $f . 'rate.log',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 场景8：持久化与崩溃恢复
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/persist — 场景8 持久化与崩溃恢复
     *
     * 创建 persist(true) + countdown(8) 的 shell 任务。
     * 可调用 /xhjob_prod/restart 重启 daemon 测试恢复。
     *
     * @return Response
     */
    public function persist()
    {
        try {
            $this->ensure();
            $mgr = $this->mgr();
            $f   = self::FILE_PREFIX;
            $id  = $mgr->create(
                TaskBuilder::shell('echo RECOVERED > ' . $f . 'recovered.txt')
                    ->persist(true)
                    ->countdown(8)
                    ->withRetry(0, 0)
            );
            return $this->json([
                'task_id'  => $id,
                'scenario' => '持久化与崩溃恢复（persist + countdown 8s）',
                'hint'     => '可调用 POST /xhjob_prod/restart 重启 daemon 测试持久化恢复',
                'file'     => $f . 'recovered.txt',
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // 任务 / 链 / chord 状态查询
    // -----------------------------------------------------------------

    /**
     * GET /xhjob_prod/state?id=xxx — 查询任务状态
     *
     * @return Response
     */
    public function state()
    {
        try {
            $this->ensure();
            $id  = $this->request->param('id');
            $mgr = $this->mgr();
            return $this->json($mgr->state((string) $id));
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    /**
     * GET /xhjob_prod/result?id=xxx — 查询任务结果
     *
     * @return Response
     */
    public function result()
    {
        try {
            $this->ensure();
            $id  = $this->request->param('id');
            $mgr = $this->mgr();
            return $this->json($mgr->result((string) $id));
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    /**
     * GET /xhjob_prod/chainState?id=xxx — 查询 chain 状态
     *
     * @return Response
     */
    public function chainState()
    {
        try {
            $this->ensure();
            $id  = $this->request->param('id');
            $mgr = $this->mgr();
            return $this->json($mgr->chainState((string) $id));
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    /**
     * GET /xhjob_prod/chordState?id=xxx — 查询 chord 状态
     *
     * @return Response
     */
    public function chordState()
    {
        try {
            $this->ensure();
            $id  = $this->request->param('id');
            $mgr = $this->mgr();
            return $this->json($mgr->chordState((string) $id));
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    // -----------------------------------------------------------------
    // daemon 生命周期控制
    // -----------------------------------------------------------------

    /**
     * POST /xhjob_prod/restart — 重启 daemon（测试持久化恢复）
     *
     * @return Response
     */
    public function restart()
    {
        try {
            $svc = $this->svc();
            $pid = $svc->restart();
            return $this->json([
                'pid'      => $pid,
                'restarted' => true,
                'service'  => self::SERVICE,
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }

    /**
     * POST /xhjob_prod/stop — 停止 daemon
     *
     * @return Response
     */
    public function stop()
    {
        try {
            $svc     = $this->svc();
            $stopped = $svc->stop();
            return $this->json([
                'stopped' => $stopped,
                'service' => self::SERVICE,
            ]);
        } catch (\Throwable $e) {
            return $this->fail($e);
        }
    }
}
