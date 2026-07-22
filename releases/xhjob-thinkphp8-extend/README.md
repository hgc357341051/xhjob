# xhjob ThinkPHP 8 扩展集成

## 文件结构
```
xhjob-thinkphp8-extend/
├── Xhjob/                          # extend 第三方库（复制到 tp/extend/Xhjob/）
│   ├── Exception/                  # 异常体系
│   ├── facade/Xhjob.php            # ThinkPHP Facade
│   ├── ServiceProvider.php         # ThinkPHP 服务提供者
│   ├── XhjobService.php             # daemon 生命周期管理
│   ├── TaskBuilder.php              # PHP fluent builder
│   ├── TaskManager.php              # 任务管理门面
│   ├── Client.php                  # 跨环境客户端
│   └── helper.php                  # 全局辅助函数
├── controller/
│   └── XhjobTask.php               # 控制器（25 个 action，复制到 tp/app/controller/）
├── route/
│   └── app.php                     # 路由（合并到 tp/route/app.php）
├── config/
│   └── xhjob.php                   # 配置（复制到 tp/config/）
├── service.php                     # ServiceProvider 注册（合并到 tp/app/service.php）
├── test_xhjob.php                  # CLI 测试脚本
└── README.md                       # 本文件
```

## 安装步骤

1. 安装 xhjob PHP 扩展（.so 文件）
2. 复制 `Xhjob/` 到 `tp/extend/Xhjob/`
3. 复制 `controller/XhjobTask.php` 到 `tp/app/controller/`
4. 合并 `route/app.php` 到 `tp/route/app.php`
5. 复制 `config/xhjob.php` 到 `tp/config/`
6. 在 `tp/app/service.php` 追加 `\Xhjob\ServiceProvider::class`
7. 运行测试：`php -d extension=xhjob.so test_xhjob.php`

## API 路由

| Method | Path | 说明 |
|--------|------|------|
| GET | /xhjob/index | 首页/状态 |
| GET | /xhjob/status | daemon 状态 |
| GET | /xhjob/health | 健康检查 |
| POST | /xhjob/start | 启动 daemon |
| POST | /xhjob/stop | 停止 daemon/任务 |
| POST | /xhjob/restart | 重启 daemon/任务 |
| GET | /xhjob/list | 任务列表 |
| GET | /xhjob/get | 任务详情 |
| POST | /xhjob/create | 创建任务（raw JSON） |
| POST | /xhjob/createShell | 创建 shell 任务 |
| POST | /xhjob/createHttp | 创建 HTTP 任务 |
| POST | /xhjob/createCron | 创建 cron 任务 |
| POST | /xhjob/createChain | 创建任务链 |
| POST | /xhjob/createGroup | 创建任务组 |
| GET | /xhjob/state | 任务状态 |
| GET | /xhjob/result | 任务结果 |
| GET | /xhjob/logs | 任务日志 |
| POST | /xhjob/update | 编辑任务 |
| POST | /xhjob/pause | 暂停任务 |
| POST | /xhjob/resume | 恢复任务 |
| POST | /xhjob/reschedule | 重新调度 |
| DELETE | /xhjob/delete | 删除任务 |
| GET | /xhjob/chainState | 链状态 |
| GET | /xhjob/groupState | 组状态 |
| GET | /xhjob/demo | 完整演示 |

## Facade 用法

```php
use Xhjob\facade\Xhjob;
use Xhjob\TaskBuilder;

$id = Xhjob::create(TaskBuilder::shell('echo hi')->cron('*/1 * * * *'));
$state = Xhjob::state($id);
$result = Xhjob::result($id);
```

## 测试结果

12 步 CLI 测试全部 PASS：
- daemon 启动/停止/重启/健康检查
- shell/cron/chain/group 任务创建与执行
- 任务列表/状态/结果/日志查询
- 任务停止（cancel）/重启（requeue）
- Client 跨环境客户端
