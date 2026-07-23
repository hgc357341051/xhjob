<?php
// +----------------------------------------------------------------------
// | ThinkPHP [ WE CAN DO IT JUST THINK ]
// +----------------------------------------------------------------------
// | Copyright (c) 2006~2018 http://thinkphp.cn All rights reserved.
// +----------------------------------------------------------------------
// | Licensed ( http://www.apache.org/licenses/LICENSE-2.0 )
// +----------------------------------------------------------------------
// | Author: liu21st <liu21st@gmail.com>
// +----------------------------------------------------------------------
use think\facade\Route;

Route::get('think', function () {
    return 'hello,ThinkPHP8!';
});

Route::get('hello/:name', 'index/hello');

// +----------------------------------------------------------------------
// | Xhjob 定时任务演示路由
// | 路由前缀 /xhjob，对应 app\controller\XhjobTask
// | stop / restart 同时支持 daemon（无 id）与单个任务（带 id）操作
// +----------------------------------------------------------------------
Route::group('xhjob', function () {
    Route::get('index', 'XhjobTask/index');
    Route::post('start', 'XhjobTask/start');
    Route::post('stop', 'XhjobTask/stop');
    Route::post('restart', 'XhjobTask/restart');
    Route::get('status', 'XhjobTask/status');
    Route::get('health', 'XhjobTask/health');
    Route::get('list', 'XhjobTask/list');
    Route::get('get', 'XhjobTask/get');
    Route::post('create', 'XhjobTask/create');
    Route::post('createShell', 'XhjobTask/createShell');
    Route::post('createHttp', 'XhjobTask/createHttp');
    Route::post('createCron', 'XhjobTask/createCron');
    Route::post('createChain', 'XhjobTask/createChain');
    Route::post('createGroup', 'XhjobTask/createGroup');
    Route::get('state', 'XhjobTask/state');
    Route::get('result', 'XhjobTask/result');
    Route::get('logs', 'XhjobTask/logs');
    Route::post('update', 'XhjobTask/update');
    Route::post('pause', 'XhjobTask/pause');
    Route::post('resume', 'XhjobTask/resume');
    Route::post('reschedule', 'XhjobTask/reschedule');
    Route::delete('delete', 'XhjobTask/delete');
    Route::get('chainState', 'XhjobTask/chainState');
    Route::get('groupState', 'XhjobTask/groupState');
    Route::get('demo', 'XhjobTask/demo');
});

// +----------------------------------------------------------------------
// | Xhjob 生产环境业务模拟 HTTP 触发路由
// | 路由前缀 /xhjob_prod，对应 app\controller\XhjobProduction
// | 使用独立服务实例 tp-prod-http，与 CLI 测试隔离
// +----------------------------------------------------------------------
Route::group('xhjob_prod', function () {
    Route::get('index', 'XhjobProduction/index');
    Route::get('state', 'XhjobProduction/state');
    Route::get('result', 'XhjobProduction/result');
    Route::get('chainState', 'XhjobProduction/chainState');
    Route::get('chordState', 'XhjobProduction/chordState');
    Route::post('order', 'XhjobProduction/order');
    Route::post('report', 'XhjobProduction/report');
    Route::post('etl', 'XhjobProduction/etl');
    Route::post('cleanup', 'XhjobProduction/cleanup');
    Route::post('notify', 'XhjobProduction/notify');
    Route::post('delayed', 'XhjobProduction/delayed');
    Route::post('rateLimit', 'XhjobProduction/rateLimit');
    Route::post('persist', 'XhjobProduction/persist');
    Route::post('restart', 'XhjobProduction/restart');
    Route::post('stop', 'XhjobProduction/stop');
});
