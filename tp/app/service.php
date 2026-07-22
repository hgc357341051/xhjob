<?php

use app\AppService;

// 系统服务定义文件
// 服务在完成全局初始化之后执行
return [
    AppService::class,
    // Xhjob 定时任务扩展服务（注册 xhjob.service / xhjob.manager 到容器）
    \Xhjob\ServiceProvider::class,
];
