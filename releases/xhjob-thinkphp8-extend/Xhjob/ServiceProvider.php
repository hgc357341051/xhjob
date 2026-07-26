<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - ThinkPHP 服务提供者
// +----------------------------------------------------------------------
// | 注册 XhjobService 与 TaskManager 到容器，
// | 通过 config('xhjob.*') 读取默认服务名 / 数据目录
// +----------------------------------------------------------------------
declare(strict_types=1);

namespace Xhjob;

use think\Service;

/**
 * ThinkPHP 服务提供者
 *
 * 注册到 app/service.php 后，可通过容器获取：
 *   app('xhjob.service') -> XhjobService 实例
 *   app('xhjob.manager') -> TaskManager 实例
 *
 * 也可通过 Facade 调用：
 *   \Xhjob\facade\Xhjob::create(TaskBuilder::shell('echo hi'));
 *
 * 配置文件：config/xhjob.php
 *   'service_name' => env('XHJOB_SERVICE', 'default'),
 *   'data_dir'     => env('XHJOB_DATA_DIR', null),
 *   'api_token'    => env('XHJOB_API_TOKEN', null),
 */
class ServiceProvider extends Service
{
    /**
     * 服务启动
     *
     * @return void
     */
    public function boot()
    {
        // 注册 daemon 服务实例（XhjobService）
        // 注意：闭包参数必须类型注解 \think\App，容器才能自动解析
        $this->app->bind('xhjob.service', function (\think\App $app) {
            [$name, $dataDir] = self::resolveConfig($app);
            return new XhjobService($name, $dataDir);
        });

        // 注册任务管理器（TaskManager）
        $this->app->bind('xhjob.manager', function (\think\App $app) {
            [$name, $dataDir] = self::resolveConfig($app);
            return new TaskManager($name, $dataDir);
        });
    }

    /**
     * 解析 xhjob 配置：service_name 与 data_dir
     *
     * data_dir 未配置时回退到 runtime_path/xhjob，
     * 确保 web 用户（如宝塔 www）必然可写，避免 Rust 扩展回退到 /tmp。
     *
     * @param \think\App $app
     * @return array{0:string,1:string} [$name, $dataDir]
     */
    private static function resolveConfig(\think\App $app): array
    {
        $name = $app->config->get('xhjob.service_name', 'default');
        $name = is_string($name) && $name !== '' ? $name : 'default';

        $dataDir = $app->config->get('xhjob.data_dir');
        if (!is_string($dataDir) || $dataDir === '') {
            // 回退到 runtime_path/xhjob（web 用户必然可写）
            $dataDir = $app->getRuntimePath() . 'xhjob';
        }

        return [$name, $dataDir];
    }
}
