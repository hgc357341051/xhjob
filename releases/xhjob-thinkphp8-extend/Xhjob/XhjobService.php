<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - Daemon 生命周期管理
// +----------------------------------------------------------------------
// | 封装 xhjob_start / xhjob_stop / xhjob_restart / xhjob_status 等全局函数
// | 提供 wait / ensureRunning / ensureStopped 等便捷方法
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob;

use Xhjob\Exception\ServiceNotRunningException;
use Xhjob\Exception\XhjobException;

/**
 * Daemon 生命周期管理
 *
 * 封装 xhjob PHP 扩展的 start / stop / restart / status 等全局函数，
 * 提供等待、健康检查、确保运行 / 停止等便捷方法。
 *
 * 用法：
 *   $svc = new XhjobService('default', '/var/lib/xhjob');
 *   $pid = $svc->start();
 *   $svc->ensureRunning();
 *   $svc->stop();
 */
class XhjobService
{
    /** @var string 服务名（对应 xhjob_start 的 $name 参数） */
    private $name;

    /** @var string|null 数据目录（对应 xhjob_start 的 $dataDir 参数） */
    private $dataDir;

    /**
     * 构造方法
     *
     * @param string      $name    服务名，默认 'default'
     * @param string|null $dataDir 数据目录，默认 null（使用扩展内置默认值）
     */
    public function __construct(string $name = 'default', ?string $dataDir = null)
    {
        $this->name    = $name;
        $this->dataDir = $dataDir;
    }

    /**
     * 获取服务名
     *
     * @return string
     */
    public function getName(): string
    {
        return $this->name;
    }

    /**
     * 获取数据目录
     *
     * @return string|null
     */
    public function getDataDir(): ?string
    {
        return $this->dataDir;
    }

    /**
     * 启动 daemon
     *
     * 调用 xhjob_start 后等待 daemon 真正进入 running 状态，
     * 然后从 status() 中读取 pid 返回。
     *
     * @return int 返回 daemon 进程 PID（>0 表示成功）
     *
     * @throws ServiceNotRunningException 如果启动失败或在超时内未进入 running
     */
    public function start(): int
    {
        $ok = xhjob_start($this->name, $this->dataDir);
        if (!$ok) {
            // 取回 Rust 侧记录的失败原因（data_dir 不可写 / spawn 失败 / 轮询超时等）
            $reason = '';
            if (function_exists('xhjob_last_start_error')) {
                $err = xhjob_last_start_error();
                if (is_string($err) && $err !== '') {
                    $reason = $err;
                }
            }
            // 取回环境诊断（路径、权限、SAPI 等），便于用户一眼定位
            $diagJson = '';
            if (function_exists('xhjob_diag')) {
                $diagJson = xhjob_diag($this->name, $this->dataDir);
            }
            $msg = "xhjob_start 返回 false，无法启动 daemon (name={$this->name})";
            if ($reason !== '') {
                $msg .= "; reason: " . $reason;
            }
            if ($diagJson !== '') {
                $msg .= "; diag: " . $diagJson;
            }
            throw new ServiceNotRunningException($msg);
        }
        // 等待 daemon 真正进入 running 状态
        $this->wait(10, true);
        $status = $this->status();
        $pid = $status['pid'] ?? null;
        if ($pid === null) {
            throw new ServiceNotRunningException(
                "daemon 启动后未获取到 pid (name={$this->name})"
            );
        }
        return (int) $pid;
    }

    /**
     * 停止 daemon
     *
     * @return bool
     */
    public function stop(): bool
    {
        return (bool) xhjob_stop($this->name, $this->dataDir);
    }

    /**
     * 重启 daemon
     *
     * 调用 xhjob_restart 后等待 daemon 进入 running 状态，返回新 PID。
     *
     * @return int 新的 daemon 进程 PID
     *
     * @throws ServiceNotRunningException 如果重启失败
     */
    public function restart(): int
    {
        $ok = xhjob_restart($this->name, $this->dataDir);
        if (!$ok) {
            // 取回 Rust 侧记录的失败原因（data_dir 不可写 / spawn 失败 / 轮询超时等）
            $reason = '';
            if (function_exists('xhjob_last_start_error')) {
                $err = xhjob_last_start_error();
                if (is_string($err) && $err !== '') {
                    $reason = $err;
                }
            }
            // 取回环境诊断（路径、权限、SAPI 等），便于用户一眼定位
            $diagJson = '';
            if (function_exists('xhjob_diag')) {
                $diagJson = xhjob_diag($this->name, $this->dataDir);
            }
            $msg = "xhjob_restart 返回 false (name={$this->name})";
            if ($reason !== '') {
                $msg .= "; reason: " . $reason;
            }
            if ($diagJson !== '') {
                $msg .= "; diag: " . $diagJson;
            }
            throw new ServiceNotRunningException($msg);
        }
        $this->wait(10, true);
        $status = $this->status();
        $pid = $status['pid'] ?? null;
        if ($pid === null) {
            throw new ServiceNotRunningException(
                "daemon 重启后未获取到 pid (name={$this->name})"
            );
        }
        return (int) $pid;
    }

