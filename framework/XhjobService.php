<?php
/**
 * xhjob daemon 生命周期管理
 *
 * 封装 daemon 的 start / stop / restart / status / wait / healthCheck 操作，
 * 供 PHP-FPM 与 CLI 环境统一使用。所有方法均通过 xhjob 扩展函数与 daemon 通信。
 *
 * 用法：
 *   $svc = new XhjobService('default', '/var/lib/xhjob');
 *   $pid = $svc->start();          // 启动 daemon 并等待就绪
 *   $svc->ensureRunning();         // 幂等：已运行则跳过
 *   $info = $svc->healthCheck();   // 健康检查 + 任务统计
 *   $svc->stop();                  // 停止 daemon
 */

/**
 * daemon 生命周期管理类
 */
class XhjobService
{
    /** @var string 服务名（默认 "default"） */
    private string $name;

    /** @var string|null 数据目录（PID/socket/db/log 文件所在目录） */
    private ?string $dataDir;

    /**
     * 构造函数
     *
     * @param string      $name    服务名，默认 "default"
     * @param string|null $dataDir 数据目录，null 表示使用平台默认路径
     */
    public function __construct(string $name = 'default', ?string $dataDir = null)
    {
        $this->name = $name;
        $this->dataDir = ($dataDir === '') ? null : $dataDir;
    }

    /**
     * 获取服务名
     *
     * @return string
     */
    public function name(): string
    {
        return $this->name;
    }

    /**
     * 获取数据目录
     *
     * @return string|null
     */
    public function dataDir(): ?string
    {
        return $this->dataDir;
    }

    /**
     * 启动 daemon 并等待就绪
     *
     * 调用 xhjob_start 后轮询 xhjob_status 直到 daemon 运行或超时。
     * 若 daemon 已在运行，xhjob_start 返回 true 并立即就绪。
     *
     * @param int $timeoutSec 等待就绪的超时秒数
     * @return int daemon PID
     * @throws ServiceNotRunningException 启动失败或等待超时
     */
    public function start(int $timeoutSec = 10): int
    {
        $ok = xhjob_start($this->name, $this->dataDir);
        if (!$ok) {
            throw new ServiceNotRunningException(
                "启动 daemon 失败：xhjob_start 返回 false（服务名: {$this->name}）"
            );
        }
        $this->wait($timeoutSec, true);
        $status = $this->status();
        $pid = $status['pid'];
        if ($pid === null || $pid <= 0) {
            throw new ServiceNotRunningException(
                "daemon 已启动但未获取到 PID（服务名: {$this->name}）"
            );
        }
        return $pid;
    }

    /**
     * 停止 daemon 并等待退出
     *
     * 调用 xhjob_stop 后轮询 xhjob_status 直到 daemon 不再运行或超时。
     *
     * @param int $timeoutSec 等待退出的超时秒数
     * @return bool 成功停止返回 true，停止失败或超时返回 false
     */
    public function stop(int $timeoutSec = 10): bool
    {
        $ok = xhjob_stop($this->name, $this->dataDir);
        if (!$ok) {
            return false;
        }
        try {
            $this->wait($timeoutSec, false);
        } catch (ServiceNotRunningException $e) {
            return false;
        }
        return true;
    }

    /**
     * 重启 daemon 并等待就绪
     *
     * 调用 xhjob_restart 后轮询 xhjob_status 直到新 daemon 运行或超时。
     * 重启过程中旧 daemon 收到 SIGTERM 退出，新 daemon 启动。
     *
     * @param int $timeoutSec 等待就绪的超时秒数
     * @return int 新 daemon PID
     * @throws ServiceNotRunningException 重启失败或等待超时
     */
    public function restart(int $timeoutSec = 10): int
    {
        $ok = xhjob_restart($this->name, $this->dataDir);
        if (!$ok) {
            throw new ServiceNotRunningException(
                "重启 daemon 失败：xhjob_restart 返回 false（服务名: {$this->name}）"
            );
        }
        $this->wait($timeoutSec, true);
        $status = $this->status();
        $pid = $status['pid'];
        if ($pid === null || $pid <= 0) {
            throw new ServiceNotRunningException(
                "daemon 已重启但未获取到 PID（服务名: {$this->name}）"
            );
        }
        return $pid;
    }

    /**
     * 查询 daemon 当前状态
     *
     * 调用 xhjob_status 并解析返回值为结构化数组。
     *
     * @return array ['running' => bool, 'pid' => int|null]
     */
    public function status(): array
    {
        $raw = xhjob_status($this->name, $this->dataDir);
        return $this->parseStatus($raw);
    }

