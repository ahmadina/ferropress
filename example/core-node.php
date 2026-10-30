<?php
class Item extends CoreNode {
    function label() {
        return "[" . parent::label() . "]";
    }
}

$item = new Item();
$item->tag("leaf");
echo $item->label(), "\n";
echo get_class($item), " / ", get_parent_class($item), "\n";
echo $item instanceof CoreNode ? "core\n" : "other\n";

$copy = unserialize(serialize($item));
echo $copy->label(), "\n";
