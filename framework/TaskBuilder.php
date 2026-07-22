<?php
/**
 * xhjob PHP fluent builder
 *
 * 封装 xhjob 扩展的全部任务配置项，提供链式 API 构建 shell / http / chain / group
 * 四种任务类型。字段名与默认值与 Rust 端 TaskBuilder 结构体完全对齐，确保
 * toJson() 产出的 JSON 可被 daemon 正确反序列化。
 *
 * 用法：
 *   // shell cron 任务（每分钟触发）
 *   $id = TaskBuilder::shell('echo hi')
 *       ->cron('* * * * *')
 *       ->withRetry(3, 1)
 *       ->timeout(60)
 *       ->dispatch();
 *
 *   // HTTP 任务
 *   $id = TaskBuilder::http('POST', 'https://api.example.com/webhook')
 *       ->withBody('{"k":"v"}')
 *       ->withHeaders(['X-Token' => 'abc'])
 *       ->dispatch();
 *
 *   // chain 流水线
 *   $chainId = TaskBuilder::chain([
 *       TaskBuilder::shell('echo step1'),
 *       TaskBuilder::shell('grep step1'),
 *   ])->dispatch();
 *
 *   // group 并行批处理
 *   $groupId = TaskBuilder::group([
 *       TaskBuilder::shell('echo a'),
 *       TaskBuilder::shell('echo b'),
 *   ])->dispatch();
 */

/**
 * 任务构建器
 *
 * 内部维护 $config 数组（与 Rust TaskBuilder JSON 结构对齐），
 * 支持 shell / http / chain / group 四种任务类型。
 */
class TaskBuilder
{
    /**
     * 构建器模式：shell / http 为普通任务，chain / group 为复合任务
     *
     * @var string
     */
    private string $mode = 'task';

    /**
     * 子构建器列表（仅 chain / group 模式使用）
     *
     * @var TaskBuilder[]
     */
    private array $children = [];

    /**
     * 任务配置数组（与 Rust TaskBuilder 字段名完全对齐）
     *
     * @var array
     */
    private array $config;

    /**
     * 私有构造函数，请使用静态工厂 shell() / http() / chain() / group() 创建实例
     */
    private function __construct()
    {
        $this->config = $this->defaultConfig();
    }

    /**
     * 默认配置（与 Rust TaskBuilder::default() 对齐）
     *
     * @return array
     */
    private function defaultConfig(): array
    {
        return [
            'task_type'           => null,
            'payload'             => null,
            'cron'                => null,
            'retry_max'           => 0,
            'retry_delay'         => 1,
            'timeout'             => 30,
            'priority'            => 0,
            'allow_overlap'       => false,
            'max_instances'       => 1,
            'coalesce'            => true,
            'persist'             => false,
            'service_name'        => 'default',
            'data_dir'            => null,
            'proxy'               => null,
            'encoding'            => null,
            'timezone'            => null,
            'max_executions'      => 0,
            'start_date'          => null,
            'end_date'            => null,
            'result_ttl'          => 0,
            'meta'                => null,
            'interval'            => null,
            'run_at'              => null,
            'jitter'              => 0,
            'expires'             => 0,
            'retry_backoff'       => false,
            'ignore_result'       => false,
            'acks_late'           => false,
            'soft_timeout'        => null,
            'misfire_grace_time'  => 0,
            'id'                  => null,
            'replace_existing'    => false,
            'tags'                => [],
            'rate_limit_count'    => 0,
            'rate_limit_window'   => 0,
            'acks_on_failure'     => true,
        ];
    }

    // =====================================================================
    // 静态工厂
    // =====================================================================

    /**
     * 创建 shell 任务构建器
     *
     * @param string $cmd shell 命令
     * @return self
     */
    public static function shell(string $cmd): self
    {
        $b = new self();
        $b->config['task_type'] = 'shell';
        $b->config['payload'] = ['cmd' => $cmd];
        return $b;
    }

    /**
     * 创建 HTTP 任务构建器
     *
     * @param string $method HTTP 方法（GET / POST / PUT / DELETE 等）
     * @param string $url    请求 URL
     * @return self
     */
    public static function http(string $method, string $url): self
    {
        $b = new self();
        $b->config['task_type'] = 'http';
        $b->config['payload'] = [
            'method'  => strtoupper($method),
            'url'     => $url,
            'headers' => new \stdClass(),
            'body'    => null,
        ];
        return $b;
    }

