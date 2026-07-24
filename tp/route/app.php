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
// | stop / restart 通过 ?scope=daemon（显式）操作 daemon，默认 scope=task 操作单个任务（需 id）
// | demo 为状态变更操作（起停 daemon + 派发任务），使用 POST 避免 GET 触发
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
    Route::post('demo', 'XhjobTask/demo');
})->middleware(\app\middleware\XhjobAuth::class);
