<?php
function parse_qty($value) {
    if ($value < 1) {
        throw new Exception("quantity must be at least 1");
    }
    return $value;
}

try {
    echo parse_qty(0);
} catch (Exception $error) {
    echo "caught: ", $error->message, "\n";
}

echo parse_qty(2), "\n";
