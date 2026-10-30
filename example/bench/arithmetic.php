<?php
$n = 0;
$i = 0;
while ($i < 400000) {
    $n = $n + ($i * 3) - ($i % 2);
    $i = $i + 1;
}
echo $n, "\n";