    /**
     * 创建 chain 任务构建器（顺序流水线）
     *
     * 每个子任务的 stdout 作为下一个子任务的输入。任一步失败则中断。
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return self
     */
    public static function chain(array $builders): self
    {
        $b = new self();
        $b->mode = 'chain';
        $b->children = array_values($builders);
        return $b;
    }

    /**
     * 创建 group 任务构建器（并行批处理）
     *
     * 所有子任务并行派发，独立执行。
     *
     * @param TaskBuilder[] $builders 子任务构建器数组
     * @return self
     */
    public static function group(array $builders): self
    {
        $b = new self();
        $b->mode = 'group';
        $b->children = array_values($builders);
        return $b;
    }

    // =====================================================================
    // 调度配置方法（链式返回 $this）
    // =====================================================================

    /**
     * 设置 cron 表达式（5 段标准或 6 段含秒）
     *
     * @param string $expr cron 表达式，如 "* * * * *"（每分钟）或 "0 8 * * *"（每天 8 点）
     * @return self
     */
    public function cron(string $expr): self
    {
        $this->config['cron'] = $expr;
        return $this;
    }

    /**
     * 设置 IntervalTrigger 周期（秒）
     *
     * 任务每 $secs 秒触发一次。与 cron / runAt 互斥，优先级最低。
     *
     * @param int $secs 周期秒数
     * @return self
     */
    public function every(int $secs): self
    {
        $this->config['interval'] = $secs < 0 ? 0 : $secs;
        return $this;
    }

    /**
     * 设置 DateTrigger 绝对时间戳
     *
     * 任务在指定 Unix 时间戳触发一次，然后进入终态。优先级最高。
     *
     * @param int $ts Unix 时间戳
     * @return self
     */
    public function runAt(int $ts): self
    {
        $this->config['run_at'] = $ts;
        return $this;
    }

    /**
     * 设置执行超时（秒）
     *
     * @param int $secs 超时秒数
     * @return self
     */
    public function timeout(int $secs): self
    {
        $this->config['timeout'] = $secs;
        return $this;
    }

    /**
     * 设置任务优先级（越高越先执行）
     *
     * @param int $p 优先级
     * @return self
     */
    public function priority(int $p): self
    {
        $this->config['priority'] = $p;
        return $this;
    }

    /**
     * 设置最大并发实例数
     *
     * 当 N>1 时强制硬性并发上限为 N，覆盖 allow_overlap。
     *
     * @param int $n 最大并发数（最小 1）
     * @return self
     */
    public function maxInstances(int $n): self
    {
        $this->config['max_instances'] = $n < 1 ? 1 : $n;
        return $this;
    }

    /**
     * 设置重试策略
     *
     * @param int $max   最大重试次数
     * @param int $delay 重试间隔秒数
     * @return self
     */
    public function withRetry(int $max, int $delay): self
    {
        $this->config['retry_max'] = $max;
        $this->config['retry_delay'] = $delay;
        return $this;
    }

    /**
     * 设置 cron 任务最大执行次数（0 = 无限）
     *
     * @param int $n 最大执行次数
     * @return self
     */
    public function maxExecutions(int $n): self
    {
        $this->config['max_executions'] = $n < 0 ? 0 : $n;
        return $this;
    }

    /**
     * 附加用户元数据（JSON 字符串）
     *
     * @param string $json JSON 字符串
     * @return self
     */
    public function withMeta(string $json): self
    {
        $this->config['meta'] = $json;
        return $this;
    }

    /**
     * 设置任务标签（替换已有标签）
     *
     * @param array $tags 标签数组
     * @return self
     */
    public function tags(array $tags): self
    {
        $clean = [];
        foreach ($tags as $t) {
            $t = (string)$t;
            if ($t !== '' && !in_array($t, $clean, true)) {
                $clean[] = $t;
            }
        }
        $this->config['tags'] = $clean;
        return $this;
    }

    /**
     * 设置 HTTP 请求头（仅 HTTP 任务有效，替换已有请求头）
     *
     * @param array $h 关联数组，如 ['X-Token' => 'abc', 'Content-Type' => 'application/json']
     * @return self
     */
    public function withHeaders(array $h): self
    {
        if ($this->config['task_type'] === 'http') {
            $this->config['payload']['headers'] = empty($h) ? new \stdClass() : $h;
        }
        return $this;
    }

    /**
     * 设置 HTTP 请求体（仅 HTTP 任务有效）
     *
     * @param string $b 请求体内容
     * @return self
     */
    public function withBody(string $b): self
    {
        if ($this->config['task_type'] === 'http') {
            $this->config['payload']['body'] = $b;
        }
        return $this;
    }

