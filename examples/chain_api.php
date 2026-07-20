<?php
/**
 * Example: Chainable API showcase.
 *
 * Demonstrates every chainable method exposed by the Xhjob class.
 *
 * Usage:
 *   php -d extension=xhjob.so examples/chain_api.php
 */

if (!xhjob_start()) {
    fwrite(STDERR, "Failed to start xhjob daemon\n");
    exit(1);
}

// Build a shell task with every option set
$id1 = Xhjob::task()
    ->viaShell('echo "chain-api-demo"')
    ->withRetry(2, 1)
    ->timeout(10)
    ->priority(5)
    ->allowOverlap(true)
    ->maxInstances(3)
    ->coalesce(true)
    ->persist(false)
    ->dispatch();

echo "Shell task dispatched: {$id1}\n";

// Build an HTTP task with retry + timeout
$id2 = Xhjob::task()
    ->viaHttp('GET', 'https://httpbin.org/get?demo=chain')
    ->withRetry(1, 1)
    ->timeout(15)
    ->priority(10)
    ->dispatch();

echo "HTTP task dispatched: {$id2}\n";

// Poll state of both tasks
foreach ([$id1, $id2] as $id) {
    for ($i = 0; $i < 50; $i++) {
        $s = xhjob_state($id);
        $state = $s['state'] ?? 'UNKNOWN';
        if ($state === 'SUCCESS' || $state === 'FAILED') {
            echo "Task {$id}: state={$state}\n";
            $r = xhjob_result($id);
            if (isset($r['exit_code'])) echo "  exit_code={$r['exit_code']}\n";
            if (isset($r['status_code'])) echo "  status_code={$r['status_code']}\n";
            break;
        }
        usleep(100000);
    }
}

// Cleanup
xhjob_stop();
echo "Done.\n";
