<?php
function title_case($text) {
    return strtoupper($text);
}

function wrap($text) {
    return "[" . $text . "]";
}

function shout($text) {
    return $text . "!";
}

bridge_add("title_case");
bridge_add("wrap");
bridge_add("shout");
echo bridge_apply("hello"), "\n";
echo preg_replace("/l+/", "L", "hello"), "\n";
