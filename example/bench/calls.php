<?php
function add1($n) {
    return $n + 1;
}

$n = 0;
$i = 0;
while ($i < 120000) {
    $n = add1($n);
    $i = $i + 1;
}
echo $n, "\n";
