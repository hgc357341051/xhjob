<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 任务构建器（Fluent Builder）
// +----------------------------------------------------------------------
// | 通过链式 API 构建 task / chain / group 配置，最终输出 xhjob_dispatch
// | 所需的 JSON 结构
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob;

use Xhjob\Exception\InvalidTaskConfigException;

/**
 * 任务构建器
 *
 * 提供链式 API 构建 xhjob_dispatch / xhjob_chain / xhjob_group 所需的任务配置。
 *
 * 用法：
 *   $id = TaskBuilder::shell('echo hi')
 *       ->cron('0 * * * *')
 *       ->withRetry(3, 2)
 *       ->dispatch();
 *
 * 静态工厂：
 *   - shell($cmd)             构建 shell 任务
 *   - http($method, $url)     构建 http 任务
 *   - chain(array $builders)  构建任务链（dispatch 时调用 xhjob_chain）
 *   - group(array $builders)  构建任务组（dispatch 时调用 xhjob_group）
 *   - chord(array $headers, self $callback)  构建 chord（dispatch 时调用 xhjob_chord）
 *   - fromJson($json)         从 JSON 字符串反序列化
 */
class TaskBuilder
{
    /** @var array 任务配置（与 xhjob_dispatch 的 TaskBuilder JSON 结构一致） */
    protected $config = [];

    /** @var array|null chain 模式下的子 builder 列表 */
    protected $chainBuilders = null;

    /** @var array|null group 模式下的子 builder 列表 */
    protected $groupBuilders = null;

    /** @var array|null chord 模式下的 header builder 列表 */
    protected $chordHeaderBuilders = null;

    /** @var self|null chord 模式下的回调 builder */
    protected $chordCallbackBuilder = null;

    /**
     * 内部构造方法（请使用静态工厂 shell / http / chain / group / fromJson）
     */
    protected function __construct()
    {
        // 默认配置与 xhjob TaskBuilder 默认值对齐
        // 注意：Rust 端 TaskBuilder 中 Option<u64>/Option<i64> 字段
        // （interval / run_at / start_date / end_date / soft_timeout）
        // 必须输出 null，否则 JSON 中的 0 会被反序列化为 Some(0)，
        // 导致 interval 任务被误判为 DateTrigger 立即转 Success 终态。
        $this->config = [
            'task_type'           => 'shell',
            'payload'             => ['cmd' => ''],
            'cron'                => null,
            'interval'            => null,
            'run_at'              => null,
            'retry_max'           => 0,
            'retry_delay'         => 1,
            'timeout'             => 30,
            'priority'            => 0,
            'max_instances'       => 1,
            'max_executions'      => 0,
            'coalesce'            => true,
            'persist'             => false,
            'allow_overlap'       => false,
            'start_date'          => null,
            'end_date'            => null,
            'result_ttl'          => 0,
            'meta'                => null,
            'tags'                => [],
            'jitter'              => 0,
            'expires'             => 0,
            'retry_backoff'       => false,
            'ignore_result'       => false,
            'acks_late'           => false,
            'soft_timeout'        => null,
            'misfire_grace_time'  => 0,
            'timezone'            => null,
            'id'                  => null,
            'replace_existing'    => false,
            'rate_limit_count'    => 0,
            'rate_limit_window'   => 0,
            'acks_on_failure'     => true,
            'idempotent'          => false,
            'countdown'           => null,
        ];
    }

    // -----------------------------------------------------------------
    // 静态工厂
    // -----------------------------------------------------------------

    /**
     * 创建 shell 任务
     *
     * @param string $cmd shell 命令
     *
     * @return self
     */
    public static function shell(string $cmd): self
    {
        $b = new static();
        $b->config['task_type'] = 'shell';
        $b->config['payload']   = ['cmd' => $cmd];
        return $b;
    }

    /**
     * 创建 http 任务
     *
     * @param string $method HTTP 方法（GET / POST / PUT / DELETE ...）
     * @param string $url    请求 URL
     *
     * @return self
     */
    public static function http(string $method, string $url): self
    {
        $b = new static();
        $b->config['task_type'] = 'http';
        $b->config['payload']   = [
            'method'  => strtoupper($method),
            'url'     => $url,
            // headers must be a JSON object {}, not array [] — Rust's
            // HttpPayload deserializes headers as HashMap<String,String>
            // which requires {} not []. Using (object)[] ensures json_encode
            // produces "{}" instead of "[]".
            'headers' => (object)[],
            'body'    => null,
            'proxy'   => null,
        ];
        return $b;
    }

