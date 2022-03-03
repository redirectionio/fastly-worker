<?php

// file_put_contents('log',  __METHOD__ . ': ' . print_r($_SERVER, true) . "\n", FILE_APPEND);

if ($_SERVER['HTTP_HOST'] === 'agent.redirection.io') {
    // sleep(5);
}

echo $_SERVER['REQUEST_URI'];
echo "\n";
