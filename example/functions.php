<?php
function greet($name) {
    return "hello " . $name;
}

$exclaim = function ($text) {
    return $text . "!";
};

echo call_user_func($exclaim, greet("Ada")), "\n";

$n = 1;
$bump = function () use (&$n) {
    $n = $n + 1;
    return $n;
};
echo $bump(), $bump(), "\n";
