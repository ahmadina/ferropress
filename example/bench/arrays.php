<?php
$a = [];
$i = 0;
while ($i < 8000) {
    $a[] = $i;
    $i = $i + 1;
}
$total = 0;
foreach ($a as $value) {
    $total = $total + $value;
}
echo count($a), " ", $total, "\n";