    /**
     * 创建任务链
     *
     * dispatch 时会调用 xhjob_chain，按顺序执行每个子任务，
     * 上一个任务的 stdout 作为下一个任务的 stdin。
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return self
     */
    public static function chain(array $builders): self
    {
        $b = new static();
        $b->chainBuilders = array_values($builders);
        return $b;
    }

    /**
     * 创建任务组
     *
     * dispatch 时会调用 xhjob_group，并行执行所有子任务。
     *
     * @param array $builders TaskBuilder 实例数组
     *
     * @return self
     */
    public static function group(array $builders): self
    {
        $b = new static();
        $b->groupBuilders = array_values($builders);
        return $b;
    }

    /**
     * 创建 chord（header + body 回调）
     *
     * dispatch 时调用 xhjob_chord，并行执行所有 header 任务，
     * 全部成功后执行 callback 任务，callback 的 meta 携带所有 header 结果。
     * 任一 header 失败时 chord 转 partial_failed 终态，不派发 callback。
     * 参考 Celery chord。
     *
     * @param array $headerBuilders TaskBuilder 实例数组（header）
     * @param self  $callback       回调 TaskBuilder（body）
     *
     * @return self
     */
    public static function chord(array $headerBuilders, self $callback): self
    {
        $b = new static();
        $b->chordHeaderBuilders = array_values($headerBuilders);
        $b->chordCallbackBuilder = $callback;
        return $b;
    }

    /**
     * 从 JSON 字符串反序列化构建器
     *
     * @param string $json TaskBuilder JSON 字符串
     *
     * @return self
     */
    public static function fromJson(string $json): self
    {
        $b = new static();
        $data = json_decode($json, true);
        $b->config = is_array($data) ? $data : [];
        return $b;
    }

    // -----------------------------------------------------------------
    // 链式配置方法（全部返回 $this）
    // -----------------------------------------------------------------

    /**
     * 设置 cron 表达式
     *
     * @param string $expr 5 字段 cron 表达式，如 "0 * * * *" 或 "0 8 * * 1-5"
     *
     * @return self
     */
    public function cron(string $expr): self
    {
        $this->config['cron'] = $expr;
        return $this;
    }

    /**
     * 设置固定间隔（秒）
     *
     * @param int $secs 间隔秒数
     *
     * @return self
     */
    public function every(int $secs): self
    {
        $this->config['interval'] = $secs;
        return $this;
    }

    /**
     * 设置一次性运行时间戳
     *
     * @param int $ts Unix 时间戳
     *
     * @return self
     */
    public function runAt(int $ts): self
    {
        $this->config['run_at'] = $ts;
        return $this;
    }

    /**
     * 设置倒计时延迟（秒）
     *
     * 等价于 runAt(time() + $secs)。与 runAt 同时设置时 runAt 优先。
     * 参考 Celery apply_async(countdown=N)。
     *
     * @param int $secs 延迟秒数
     *
     * @return self
     */
    public function countdown(int $secs): self
    {
        $this->config['countdown'] = $secs;
        return $this;
    }

    /**
     * 设置单次执行超时（秒）
     *
     * @param int $secs 超时秒数
     *
     * @return self
     */
    public function timeout(int $secs): self
    {
        $this->config['timeout'] = $secs;
        return $this;
    }

    /**
     * 设置优先级
     *
     * @param int $p 优先级（数值越大越优先）
     *
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
     * @param int $n 最大并发实例数
     *
     * @return self
     */
    public function maxInstances(int $n): self
    {
        $this->config['max_instances'] = $n;
        return $this;
    }

    /**
     * 设置重试策略
     *
     * @param int $max   最大重试次数
     * @param int $delay 重试间隔（秒）
     *
     * @return self
     */
    public function withRetry(int $max, int $delay = 1): self
    {
        $this->config['retry_max']   = $max;
        $this->config['retry_delay'] = $delay;
        return $this;
    }

    /**
     * 设置最大执行次数（0 = 无限）
     *
     * @param int $n 最大执行次数
     *
     * @return self
     */
    public function maxExecutions(int $n): self
    {
        $this->config['max_executions'] = $n;
        return $this;
    }

    /**
     * 设置 meta 元数据（任意 JSON 字符串）
     *
     * @param string|null $json JSON 字符串
     *
     * @return self
     */
    public function withMeta(?string $json): self
    {
        $this->config['meta'] = $json;
        return $this;
    }