    /**
     * 设置 HTTP/SOCKS5 代理 URL（仅 HTTP 任务有效）
     *
     * 支持 http://、https://、socks5://、socks5h:// 协议，可含 user:pass@ 凭证。
     *
     * @param string $p 代理 URL
     * @return self
     */
    public function withProxy(string $p): self
    {
        $this->config['proxy'] = $p;
        return $this;
    }

    /**
     * 设置输出编码（仅 Shell 任务有效）
     *
     * 如 GBK、Big5、auto。auto 在 Windows 上自动检测 OEM 代码页。
     *
     * @param string $e 编码标签
     * @return self
     */
    public function withEncoding(string $e): self
    {
        $this->config['encoding'] = $e;
        return $this;
    }

    /**
     * 设置 cron 求值时区
     *
     * 如 Asia/Shanghai、America/New_York。设置后 next_fire 在此时区计算。
     *
     * @param string $tz IANA 时区名
     * @return self
     */
    public function withTimezone(string $tz): self
    {
        $this->config['timezone'] = $tz;
        return $this;
    }

    /**
     * 设置开始日期（Unix 时间戳）
     *
     * cron 在此时间之前的触发将被跳过。
     *
     * @param int $ts Unix 时间戳
     * @return self
     */
    public function startAt(int $ts): self
    {
        $this->config['start_date'] = $ts;
        return $this;
    }

    /**
     * 设置结束日期（Unix 时间戳）
     *
     * 此时间之后任务进入 Success 终态。
     *
     * @param int $ts Unix 时间戳
     * @return self
     */
    public function endAt(int $ts): self
    {
        $this->config['end_date'] = $ts;
        return $this;
    }

    /**
     * 设置结果 TTL（秒，0 = 永久保留）
     *
     * 任务进入终态后，结果在此时间后自动清理。
     *
     * @param int $secs TTL 秒数
     * @return self
     */
    public function resultTtl(int $secs): self
    {
        $this->config['result_ttl'] = $secs < 0 ? 0 : $secs;
        return $this;
    }

    /**
     * 设置抖动秒数
     *
     * 为 cron / interval 任务的 next_fire 添加 [0, $secs] 随机偏移，避免惊群效应。
     *
     * @param int $secs 抖动秒数
     * @return self
     */
    public function jitter(int $secs): self
    {
        $this->config['jitter'] = $secs < 0 ? 0 : $secs;
        return $this;
    }

    /**
     * 设置任务过期时间（秒）
     *
     * 任务保持 Pending 超过此时间则进入 Expired 终态。0 = 不过期。
     *
     * @param int $secs 过期秒数
     * @return self
     */
    public function expires(int $secs): self
    {
        $this->config['expires'] = $secs < 0 ? 0 : $secs;
        return $this;
    }

    /**
     * 启用/禁用重试指数退避
     *
     * 启用后重试间隔按 min(retry_delay * 2^(attempts-1), retry_delay * 60) 增长。
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function retryBackoff(bool $on): self
    {
        $this->config['retry_backoff'] = $on;
        return $this;
    }

    /**
     * 启用/禁用 fire-and-forget 模式
     *
     * 启用后 daemon 不保存任务结果，xhjob_result() 将返回空。
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function ignoreResult(bool $on): self
    {
        $this->config['ignore_result'] = $on;
        return $this;
    }

    /**
     * 启用/禁用延迟确认
     *
     * 启用后 daemon 重启时 Running 状态的任务自动重置为 Pending（崩溃恢复语义）。
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function acksLate(bool $on): self
    {
        $this->config['acks_late'] = $on;
        return $this;
    }

    /**
     * 设置软超时（秒，0 = 不设置）
     *
     * shell 执行器在 soft_timeout 秒后发送 SIGTERM；若 (timeout - soft_timeout) 秒后
     * 仍未退出则发送 SIGKILL。HTTP 任务忽略此字段。
     *
     * @param int $secs 软超时秒数
     * @return self
     */
    public function softTimeout(int $secs): self
    {
        $this->config['soft_timeout'] = $secs > 0 ? $secs : null;
        return $this;
    }

    /**
     * 设置 misfire 宽限时间（秒，0 = 使用全局默认 60s）
     *
     * 当 now - next_fire > grace_time 时触发被视为 misfire。
     *
     * @param int $secs 宽限秒数
     * @return self
     */
    public function misfireGraceTime(int $secs): self
    {
        $this->config['misfire_grace_time'] = $secs < 0 ? 0 : $secs;
        return $this;
    }

