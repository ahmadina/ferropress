<?php
$s = "abcdefghijklmnopqrstuvwxyz";
$n = 0;
$i = 0;
while ($i < 80000) {
    $n = $n + strlen($s) + strpos($s, "m");
    $n = $n + (str_contains($s, "q") ? 1 : 0);
    $i = $i + 1;
}
echo $n, "\n";
