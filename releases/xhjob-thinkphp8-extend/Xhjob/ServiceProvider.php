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
            $name    = $app->config->get('xhjob.service_name', 'default');
            $dataDir = $app->config->get('xhjob.data_dir', null);
            return new XhjobService($name, $dataDir);
        });

        // 注册任务管理器（TaskManager）
        $this->app->bind('xhjob.manager', function (\think\App $app) {
            $name    = $app->config->get('xhjob.service_name', 'default');
            $dataDir = $app->config->get('xhjob.data_dir', null);
            return new TaskManager($name, $dataDir);
        });
    }
}
