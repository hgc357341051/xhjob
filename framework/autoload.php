<?php
/**
 * xhjob 框架简易自动加载
 *
 * 加载 framework/ 目录下的所有 PHP 类文件。
 * 使用方式：require_once __DIR__ . '/framework/autoload.php';
 */

spl_autoload_register(function ($class) {
    $map = [
        'XhjobFrameworkException'    => __DIR__ . '/exceptions.php',
        'ServiceNotRunningException' => __DIR__ . '/exceptions.php',
        'TaskNotFoundException'      => __DIR__ . '/exceptions.php',
        'InvalidTaskConfigException' => __DIR__ . '/exceptions.php',
        'IpcException'               => __DIR__ . '/exceptions.php',
        'XhjobService'               => __DIR__ . '/XhjobService.php',
        'TaskBuilder'                => __DIR__ . '/TaskBuilder.php',
        'TaskManager'                => __DIR__ . '/TaskManager.php',
        'Client'                     => __DIR__ . '/Client.php',
        'HttpApi'                    => __DIR__ . '/HttpApi.php',
    ];
    if (isset($map[$class]) && file_exists($map[$class])) {
        require_once $map[$class];
    }
});
