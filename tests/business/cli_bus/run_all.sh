#!/usr/bin/env bash
# php-cli 业务场景串联测试
#
# 模拟生产环境完整生命周期：
#   1. 启动服务（独立 daemon 进程）
#   2. 另一脚本连接服务投递任务
#   3. 重启服务（验证 PID 变化）
#   4. 停止服务（验证清理）
#
# 每个步骤是独立的 PHP 进程，daemon 必须跨进程存活。

set -e

# 定位项目根目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
SO_PATH="$PROJECT_ROOT/target/release/libxhjob.so"

if [ ! -f "$SO_PATH" ]; then
    echo "ERROR: 扩展未编译: $SO_PATH"
    echo "请先执行: cd $PROJECT_ROOT && cargo build --release"
    exit 1
fi

SERVICE_NAME="${1:-production-queue}"
PHP_OPTS="-d extension=$SO_PATH"

echo "============================================================"
echo "php-cli 业务场景串联测试"
echo "  扩展: $SO_PATH"
echo "  服务名: $SERVICE_NAME"
echo "  PHP: $(which php)"
echo "============================================================"

# 清理可能残留的旧服务
echo ""
echo "[cleanup] 清理可能残留的旧服务..."
php $PHP_OPTS -r "@xhjob_stop('$SERVICE_NAME');" 2>/dev/null || true
sleep 1

# 步骤 1: 启动服务
echo ""
echo "============================================================"
echo "[step 1] 启动服务 (独立 daemon 进程)"
echo "============================================================"
php $PHP_OPTS "$SCRIPT_DIR/01_start_service.php" "$SERVICE_NAME"
STEP1=$?
if [ $STEP1 -ne 0 ]; then
    echo "FAIL: step 1 失败 (exit=$STEP1)"
    exit $STEP1
fi

# 验证 daemon 跨进程存活：启动一个新 PHP 进程查询 status
echo ""
echo "[verify] 启动新 PHP 进程验证 daemon 仍存活..."
php $PHP_OPTS -r "
\$s = xhjob_status('$SERVICE_NAME');
echo '[verify] 新进程查询: running=' . \$s['running'] . ', pid=' . (\$s['pid'] ?? 'none') . PHP_EOL;
if (\$s['running'] !== 'true') { exit(1); }
"

# 步骤 2: 连接服务进行业务操作
echo ""
echo "============================================================"
echo "[step 2] 连接服务进行业务操作 (另一个 PHP 进程)"
echo "============================================================"
php $PHP_OPTS "$SCRIPT_DIR/02_operate.php" "$SERVICE_NAME"
STEP2=$?
if [ $STEP2 -ne 0 ]; then
    echo "FAIL: step 2 失败 (exit=$STEP2)"
    @xhjob_stop
    exit $STEP2
fi

# 步骤 3: 重启服务
echo ""
echo "============================================================"
echo "[step 3] 重启服务 (验证 PID 变化)"
echo "============================================================"
php $PHP_OPTS "$SCRIPT_DIR/03_restart_service.php" "$SERVICE_NAME"
STEP3=$?
if [ $STEP3 -ne 0 ]; then
    echo "FAIL: step 3 失败 (exit=$STEP3)"
    exit $STEP3
fi

# 步骤 4: 停止服务
echo ""
echo "============================================================"
echo "[step 4] 停止服务 (验证清理)"
echo "============================================================"
php $PHP_OPTS "$SCRIPT_DIR/04_stop_service.php" "$SERVICE_NAME"
STEP4=$?
if [ $STEP4 -ne 0 ]; then
    echo "FAIL: step 4 失败 (exit=$STEP4)"
    exit $STEP4
fi

# 最终验证
echo ""
echo "============================================================"
echo "[final] 全部步骤完成，最终状态验证"
echo "============================================================"
php $PHP_OPTS -r "
\$s = xhjob_status('$SERVICE_NAME');
echo '[final] 服务状态: running=' . \$s['running'] . PHP_EOL;
\$pidFile = sys_get_temp_dir() . '/xhjob.$SERVICE_NAME.pid';
echo '[final] PID 文件存在: ' . (file_exists(\$pidFile) ? 'YES (未清理!)' : 'NO (已清理)') . PHP_EOL;
\$sockFile = sys_get_temp_dir() . '/xhjob.$SERVICE_NAME.sock';
echo '[final] sock 文件存在: ' . (file_exists(\$sockFile) ? 'YES (未清理!)' : 'NO (已清理)') . PHP_EOL;
if (\$s['running'] === 'true') { echo 'FAIL: 服务仍运行'; exit(1); }
echo '[final] SUCCESS: 所有业务场景测试通过';
"

echo ""
echo "============================================================"
echo "全部业务场景测试通过"
echo "============================================================"
