<?php
class Product {
    public $name;
    public $price;

    function __construct($name, $price) {
        $this->name = $name;
        $this->price = $price;
    }

    function label() {
        return $this->name . " " . $this->price;
    }
}

class Offer extends Product {
    function label() {
        return parent::label() . " (offer)";
    }
}

$tea = new Offer("Tea", 3);
echo $tea->label(), "\n";
echo $tea instanceof Product ? "product\n" : "other\n";
echo get_class($tea), " extends ", get_parent_class($tea), "\n";