    /**
     * 设置标签数组
     *
     * @param array $tags 标签数组
     *
     * @return self
     */
    public function tags(array $tags): self
    {
        $this->config['tags'] = array_values($tags);
        return $this;
    }

    /**
     * 追加单个标签
     *
     * @param string $tag 标签名
     *
     * @return self
     */
    public function tag(string $tag): self
    {
        $tags = $this->config['tags'] ?? [];
        if (!is_array($tags)) {
            $tags = [];
        }
        $tags[] = $tag;
        $this->config['tags'] = array_values($tags);
        return $this;
    }

    /**
     * 设置 http 请求头（仅 http 任务）
     *
     * @param array $h 键值对请求头
     *
     * @return self
     */
    public function withHeaders(array $h): self
    {
        if ($this->config['task_type'] !== 'http') {
            throw new InvalidTaskConfigException('withHeaders 仅适用于 http 任务');
        }
        $this->config['payload']['headers'] = $h;
        return $this;
    }

    /**
     * 设置 http 请求体（仅 http 任务）
     *
     * @param string $b 请求体
     *
     * @return self
     */
    public function withBody(string $b): self
    {
        if ($this->config['task_type'] !== 'http') {
            throw new InvalidTaskConfigException('withBody 仅适用于 http 任务');
        }
        $this->config['payload']['body'] = $b;
        return $this;
    }

    /**
     * 设置 http 代理（仅 http 任务）
     *
     * @param string $p 代理地址
     *
     * @return self
     */
    public function withProxy(string $p): self
    {
        if ($this->config['task_type'] !== 'http') {
            throw new InvalidTaskConfigException('withProxy 仅适用于 http 任务');
        }
        $this->config['payload']['proxy'] = $p;
        return $this;
    }

    /**
     * 设置时区
     *
     * @param string $tz 时区标识符，如 "Asia/Shanghai"
     *
     * @return self
     */
    public function withTimezone(string $tz): self
    {
        $this->config['timezone'] = $tz;
        return $this;
    }

    /**
     * 设置任务起始时间戳
     *
     * @param int $ts Unix 时间戳
     *
     * @return self
     */
    public function startAt(int $ts): self
    {
        $this->config['start_date'] = $ts;
        return $this;
    }

    /**
     * 设置任务结束时间戳
     *
     * @param int $ts Unix 时间戳
     *
     * @return self
     */
    public function endAt(int $ts): self
    {
        $this->config['end_date'] = $ts;
        return $this;
    }

    /**
     * 设置结果 TTL（秒）
     *
     * @param int $secs TTL 秒数
     *
     * @return self
     */
    public function resultTtl(int $secs): self
    {
        $this->config['result_ttl'] = $secs;
        return $this;
    }

    /**
     * 设置抖动（秒）
     *
     * @param int $secs 抖动秒数
     *
     * @return self
     */
    public function jitter(int $secs): self
    {
        $this->config['jitter'] = $secs;
        return $this;
    }

    /**
     * 设置过期时间（秒）
     *
     * @param int $secs 过期秒数
     *
     * @return self
     */
    public function expires(int $secs): self
    {
        $this->config['expires'] = $secs;
        return $this;
    }

    /**
     * 启用 / 禁用退避重试
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function retryBackoff(bool $on = true): self
    {
        $this->config['retry_backoff'] = $on;
        return $this;
    }

    /**
     * 启用 / 禁用忽略结果
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function ignoreResult(bool $on = true): self
    {
        $this->config['ignore_result'] = $on;
        return $this;
    }

    /**
     * 启用 / 禁用延迟确认
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function acksLate(bool $on = true): self
    {
        $this->config['acks_late'] = $on;
        return $this;
    }

    /**
     * 设置软超时（秒）
     *
     * @param int $secs 软超时秒数
     *
     * @return self
     */
    public function softTimeout(int $secs): self
    {
        $this->config['soft_timeout'] = $secs;
        return $this;
    }

    /**
     * 设置误触发宽限时间（秒）
     *
     * @param int $secs 宽限秒数
     *
     * @return self
     */
    public function misfireGraceTime(int $secs): self
    {
        $this->config['misfire_grace_time'] = $secs;
        return $this;
    }

    /**
     * 指定任务 ID
     *
     * @param string $id 任务 ID
     *
     * @return self
     */
    public function withId(string $id): self
    {
        $this->config['id'] = $id;
        return $this;
    }

