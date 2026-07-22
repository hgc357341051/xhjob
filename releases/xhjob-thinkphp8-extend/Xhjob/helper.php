<?php
// +----------------------------------------------------------------------
// | Xhjob 扩展 - 全局辅助函数（可选）
// +----------------------------------------------------------------------
// | 提供 xhjob_task() / xhjob_manager() / xhjob_service() 等快捷函数，
// | 需在 composer.json 的 autoload.files 中显式引入，或在入口手动 require
// +----------------------------------------------------------------------
declare(strict_types=1);

if (!function_exists('xhjob_manager')) {
    /**
     * 获取容器中的 TaskManager 实例
     *
     * 需先注册 Xhjob\ServiceProvider。
     *
     * @return \Xhjob\TaskManager
     */
    function xhjob_manager(): \Xhjob\TaskManager
    {
        return app('xhjob.manager');
    }
}

if (!function_exists('xhjob_service')) {
    /**
     * 获取容器中的 XhjobService 实例
     *
     * @return \Xhjob\XhjobService
     */
    function xhjob_service(): \Xhjob\XhjobService
    {
        return app('xhjob.service');
    }
}

if (!function_exists('xhjob_task')) {
    /**
     * 快捷创建并派发 shell 任务
     *
     * @param string      $cmd     shell 命令
     * @param string|null $service 服务名（null 走容器默认）
     * @param string|null $dataDir 数据目录
     *
     * @return string task_id
     */
    function xhjob_task(string $cmd, ?string $service = null, ?string $dataDir = null): string
    {
        if ($service !== null) {
            $mgr = new \Xhjob\TaskManager($service, $dataDir);
        } else {
            $mgr = xhjob_manager();
        }
        return $mgr->create(\Xhjob\TaskBuilder::shell($cmd));
    }
}
