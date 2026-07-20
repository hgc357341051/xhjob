<?php
/**
 * Example: Overlap control.
 *
 * Demonstrates the difference between allowOverlap(false) and allowOverlap(true).
 *
 * Usage:
 *   php -d extension=xhjob.so examples/overlap.php
 */

if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// Slow shell task that sleeps 3 seconds.
$slowCmd = PHP_OS_FAMILY === 'Windows'
    ? 'cmd /C ping -n 4 127.0.0.1 >NUL'
    : 'bash -c "sleep 3"';

// Dispatch with allowOverlap=false — cron triggers during the 3s sleep
// will be silently skipped (no concurrent execution).
$noOverlapId = Xhjob::task()
    ->viaShell($slowCmd)
    ->cron('*/1 * * * * *')      // every 1 second
    ->allowOverlap(false)         // default: max_instances=1
    ->persist(false)
    ->timeout(10)
    ->dispatch();

echo "No-overlap task dispatched: {$noOverlapId}\n";
echo "Letting cron attempt to fire multiple times during the slow task...\n";

sleep(5);

$s = xhjob_state($noOverlapId);
echo "Final state: {$s['state']}\n";
echo "Attempts:    {$s['attempts']}\n";

// Dispatch with allowOverlap=true + maxInstances=3
$overlapId = Xhjob::task()
    ->viaShell($slowCmd)
    ->cron('*/1 * * * * *')
    ->allowOverlap(true)
    ->maxInstances(3)             // allow up to 3 concurrent instances
    ->persist(false)
    ->timeout(10)
    ->dispatch();

echo "Overlap-allowed task dispatched: {$overlapId}\n";
echo "Wait for it to fire...\n";

sleep(5);
$s = xhjob_state($overlapId);
echo "Final state: {$s['state']}\n";

xhjob_stop();
echo "Done.\n";