    /**
     * 启用 / 禁用替换已存在任务
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function replaceExisting(bool $on = true): self
    {
        $this->config['replace_existing'] = $on;
        return $this;
    }

    /**
     * 设置速率限制
     *
     * @param int $count  窗口内允许的次数
     * @param int $window 窗口大小（秒）
     *
     * @return self
     */
    public function rateLimit(int $count, int $window): self
    {
        $this->config['rate_limit_count']  = $count;
        $this->config['rate_limit_window'] = $window;
        return $this;
    }

    /**
     * 启用 / 禁用失败时确认
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function acksOnFailure(bool $on = true): self
    {
        $this->config['acks_on_failure'] = $on;
        return $this;
    }

    /**
     * 声明此 HTTP 任务为幂等（即使使用 POST/PUT/DELETE/PATCH 也允许重试）
     *
     * 当 false（默认）时，非幂等 HTTP 方法（POST/PUT/DELETE/PATCH）在 5xx
     * 错误时不会被重试，以防止重复副作用（如重复扣款、重复发邮件）。
     * GET/HEAD/OPTIONS 是安全方法，无论此标志如何都会重试。
     *
     * @param bool $on 是否声明为幂等
     *
     * @return self
     */
    public function idempotent(bool $on = true): self
    {
        $this->config['idempotent'] = $on;
        return $this;
    }

    /**
     * 启用 / 禁用合并
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function coalesce(bool $on = true): self
    {
        $this->config['coalesce'] = $on;
        return $this;
    }

    /**
     * 启用 / 禁用持久化
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function persist(bool $on = true): self
    {
        $this->config['persist'] = $on;
        return $this;
    }

    /**
     * 启用 / 禁用允许重叠执行
     *
     * @param bool $on 是否启用
     *
     * @return self
     */
    public function allowOverlap(bool $on = true): self
    {
        $this->config['allow_overlap'] = $on;
        return $this;
    }

    // -----------------------------------------------------------------
    // 终结方法
    // -----------------------------------------------------------------

    /**
     * 派发任务到 daemon
     *
     * 根据构建器类型分别调用：
     *   - 普通 builder：xhjob_dispatch
     *   - chain builder：xhjob_chain
     *   - group builder：xhjob_group
     *   - chord builder：xhjob_chord
     *
     * @param string|null $service  服务名（null 表示使用默认）
     * @param string|null $dataDir  数据目录（null 表示使用默认）
     *
     * @return string task_id / chain_id / group_id / chord_id
     *
     * @throws InvalidTaskConfigException 当 daemon 返回 "error: ..." 时
     */
    public function dispatch(?string $service = null, ?string $dataDir = null): string
    {
        if ($this->chainBuilders !== null) {
            $tasksJson = json_encode(array_map(function (TaskBuilder $b) {
                return $b->toArray();
            }, $this->chainBuilders));
            $result = xhjob_chain($tasksJson, $service, $dataDir);
        } elseif ($this->groupBuilders !== null) {
            $tasksJson = json_encode(array_map(function (TaskBuilder $b) {
                return $b->toArray();
            }, $this->groupBuilders));
            $result = xhjob_group($tasksJson, $service, $dataDir);
        } elseif ($this->chordHeaderBuilders !== null) {
            $headerJson = json_encode(array_map(function (TaskBuilder $b) {
                return $b->toArray();
            }, $this->chordHeaderBuilders));
            $callbackJson = $this->chordCallbackBuilder !== null
                ? $this->chordCallbackBuilder->toJson()
                : '{}';
            $result = xhjob_chord($headerJson, $callbackJson, $service, $dataDir);
        } else {
            $result = xhjob_dispatch($this->toJson(), $service, $dataDir);
        }
        if (is_string($result) && strncmp($result, 'error:', 6) === 0) {
            throw new InvalidTaskConfigException(
                'dispatch 失败：' . trim(substr($result, 6))
            );
        }
        return (string) $result;
    }

    /**
     * 导出为关联数组
     *
     * @return array
     */
    public function toArray(): array
    {
        return $this->config;
    }

    /**
     * 导出为 JSON 字符串
     *
     * @return string
     */
    public function toJson(): string
    {
        $json = json_encode($this->config);
        if ($json === false) {
            throw new InvalidTaskConfigException(
                'TaskBuilder 配置无法编码为 JSON：' . json_last_error_msg()
            );
        }
        return $json;
    }
}