    /**
     * 查询 daemon 状态
     *
     * xhjob_status 返回字符串值（如 'true' / 'false' / '123'），
     * 本方法将其归一化为：
     *   - running: bool
     *   - pid: int|null（未运行时为 null）
     *
     * @return array{running: bool, pid: int|null}
     */
    public function status(): array
    {
        $raw = xhjob_status($this->name, $this->dataDir);
        $parsed = $this->parseStateArray($raw);
        $runningStr = $parsed['running'] ?? 'false';
        $running = ($runningStr === 'true' || $runningStr === true || $runningStr === 1);
        $pid = null;
        if (isset($parsed['pid']) && $parsed['pid'] !== '' && $parsed['pid'] !== 'null') {
            $pid = (int) $parsed['pid'];
        }
        return ['running' => $running, 'pid' => $pid];
    }

    /**
     * 健康检查
     *
     * 当 daemon 处于 running 状态且 pid > 0 时视为健康。
     *
     * @return array{healthy: bool, pid: int|null, stats: array}
     */
    public function healthCheck(): array
    {
        $status = $this->status();
        $healthy = $status['running'] && $status['pid'] !== null && $status['pid'] > 0;
        return [
            'healthy' => $healthy,
            'pid'     => $status['pid'],
            'stats'   => $status,
        ];
    }

    /**
     * 等待 daemon 进入期望状态
     *
     * 通过轮询 status() 实现，最长等待 $timeoutSec 秒。
     *
     * @param int  $timeoutSec    超时秒数
     * @param bool $expectRunning 期望 daemon 处于 running（true）或非 running（false）
     *
     * @return bool 是否在超时前进入期望状态
     */
    public function wait(int $timeoutSec = 10, bool $expectRunning = true): bool
    {
        $start = microtime(true);
        $deadline = $start + $timeoutSec;
        while (microtime(true) < $deadline) {
            $status = $this->status();
            if ($status['running'] === $expectRunning) {
                return true;
            }
            usleep(200000); // 200ms
        }
        return false;
    }

    /**
     * 确保 daemon 处于 running 状态
     *
     * 若未运行则启动，若已运行则直接返回。
     *
     * @return void
     *
     * @throws ServiceNotRunningException 如果启动失败
     */
    public function ensureRunning(): void
    {
        $status = $this->status();
        if ($status['running']) {
            return;
        }
        $this->start();
    }

    /**
     * 确保 daemon 处于停止状态
     *
     * 若正在运行则停止并等待其退出。
     *
     * @return void
     */
    public function ensureStopped(): void
    {
        $status = $this->status();
        if (!$status['running']) {
            return;
        }
        $this->stop();
        $this->wait(10, false);
    }

    /**
     * 将 xhjob_status / xhjob_state / xhjob_result 的返回值归一化为关联数组
     *
     * 扩展当前返回关联数组（['k' => 'v', ...]）；
     * 同时兼容 [[k, v], ...] 形式的键值对数组，保证后续升级兼容。
     *
     * @param array $raw 原始返回
     *
     * @return array
     */
    private function parseStateArray(array $raw): array
    {
        if (empty($raw)) {
            return [];
        }
        $first = reset($raw);
        // 若首个元素本身是数组（[[k, v], ...]），转换为关联数组
        if (is_array($first)) {
            $out = [];
            foreach ($raw as $pair) {
                if (is_array($pair) && count($pair) >= 2) {
                    $out[(string) $pair[0]] = $pair[1];
                }
            }
            return $out;
        }
        return $raw;
    }

    /**
     * 环境诊断
     *
     * 包装扩展函数 xhjob_diag()，返回诊断信息数组，便于排查 daemon 启动失败。
     * 可在控制器中 `return json(XhjobService::diag($name, $dataDir));` 直接输出。
     *
     * @param string|null $name    服务名，默认 'default'
     * @param string|null $dataDir 数据目录
     *
     * @return array 诊断数组（含 php_binary / sapi / data_dir / data_dir_writable /
     *               pid_file_path / log_file_path / ipc_socket_path /
     *               current_uid / current_gid / open_basedir / last_start_error 等）
     *
     * @throws XhjobException 如果扩展未加载或 xhjob_diag 不存在
     */
    public static function diag(?string $name = null, ?string $dataDir = null): array
    {
        if (!function_exists('xhjob_diag')) {
            throw new XhjobException("xhjob_diag() not available; extension not loaded");
        }
        $json = xhjob_diag($name, $dataDir);
        $arr = json_decode($json, true);
        if (!is_array($arr)) {
            return ['raw' => $json];
        }
        return $arr;
    }
}