    /**
     * 设置显式任务 ID
     *
     * 设置后 dispatch 使用此 ID 而非自动生成的 UUID。
     * 空字符串清除已设置的 ID。
     *
     * @param string $id 任务 ID
     * @return self
     */
    public function withId(string $id): self
    {
        $this->config['id'] = $id === '' ? null : $id;
        return $this;
    }

    /**
     * 启用/禁用替换已存在任务
     *
     * 启用且设置了 ID 时，dispatch 替换同 ID 的已有任务（全量覆盖）。
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function replaceExisting(bool $on): self
    {
        $this->config['replace_existing'] = $on;
        return $this;
    }

    /**
     * 设置速率限制
     *
     * 在 $window 秒内最多触发 $count 次。0 = 不限制。
     *
     * @param int $count  最大触发次数
     * @param int $window 时间窗口秒数
     * @return self
     */
    public function rateLimit(int $count, int $window): self
    {
        $this->config['rate_limit_count'] = $count < 0 ? 0 : $count;
        $this->config['rate_limit_window'] = $window < 0 ? 0 : $window;
        return $this;
    }

    /**
     * 启用/禁用失败确认
     *
     * 启用时（默认），任务失败遵循 retry_max；禁用时，失败后无限重试直到成功或取消。
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function acksOnFailure(bool $on): self
    {
        $this->config['acks_on_failure'] = $on;
        return $this;
    }

    /**
     * 启用/禁用 coalesce（合并 misfire 触发）
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function coalesce(bool $on): self
    {
        $this->config['coalesce'] = $on;
        return $this;
    }

    /**
     * 启用/禁用持久化
     *
     * @param bool $on 是否启用
     * @return self
     */
    public function persist(bool $on): self
    {
        $this->config['persist'] = $on;
        return $this;
    }

    /**
     * 启用/禁用允许重叠执行
     *
     * @param bool $on 是否允许
     * @return self
     */
    public function allowOverlap(bool $on): self
    {
        $this->config['allow_overlap'] = $on;
        return $this;
    }

    // =====================================================================
    // 终结方法
    // =====================================================================

    /**
     * 派发任务到 daemon
     *
     * - shell / http 任务：调用 xhjob_dispatch，返回 task_id
     * - chain 任务：调用 xhjob_chain，返回 chain_id
     * - group 任务：调用 xhjob_group，返回 group_id
     *
     * 返回值以 "error:" 开头表示失败。
     *
     * @param string|null $service  覆盖服务名，null 使用 config 中的 service_name
     * @param string|null $dataDir  覆盖数据目录，null 使用 config 中的 data_dir
     * @return string task_id / chain_id / group_id，或 "error: ..."
     */
    public function dispatch(?string $service = null, ?string $dataDir = null): string
    {
        $name = $service ?? $this->config['service_name'] ?? 'default';
        $dir = $dataDir ?? $this->config['data_dir'] ?? null;
        $dir = ($dir === '') ? null : $dir;

        if ($this->mode === 'chain') {
            return xhjob_chain($this->toJson(), $name, $dir);
        }
        if ($this->mode === 'group') {
            return xhjob_group($this->toJson(), $name, $dir);
        }
        return xhjob_dispatch($this->toJson(), $name, $dir);
    }

    /**
     * 返回配置数组
     *
     * - shell / http 任务：返回完整的 task config 数组
     * - chain / group 任务：返回子任务 config 数组的列表
     *
     * @return array
     */
    public function toArray(): array
    {
        if ($this->mode === 'chain' || $this->mode === 'group') {
            $result = [];
            foreach ($this->children as $child) {
                if ($child instanceof self) {
                    $result[] = $child->toArray();
                } elseif (is_array($child)) {
                    $result[] = $child;
                }
            }
            return $result;
        }
        return $this->config;
    }

    /**
     * 返回 JSON 字符串
     *
     * shell / http 任务返回 task config JSON（传给 xhjob_dispatch）；
     * chain / group 任务返回子任务 config 的 JSON 数组（传给 xhjob_chain / xhjob_group）。
     *
     * @return string
     */
    public function toJson(): string
    {
        return json_encode(
            $this->toArray(),
            JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
        );
    }

    /**
     * 返回已设置的任务 ID
     *
     * @return string|null 已设置返回 ID 字符串，未设置返回 null
     */
    public function id(): ?string
    {
        return $this->config['id'] ?? null;
    }
}
