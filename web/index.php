<?php
/**
 * xhjob HTTP API Web 入口（php-fpm / Apache / Nginx）
 *
 * 引入框架自动加载，创建 HttpApi 实例并处理当前 HTTP 请求。
 * 服务名和数据目录通过环境变量配置：
 *   XHJOB_SERVICE   服务名（默认 default）
 *   XHJOB_DATA_DIR   数据目录（默认使用平台路径）
 *   XHJOB_API_TOKEN  鉴权 token（未设置则跳过鉴权，开发模式）
 *
 * 配合 web/.htaccess 的 rewrite 规则，所有非文件请求转发到本文件。
 */

require_once __DIR__ . '/../framework/autoload.php';

$api = new HttpApi(
    getenv('XHJOB_SERVICE') ?: 'default',
    getenv('XHJOB_DATA_DIR') ?: null
);
$api->handle();
