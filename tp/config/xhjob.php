<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展配置
// +----------------------------------------------------------------------
// | 通过 env() 覆盖默认值，避免硬编码
// +----------------------------------------------------------------------
// | 支持两种配置模式（自动识别）：
// |
// | 模式 A：单实例模式（默认，向后兼容）
// |   顶层 service_name + data_dir，老用户配置不动也能工作。
// |
// | 模式 B：多实例模式（同一项目跑多套独立 daemon）
// |   配置 'default' 指定默认实例名，'instances' 字典定义每个实例的
// |   service_name + data_dir。控制器通过 ?instance=xxx 查询参数选择实例。
// |   示例：
// |   return [
// |       'default' => 'data1',
// |       'instances' => [
// |           'data1' => [
// |               'service_name' => env('XHJOB_SERVICE_DATA1', 'default'),
// |               'data_dir'     => env('XHJOB_DATA_DIR_DATA1', null),
// |           ],
// |           'data2' => [
// |               'service_name' => env('XHJOB_SERVICE_DATA2', 'data2'),
// |               'data_dir'     => env('XHJOB_DATA_DIR_DATA2', null),
// |           ],
// |       ],
// |       'api_token' => env('XHJOB_API_TOKEN', null),
// |       'pool_mode' => env('XHJOB_POOL_MODE', 'async'),
// |   ];
// |   未显式配置 data_dir 时，非 default 实例会按实例名在 runtime_path/xhjob/
// |   下分子目录（如 runtime_path/xhjob/data1），避免多实例 SQLite/sock/pid 互踩。
// +----------------------------------------------------------------------

return [
    // 服务名（多实例时通过该值区分 daemon 与 sock 文件）
    'service_name' => env('XHJOB_SERVICE', 'default'),
    // 数据目录（默认 null 由控制器 / ServiceProvider 回退到 runtime_path/xhjob，
    // 确保 www / nginx 等 web 用户可写；显式设置可覆盖）
    'data_dir'     => env('XHJOB_DATA_DIR', null),
    // API Token（必填：/xhjob/* HTTP 网关鉴权用；未配置时 XhjobAuth 中间件 fail closed 拒绝所有请求）
    'api_token'    => env('XHJOB_API_TOKEN', null),

    // 任务执行池模式：
    //   - 'async'（默认，推荐）：async task 池，基于 tokio M:N 调度（N 个 tokio
    //     worker 线程复用跑 M 个 async task，task 在 await 时 yield 让出线程）。
    //     适合 IO 密集型任务（HTTP 请求、shell 命令）。最大并发 1024，
    //     可通过 XHJOB_ASYNC_POOL_SIZE 覆盖。
    //     兼容别名：'coroutine'（等价于 'async'，保持向后兼容）。
    //   - 'thread'：1:1 OS 线程池，基于 std::thread + crossbeam-channel，每个任务
    //     在独立工作线程中执行（block_on），适合 CPU 密集型或需严格并发控制
    //     的场景。线程数默认=CPU 核数，可通过 XHJOB_THREAD_POOL_SIZE 覆盖。
    'pool_mode'    => env('XHJOB_POOL_MODE', 'async'),
];
