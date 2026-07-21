<?php
/**
 * xhjob Lifecycle API Example
 *
 * Demonstrates: pause / resume / cancel / remove / list
 * Reference: APScheduler pause_job / resume_job / remove_job, Celery revoke
 */

// Load extension if not already loaded
if (!extension_loaded('xhjob')) {
    echo "xhjob extension not loaded\n";
    exit(1);
}

xhjob_start();

echo "=== Lifecycle API Demo ===\n\n";

// 1. Dispatch a cron task
$id1 = Xhjob::task()
    ->viaShell('echo task1')
    ->cron('*/1 * * * * *')
    ->dispatch();
echo "1. Dispatched task: {$id1}\n";

// 2. Pause it
$ok = xhjob_pause($id1);
echo "2. Paused: " . ($ok ? "OK" : "FAIL") . "\n";
usleep(1500000);  // 1.5s
$state = xhjob_state($id1);
echo "   state after pause: state={$state['state']} paused=" . ($state['paused'] ? 'true' : 'false') . " count={$state['execution_count']}\n";

// 3. Resume it
$ok = xhjob_resume($id1);
echo "3. Resumed: " . ($ok ? "OK" : "FAIL") . "\n";
usleep(1500000);
$state = xhjob_state($id1);
echo "   state after resume: state={$state['state']} paused=" . ($state['paused'] ? 'true' : 'false') . " count={$state['execution_count']}\n";

// 4. Dispatch another task and cancel it
$id2 = Xhjob::task()
    ->viaShell('echo task2')
    ->cron('*/1 * * * * *')
    ->dispatch();
echo "4. Dispatched task: {$id2}\n";
$ok = xhjob_cancel($id2);
echo "   Cancelled: " . ($ok ? "OK" : "FAIL") . "\n";
usleep(500000);
$state = xhjob_state($id2);
echo "   state after cancel: state={$state['state']}\n";

// 5. Dispatch another task and remove it
$id3 = Xhjob::task()
    ->viaShell('echo task3')
    ->cron('*/1 * * * * *')
    ->dispatch();
echo "5. Dispatched task: {$id3}\n";
$ok = xhjob_remove($id3);
echo "   Removed: " . ($ok ? "OK" : "FAIL") . "\n";

// 6. List all tasks
echo "6. List all tasks:\n";
$json = xhjob_list();
$tasks = json_decode($json, true);
foreach ($tasks as $t) {
    echo "   - id={$t['id']} state={$t['state']} cron={$t['cron']} count={$t['execution_count']}\n";
}

// 7. List CANCELLED tasks
echo "7. List CANCELLED tasks:\n";
$json = xhjob_list('default', 'CANCELLED');
$tasks = json_decode($json, true);
foreach ($tasks as $t) {
    echo "   - id={$t['id']} state={$t['state']}\n";
}

// Cleanup
echo "\n=== Cleanup ===\n";
xhjob_remove($id1);
xhjob_stop();
echo "Done.\n";
