<?php
require "lib/greet.php";
$g = new Greeter();
echo $g->greet("Ada"), "\n";
echo file_exists("lib/greet.php") ? "loaded\n" : "missing\n";
