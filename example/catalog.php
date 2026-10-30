<?php
$cart = ["tea" => 2, "bread" => 1];
$cart["tea"] = $cart["tea"] + 1;
$cart["jam"] = 1;

echo implode(",", array_keys($cart)), "\n";

$total = 0;
foreach ($cart as $name => $qty) {
    echo $name, "=", $qty, "\n";
    $total = $total + $qty;
}
echo "items=", $total, "\n";
echo json_encode($cart), "\n";
