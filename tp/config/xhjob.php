<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展配置
// +----------------------------------------------------------------------
// | 通过 env() 覆盖默认值，避免硬编码
// +----------------------------------------------------------------------

return [
    // 服务名（多实例时通过该值区分 daemon 与 sock 文件）
    'service_name' => env('XHJOB_SERVICE', 'default'),
    // 数据目录（默认 null 由扩展内置）
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    // API Token（可选，用于后续 HTTP 网关鉴权）
    'api_token'    => env('XHJOB_API_TOKEN', null),

    // 任务执行池模式：
    //   - 'coroutine'（默认）：多协程池，基于 tokio async runtime，适合 IO 密集型
    //     任务（HTTP 请求、shell 命令）。最大并发 1024，可通过
    //     XHJOB_COROUTINE_POOL_SIZE 覆盖。
    //   - 'thread'：多线程池，基于 std::thread + crossbeam-channel，每个任务
    //     在独立工作线程中执行（block_on），适合 CPU 密集型或需严格并发控制
    //     的场景。线程数默认=CPU 核数，可通过 XHJOB_THREAD_POOL_SIZE 覆盖。
    'pool_mode'    => env('XHJOB_POOL_MODE', 'coroutine'),
];
