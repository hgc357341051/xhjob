<?php
/**
 * Smoke test: verify the xhjob extension loads and registers all
 * expected functions + the Xhjob class.
 *
 * Usage:
 *   php -d extension=xhjob.so tests/smoke.php
 */
if (!extension_loaded('xhjob')) {
    fwrite(STDERR, "xhjob extension not loaded (use: php -d extension=xhjob.so " . __FILE__ . ")\n");
    exit(1);
}

$required = ['xhjob_start','xhjob_stop','xhjob_restart','xhjob_status',
             'xhjob_dispatch','xhjob_state','xhjob_result'];
foreach ($required as $fn) {
    if (!function_exists($fn)) {
        fwrite(STDERR, "Missing function: $fn\n");
        exit(1);
    }
    echo "OK: $fn\n";
}

if (!class_exists('Xhjob')) {
    fwrite(STDERR, "Missing class: Xhjob\n");
    exit(1);
}
echo "OK: class Xhjob\n";

// Verify chainable API is callable (does not dispatch — no daemon required)
$builder = Xhjob::task();
if (!is_object($builder)) {
    fwrite(STDERR, "Xhjob::task() did not return an object\n");
    exit(1);
}
echo "OK: Xhjob::task() chainable builder\n";

echo "XHJob smoke test passed\n";