    /**
     * 健康检查
     *
     * 通过 xhjob_status 验证 daemon 是否运行，并通过 xhjob_list 获取任务统计。
     * stats 为按状态分组的任务计数（如 PENDING => 3, RUNNING => 1）。
     *
     * @return array ['healthy' => bool, 'pid' => int|null, 'stats' => array]
     */
    public function healthCheck(): array
    {
        $status = $this->status();
        $healthy = $status['running'] && $status['pid'] !== null && $status['pid'] > 0;

        $stats = [];
        if ($healthy) {
            $listJson = xhjob_list($this->name, null, null, $this->dataDir);
            $stats = $this->computeStats($listJson);
        }

        return [
            'healthy' => $healthy,
            'pid'     => $status['pid'],
            'stats'   => $stats,
        ];
    }

    /**
     * 轮询等待 daemon 达到期望状态
     *
     * 以 100ms 间隔轮询 xhjob_status，直到 running 状态匹配 $expectRunning 或超时。
     *
     * @param int  $timeoutSec    超时秒数
     * @param bool $expectRunning 期望 daemon 运行（true）还是已停止（false）
     * @return bool 达到期望状态返回 true
     * @throws ServiceNotRunningException 超时未达到期望状态
     */
    public function wait(int $timeoutSec = 10, bool $expectRunning = true): bool
    {
        $deadline = microtime(true) + $timeoutSec;
        while (microtime(true) < $deadline) {
            $status = $this->status();
            if ($expectRunning && $status['running']) {
                return true;
            }
            if (!$expectRunning && !$status['running']) {
                return true;
            }
            usleep(100000); // 100ms
        }
        $action = $expectRunning ? '启动' : '停止';
        throw new ServiceNotRunningException(
            "等待 daemon {$action}超时（{$timeoutSec} 秒，服务名: {$this->name}）"
        );
    }

    /**
     * 确保 daemon 正在运行
     *
     * 若 daemon 未运行则启动，已运行则跳过。
     *
     * @param int $timeoutSec 等待就绪的超时秒数
     * @return int daemon PID
     * @throws ServiceNotRunningException 启动失败或等待超时
     */
    public function ensureRunning(int $timeoutSec = 10): int
    {
        $status = $this->status();
        if ($status['running'] && $status['pid'] !== null && $status['pid'] > 0) {
            return $status['pid'];
        }
        return $this->start($timeoutSec);
    }

    /**
     * 确保 daemon 已停止
     *
     * 若 daemon 正在运行则停止，已停止则跳过。
     *
     * @param int $timeoutSec 等待退出的超时秒数
     * @return bool 成功停止或已停止返回 true，停止失败返回 false
     */
    public function ensureStopped(int $timeoutSec = 10): bool
    {
        $status = $this->status();
        if (!$status['running']) {
            return true;
        }
        return $this->stop($timeoutSec);
    }

    /**
     * 解析 xhjob_status 返回值为结构化数组
     *
     * 兼容两种返回格式：
     * - 关联数组：['running' => 'true', 'pid' => '123']
     * - 键值对数组：[['running', 'true'], ['pid', '123']]
     *
     * @param array $raw xhjob_status 原始返回值
     * @return array ['running' => bool, 'pid' => int|null]
     */
    private function parseStatus(array $raw): array
    {
        $map = [];

        // 检测是否为关联数组格式（直接包含 running / pid / error 等字符串键）
        if (isset($raw['running']) || isset($raw['pid']) || isset($raw['error'])) {
            $map = $raw;
        } else {
            // 键值对格式：[['running', 'true'], ['pid', '123']]
            foreach ($raw as $pair) {
                if (is_array($pair) && count($pair) >= 2) {
                    $map[$pair[0]] = $pair[1];
                }
            }
        }

        $running = ($map['running'] ?? 'false') === 'true';
        $pidStr = $map['pid'] ?? null;
        $pid = ($pidStr !== null && $pidStr !== '' && ctype_digit((string)$pidStr))
            ? (int)$pidStr
            : null;

        return ['running' => $running, 'pid' => $pid];
    }

    /**
     * 从 xhjob_list 返回的 JSON 计算任务统计
     *
     * @param string $listJson xhjob_list 返回的 JSON 字符串
     * @return array<string,int> 按状态分组的任务计数
     */
    private function computeStats(string $listJson): array
    {
        if ($listJson === '' || str_starts_with($listJson, 'error:')) {
            return [];
        }
        $tasks = json_decode($listJson, true);
        if (!is_array($tasks)) {
            return [];
        }
        $stats = [];
        foreach ($tasks as $task) {
            if (!is_array($task)) {
                continue;
            }
            $state = $task['state'] ?? 'UNKNOWN';
            if (!isset($stats[$state])) {
                $stats[$state] = 0;
            }
            $stats[$state]++;
        }
        return $stats;
    }
}
