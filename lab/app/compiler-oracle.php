<?php
/*
 * Ferropress lab compiler oracle: prints what the reference PHP reports while compiling source text
 * without running it, as JSON.
 *
 * Every case is written to a file of its own and compiled in lint mode (`php -l`), many files per
 * process; lint mode does not carry classes or functions from one file into the next. For each case
 * the oracle records the diagnostics compiling it reported, in order: severity ("Fatal error",
 * "Warning", "Deprecated", ...), message and line. Lint mode stops a file at its first fatal error.
 * A message naming the case's own file has that name replaced by `%file%`.
 *
 * Cases are hand-written cases grouped by the area of the compiler they probe, followed by every
 * parser oracle case that parses.
 *
 * Run by `cargo xtask lab compiler`; never part of Ferropress.
 */

declare(strict_types=1);

require __DIR__ . '/parser-oracle.php';

const COMPILER_SCHEMA = 1;
const COMPILER_BATCH  = 250;
const COMPILER_FILE   = '%file%';

/** Hand-written cases: topic => list of code following "<?php ". */
function compiler_topic_cases(): array {
	return array(
		'write context'      => array(
			'$a[];', 'echo $a[];', 'f($a[]);', 'f() = 1;', '$a->m() = 1;', 'A::m() = 1;', '$a?->b = 1;', '$a?->b[] = 1;',
			'$a?->b ??= 1;', 'foreach ($a as $b?->c) {}', '$a = &$b?->c;', '$a = &$b?->c->d;', 'f(1)[] = 2;',
			'strlen($a) = 1;', 'strlen($a)[0] = 1;', '++f();', 'f()++;', '"abc"[0] = 1;', '[1, 2][0] = 1;', 'A::B[0] = 1;',
			'(1 + 2)[] = 3;', 'unset($a[]);', 'unset(f()[0]);', 'isset(f());', 'isset(1 + 1);', 'isset($a, A::B);',
			'empty(f());', '$a = &f();', '$a = &$b->m();', '$a = &new A;', '$a = &A::B;', '$a::f() = 1;', '$a->b() .= 1;',
			'[$a, f()] = $b;', 'list($a, $b->m()) = $c;', 'foreach ($a as f()) {}', 'foreach ($a as $k => f()) {}',
			'$a->b?->c = 1;', 'unset($a?->b);', 'f(...)[0] = 1;',
		),
		'this'               => array(
			'$this = 1;', 'function f() { $this = 1; }', 'class A { function f() { $this = 1; } }', '[$this] = $a;',
			'list($a, $this) = $b;', '$this .= 1;', '$this++;', '--$this;', 'unset($this);', 'global $this;',
			'function f() { static $this; }', 'function f($this) {}', 'fn($this) => 1;', 'function () use ($this) {};',
			'foreach ($a as $this) {}', 'foreach ($a as $this => $v) {}', 'try {} catch (E $this) {}', '$a = &$this;',
			'$this = &$a;', '$this[] = 1;', '$this->a = 1;', 'unset($this->a);', '$$this = 1;', 'class A { function f() { $this = &$a; } }',
		),
		'globals'            => array(
			'$GLOBALS = 1;', '$GLOBALS[] = 1;', '$GLOBALS["a"] = 1;', '$GLOBALS["a"]["b"] = 1;', '$a = &$GLOBALS;',
			'$a = &$GLOBALS["a"];', 'foreach ($a as $GLOBALS) {}', 'unset($GLOBALS);', 'unset($GLOBALS["a"]);',
			'$GLOBALS .= 1;', '[$GLOBALS] = $a;', 'list($GLOBALS) = $a;', '$GLOBALS++;', 'function f(&$a) {} f($GLOBALS);',
			'function () use ($_GET) {};', 'function () use ($GLOBALS) {};', 'function f() { global $_GET; }',
			'function f() { static $_GET; }', 'function f($_GET) {}', '$_GET = 1;', 'unset($_GET);', '$GLOBALS->a = 1;',
		),
		'lexical variables'  => array(
			'function () use ($a, $a) {};', 'function ($a) use ($a) {};', 'function () use (&$a, $a) {};',
			'function f($a, $a) {}', 'fn($a, $a) => 1;', 'function f($a, ...$a) {}',
		),
		'list assignment'    => array(
			'[] = $a;', 'list() = $a;', '[, ] = $a;', '[$a, , $b] = $c;', '["a" => $a, $b] = $c;', '[$a, "b" => $b] = $c;',
			'[[$a], list($b)] = $c;', 'list([$a]) = $b;', '[...$a] = $b;', '[$a, ...$b] = $c;', '[&$a, $b] = $c;',
			'["k" => &$a] = $b;', 'list($a);', '[$a] = [$b] = $c;', '[1, , 2];', 'array(1, , 2);', '$a = [, 1];',
			'array() = $a;', 'foreach ($a as &$k => $v) {}', 'foreach ($a as list($k) => $v) {}', 'foreach ($a as [$k] => $v) {}',
			'foreach ($a as [$b, [$c]]) {}', 'foreach ($a as [$b, list($c)]) {}', 'foreach ($a as ["k" => $b, $c]) {}',
			'foreach ($a as [, $b]) {}', 'foreach ($a as []) {}', '[$a, [, ]] = $b;', '["a" => $a, , ] = $b;',
			'[$a, $b] = &$c;', '[[$a, &$b]] = $c;', '$a = [&$b => 1];', 'list(, $a) = $b;', '[$a->b, $c[0]] = $d;',
		),
		'jumps'              => array(
			'break;', 'continue;', 'while (1) { break 2; }', 'while (1) { break 0; }', 'while (1) { continue 0; }',
			'while (1) { break 1; }', 'while (1) { while (1) { break 2; } }', 'while (1) { while (1) { break 3; } }',
			'switch (1) { case 1: continue; }', 'while (1) { switch (1) { case 1: continue; } }',
			'while (1) { switch (1) { case 1: continue 1; } }', 'switch (1) { case 1: switch (2) { case 2: continue 2; } }',
			'while (1) { switch (1) { case 1: continue 2; } }', 'foreach ($a as $b) { switch ($b) { default: continue; } }',
			'function f() { break; }', 'while (1) { function f() { break; } }', 'while (1) { $f = function () { continue; }; }',
			'goto a;', 'a: a: ;', "a:\nb:\na:", 'goto a; while (1) { a: }', 'goto a; switch (1) { case 1: a: }',
			'goto a; foreach ($b as $c) { a: }', 'while (1) { a: } goto a;', 'try {} finally { goto a; } a:',
			'goto a; try {} finally { a: }', 'try { goto a; } finally {} a:', 'try {} finally { return; }',
			'while (1) { try {} finally { break; } }', 'while (1) { try {} finally { continue; } }',
			'try {} finally { while (1) { break; } }', 'try {}', 'try { } catch (E $e) { goto a; } a:',
			'function f() { goto a; } a:', 'a: function f() { goto a; }', 'do { break; } while (0);', 'for (;;) { continue 2; }',
		),
		'declare'            => array(
			'declare(strict_types=1);', 'echo 1; declare(strict_types=1);', 'declare(strict_types=2);',
			'declare(strict_types=1) {}', 'declare(strict_types=1): enddeclare;', 'declare(ticks=1);', 'declare(ticks=1) { echo 1; }',
			'declare(foo=1);', 'declare(encoding="UTF-8");', 'echo 1; declare(encoding="UTF-8");', 'declare(ticks=$a);',
			'declare(strict_types=$a);', 'namespace A; declare(strict_types=1);', 'declare(strict_types=1); namespace A;',
			'declare(encoding="foo");', 'declare(strict_types=1, ticks=1);', 'declare(ticks=1, strict_types=1);',
			'declare(STRICT_TYPES=1);', 'declare(strict_types="1");', 'declare(strict_types=1.0);', 'function f() { declare(strict_types=1); }',
			'declare(ticks=1); declare(strict_types=1);', 'declare(strict_types=0); declare(strict_types=1);',
		),
		'namespaces'         => array(
			'echo 1; namespace A;', 'namespace A; namespace B;', 'namespace A {} namespace B;', 'namespace A; namespace B {}',
			'namespace A {} echo 1;', 'namespace A {} namespace {}', 'namespace self;', 'namespace parent;', 'namespace static;',
			'namespace A\self;', 'namespace Self\A;', "namespace A;\nuse B\\C;\nuse D\\C;", 'use A;', 'use \A;', 'use A as B;',
			'use A\B; class B {}', 'class B {} use A\B;', 'use function A\f; function f() {}', 'function f() {} use function A\f;',
			'use const A\C; const C = 1;', 'use A\self;', 'use A\B as parent;', 'use A\B as static;', 'use function A\B as self;',
			'namespace A; use B\A;', 'namespace A; class B {} use C\B;', 'use A\B, C\B;', 'use function A\f, B\f;',
			'use const A\C, B\C;', 'use A\B; use function C\B;', 'use A\{B, C\B};', 'namespace A; use function B\f; function f() {}',
			'namespace A; function f() {} use function B\f;', 'use A\B as b; use C\B;', 'namespace A { namespace B {} }',
			"namespace A;\n\nfunction f() {}\nuse function B\\f;", 'use A\B as _;', 'namespace _;', 'namespace A\_;',
		),
		'class names'        => array(
			'class self {}', 'class parent {}', 'class int {}', 'class string {}', 'class mixed {}', 'class never {}',
			'class iterable {}', 'class object {}', 'class void {}', 'class null {}', 'class false {}', 'class true {}',
			'interface int {}', 'trait void {}', 'enum bool {}', 'class _ {}', 'interface _ {}', 'trait _ {}', 'enum _ {}',
			'class A extends self {}', 'class A extends int {}', 'class A implements self {}', 'class A implements int {}',
			'interface I extends self {}', 'class A extends static {}', 'class A extends parent {}', 'new self;', 'new parent;',
			'new static;', 'parent::f();', 'self::f();', 'static::f();', 'class A { function f() { parent::f(); } }',
			'class A extends B { function f() { parent::f(); } }', 'function f(): static {}', 'function f(): self {}',
			'function f(parent $a) {}', 'function f(): parent {}', 'class A { function f(): static {} }', 'function f(self $a) {}',
			'try {} catch (static $e) {}', 'try {} catch (self $e) {}', 'try {} catch (int $e) {}', 'A::class;', '$a::class;',
			'const A = static::class;', 'const A = self::class;', 'class A { const B = parent::class; }', 'class A { const B = static::C; }',
			'class A { const B = static::class; }', 'const A = $a::class;', '\self::f();', 'namespace\self::f();', '\parent::f();',
			'new class extends self {};', 'f()::class;', '(1)::class;', '"A"::class;', 'A::class::class;', 'class A { const B = self::class; }',
			'self::class;', 'static::class;', 'parent::class;', 'function f() { return self::class; }', 'class A { function f() { return parent::class; } }',
			'$a instanceof self;', '$a instanceof int;', 'new int;', 'int::f();', 'class A { use self; }', 'class A { use int; }',
			'namespace A; class int {}', 'class A extends \int {}', 'function f(): \int {}', 'function f(\self $a) {}',
		),
		'functions'          => array(
			'function f() {} function f() {}', 'function F() {} function f() {}', "function f() {}\n\nfunction f() {}",
			'if (1) { function f() {} } function f() {}', 'function f() {} if (1) { function f() {} }', 'function __autoload() {}',
			'namespace A; function __autoload() {}', 'function f(...$a, $b) {}', 'function f(...$a = 1) {}', 'function f($a = 1, $b) {}',
			'function f(?int $a = 1, $b) {}', 'function f(int $a = null) {}', 'function f(int $a = null, $b) {}', 'function f($a = null, $b) {}',
			'function f(?int $a = null, $b) {}', 'function f(int|null $a = null, $b) {}', 'function f(mixed $a = null, $b) {}',
			'function f(int $a = NULL) {}', 'function f(int $a = \null) {}', 'function f(A $a = null) {}', 'class A { function f(int $a = null) {} }',
			'function f(int $a = null, ...$b) {}', 'function f($a = 1, ...$b) {}', 'function f($a = 1, $b = 2, $c) {}',
			'function f(): void { return 1; }', 'function f(): void { return null; }', 'function f(): void { return; }', 'function f(): int { return; }',
			'function f(): ?int { return; }', 'function f(): mixed { return; }', 'function f(): never { return; }', 'function f(): never { return 1; }',
			'function f(): never {}', 'function f(void $a) {}', 'function f(never $a) {}', 'function f(): ?mixed {}', 'function f(): ?null {}',
			'function f(): null {}', 'function f(): ?void {}', 'function f(): int|int {}', 'function f(): int|INT {}', 'function f(): bool|false {}',
			'function f(): true|false {}', 'function f(): mixed|int {}', 'function f(): void|int {}', 'function f(): never|int {}',
			'function f(): object|A {}', 'function f(): iterable|array {}', 'function f(): A|a {}', 'function f(): A&B&A {}', 'function f(): A&int {}',
			'function f(): A&self {}', 'function f(): (A&B)|A {}', 'function f(): (A&B)|(A&B&C) {}', 'function f(): (A&B)|object {}',
			'function f(): \int {}', 'function f(): \mixed {}', 'function f(): integer {}', 'function f(): resource {}', 'function f(): boolean {}',
			'function f(): double {}', 'function f(): \integer {}', 'function f(integer $a) {}', 'function f(?integer $a) {}', 'function f(): integer|string {}',
			'namespace A; function f(): integer {}', 'function f(): iterable {}', 'function f(): ?iterable {}', 'function f(): int|iterable {}',
			'function f(): false {}', 'function f(): ?false {}', 'function f(): true|null {}', 'function f(): null|false {}', 'function f(): static|object {}',
			'function &f(): void {}', 'function &f(): never {}', 'function &f() { yield from g(); }', 'function f() { yield from g(); }', 'yield 1;',
			'yield;', 'function f(): int { yield 1; }', 'function f(): iterable { yield; }', 'function f(): Traversable { yield; }',
			'function f(): Generator|int { yield; }', 'function f(): mixed { yield; }', 'function f(): object { yield; }', 'function f(): ?Iterator { yield; }',
			'function f(): array { yield; }', 'function f(): void { yield; }', 'function f(): never { yield; }', 'function f(): static { yield; }',
			'function f() { static $a; static $a; }', 'function f() { static $a, $a; }', 'function f() { global $a, $a; }', 'function f($a) { static $a; }',
			'function f() { $http_response_header; }', '$http_response_header;', 'function f() { $a = $http_response_header; }',
			'function f() { echo `ls`; }', '`ls`;', '`ls $a`;', 'function f(): int { return 1; } function g(): void { return f(); }',
			'fn(): void => 1;', 'fn(): never => 1;', 'function f(): void { return f(); }', 'function f(): void { $f = fn() => 1; }',
			'function f(): never { $f = function () { return 1; }; }', 'function f(): int { $f = function () { return; }; return 1; }',
		),
		'arguments'          => array(
			'f(a: 1, 2);', 'f(...$a, 1);', 'f(...$a, b: 1);', 'f(a: 1, ...$b);', 'f(a: 1, a: 2);', 'f(A: 1, a: 2);', 'new A(a: 1, 2);',
			'$a->m(a: 1, 2);', 'A::m(...$a, 1);', 'new A(...);', 'new class {}(...);', '$a?->b(...);', '$a?->b->c(...);', 'f(...)(...);',
			'A::m(...);', 'f(...$a, ...$b);', 'f(1, ...$a, ...$b);', 'isset($a, ...$b);', 'f(a: 1, ...$b, c: 2);', 'exit(a: 1);',
			'compact(...$a);', 'func_get_args(...);', 'f(...)?->a;', 'new A(...$a, ...$b);', 'f(&$a);', 'f(1, ...);',
		),
		'operators'          => array(
			'1 |> fn($x) => $x |> g(...);', '1 |> (fn($x) => $x) |> g(...);', '1 |> fn($x) => $x;', '$a = 1 ? 2 : 3 ? 4 : 5;',
			'1 ?: 2 ? 3 : 4;', '1 ? 2 : 3 ?: 4;', '1 ?: 2 ?: 3;', '1 ? 2 : (3 ? 4 : 5);', '(1 ? 2 : 3) ? 4 : 5;', '1 ? 2 ? 3 : 4 : 5;',
			'match (1) { default => 1, default => 2 };', 'match (1) { 1, default => 2 };', 'match (1) { default, 1 => 2 };',
			'switch (1) { default: default: }', 'switch (1) { case 1; }', 'switch (1) { default; }', 'switch (1): case 1; endswitch;',
			'(unset) $a;', '(void) $a;', '(void) f();', '$a = (void) 1;', 'f((void) 1);', 'echo (real) 1;', '(object) [];',
			'clone $a = 1;', '$a = clone;', '1 <=> 2;', '$a ??= $b ??= 1;', '$a instanceof (1);', '1 instanceof A;', '"a" instanceof A;',
			'new (1);', 'new ($a);', 'new (f());', 'clone(...);', 'print(...);', 'empty(...);', 'eval(...);',
		),
		'interpolation'      => array(
			'"${a}";', '"${a[1]}";', '"${a["b"]}";', '"${$a}";', '"${a}${b}";', "\"\n\n\${a}\";", '"${a}" . "${b}";', '`${a}`;',
			"<<<A\n\${a}\nA;", "<<<A\n\${\$a}\nA;", '"${(a)}";', '"${a->b}";', '"{$a}";', '"$a";', '"${a::B}";', '"${f()}";',
		),
		'constants'          => array(
			'const A = 1; const A = 2;', 'const A = 1, A = 2;', 'namespace B; const A = 1; const A = 2;', "const A = 1;\n\nconst a = 2;",
			'const A = $a;', 'const A = f();', 'const A = new A;', 'const A = new A($a);', 'const A = new class {};', 'const A = [...$a];',
			'const A = [...[1]];', 'const A = $a::B;', 'const A = function () {};', 'const A = static function () {};',
			'const A = static function () use ($a) {};', 'const A = static fn() => 1;', 'const A = fn() => 1;', 'const A = $f(...);',
			'const A = strlen(...);', 'const A = A::f(...);', 'const A = A::$f(...);', 'const A = new $a;', 'const A = new (A);',
			'const A = (object) [];', 'const A = (int) "1";', 'const A = (array) 1;', 'const A = (bool) 1;', 'const A = (unset) 1;',
			'const A = [1, 2][$a];', 'const A = f(...$a);', 'const A = strlen(...$a);', 'const A = 1 <=> 2;', 'const A = $a ?? 1;',
			'const A = A::B ?? 1;', 'const A = [1][0] ?? 1;', 'const A = $a = 1;', 'const A = $a++;', 'const A = isset($a);', 'const A = empty($a);',
			'const A = A::class;', 'const A = __CLASS__;', 'const A = __FUNCTION__;', 'const A = __LINE__;', 'const A = "$a";', 'const A = `ls`;',
			'const A = print 1;', 'const A = include "a";', 'const A = A?->b;', 'const A = A->b;', 'const A = A::B->c;', 'const A = A::B?->c;',
			'const A = A::B::C;', 'const A = A::{B};', 'const A = A::{"B"};', 'const A = A::$b;', 'const A = $a[0];', 'const A = [1][0];',
			'const A = (A)::B;', 'const A = f(...);', 'const A = A::f(1);', 'const A = A::B(...);', 'const A = new A(...$b);',
			'const A = match (1) { default => 1 };', 'const A = throw 1;', 'const A = exit;', 'const A = clone $a;', 'const A = list($a) = [];',
			'const A = 1 instanceof A;', 'const A = yield;', 'const A = -A::B;', 'const A = !A;', 'const A = 1 ?: 2;', 'const A = 1 |> f(...);',
			'function f($a = $b) {}', 'function f($a = [1, ...$b]) {}', 'function f($a = new A) {}', 'function f($a = new class {}) {}',
			'function f(int $a = "x") {}', 'function f(int $a = 1.5) {}', 'function f(float $a = 1) {}', 'function f(string $a = 1) {}',
			'function f(bool $a = 0) {}', 'function f(array $a = 1) {}', 'function f(?array $a = "x") {}', 'function f(int|string $a = 1.5) {}',
			'function f(iterable $a = 1) {}', 'function f(callable $a = 1) {}', 'function f(object $a = 1) {}', 'function f(A $a = 1) {}',
			'function f(int $a = []) {}', 'function f(false $a = true) {}', 'function f(true $a = false) {}', 'function f(?int $a = "1") {}',
			'function f(int $a = A) {}', 'function f(int $a = PHP_INT_MAX + 1) {}', 'function f(float $a = PHP_INT_MAX) {}',
			'function f(mixed $a = 1) {}', 'function f(static $a = 1) {}', 'function f(int $a = -1) {}', 'function f(string $a = 1 . 2) {}',
			'class A { public int $a = "x"; }', 'class A { public ?int $a = "x"; }', 'class A { public int $a = null; }',
			'class A { public float $a = 1; }', 'class A { public array $a = 1; }', 'class A { public iterable $a = 1; }',
			'class A { public object $a = 1; }', 'class A { public A $a = 1; }', 'class A { public mixed $a = null; }', 'class A { public $a = new B; }',
			'class A { public static $a = new B; }', 'class A { const int B = "x"; }', 'class A { const ?int B = null; }', 'class A { const int B = null; }',
			'class A { const float B = 1; }', 'class A { const callable B = 1; }', 'class A { const void B = 1; }', 'class A { const never B = 1; }',
			'class A { const static B = 1; }', 'class A { const self B = 1; }', 'class A { const mixed B = 1; }', 'class A { const iterable B = 1; }',
			'class A { const B = new C; }', 'class A { const B = 1, C = 2; }', '#[A] const B = 1, C = 2;', '#[A] const B = 1;',
			'class A { #[X] const B = 1, C = 2; }', 'class A { const B = self::B; }', 'class A { const B = 1; const B = 2; }',
			'class A { const B = 1; const b = 2; }', 'interface I { private const A = 1; }', 'interface I { protected const A = 1; }',
			'class A { final private const B = 1; }', 'class A { final protected const B = 1; }', 'const __COMPILER_HALT_OFFSET__ = 1;',
			'namespace A; const __COMPILER_HALT_OFFSET__ = 1;', 'const PHP_VERSION = 1;', 'const E_ALL = 1;', 'const A = 1; echo A;',
			'function f() { static $a = $b; }', 'function f() { static $a = f(); }', 'function f() { static $a = new A; }',
			'function f() { static $a = [...$b]; }', 'function f() { static $a = 1, $b = $a; }', 'class A { public $a = $b; }',
			'class A { public $a = f(); }', 'class A { public $a = self::B; }', 'class A { public $a = static::B; }', 'enum E { case A = 1 + 1; }',
			'enum E: int { case A = 1 + 1; }', 'enum E: int { case A = "x"; }', 'enum E: string { case A = 1; }', 'enum E: int { case A = $a; }',
			'enum E: int { case A = self::B; }', 'enum E: int { case A = 1; case B = 1; }', 'enum E: int { case A = E::B; }',
		),
		'constant folding'   => array(
			'const A = true ? 1 : $a;', 'const A = false ? $a : 1;', 'const A = $a ? 1 : 2;', 'const A = 0 ?: $a;', 'const A = 1 ?: $a;',
			'const A = null ?? $a;', 'const A = 1 ?? $a;', 'const A = [1, 2][1];', 'const A = [1][$a];', 'const A = "abc"[1];',
			'const A = "abc"[5][$a];', 'const A = [...1];', 'const A = [...null];', 'const A = [...[1], ...$a];', 'const A = [[] => 1];',
			'const A = [1.5 => 1];', 'const A = [null => 1];', 'const A = [true => 1, $a];', 'const A = [&$a];', 'const A = [1, [$a]];',
			'$a = [...1];', '$a = [[] => 1];', 'const A = 1 + [];', 'const A = 1 / 0;', 'const A = 1 % 0;', 'const A = "a" . [];',
			'const A = 1 << -1;', 'const A = "x" + 1;', 'const A = true && $a;', 'const A = false && $a;', 'const A = true || $a;',
			'const A = false || $a;', 'const A = 1 > $a;', 'const A = 2 > 1;', 'const A = -[];', 'const A = -$a;', 'const A = !$a;',
			'const A = ~1.5;', 'const A = ~"a";', 'const A = (int) $a;', 'const A = (string) [];', 'const A = (string) 1.5;',
			'const A = (int) 1e100;', 'const A = (1e1000 - 1e1000) ? 1 : 2;', 'const A = (bool) (1e1000 - 1e1000);',
			'const A = !(1e1000 - 1e1000);', 'const A = (1e1000 - 1e1000) xor 1;', 'const A = (1e1000 - 1e1000) && 1;',
			'const A = __LINE__ . $a;', 'const A = @1;', 'const A = 1 and $a;', 'const A = null ?? $a ?? 1;',
			'function f($a = true ? $b : 1) {}', 'function f($a = false ? $b : 1) {}', 'enum E: int { case A = true ? 1 : $a; }',
			'enum E: int { case A = $a ? 1 : 2; }', '#[A($a)] function f() {}', '#[A(new B)] function f() {}', '#[A(1 + 1)] function f() {}',
			'class A { public $a = true ? 1 : $b; }', 'class A { const B = [1, 2][$a]; }', 'class A { public $a = (object) []; }',
			'class A { const B = (object) []; }', 'function f($a = (object) []) {}', 'const A = new A(...);', 'const A = A::f(...$a);',
			'const A = new A(b: 1, 2);', 'const A = [1, , 2];', "const A = [1,\n\n, 2];", 'const A = list($a);', 'const A = $a[];',
			'const A = A::B[];', 'const A = [][];', 'const A = static function () { return $this; };', 'const A = static function () { yield; };',
			'const A = (1)::class;', 'const A = parent::class;', 'const A = self::class;', 'class A { const B = parent::C; }',
			'class A { public $a = new static; }', 'function f($a = new static) {}', 'function f($a = new parent) {}',
			'const A = \\strlen(...);', 'const A = namespace\\f(...);', 'const A = A::{"f"}(...);', 'const A = [1, 2] + [3];',
		),
		'types'              => array(
			'function f(): ?mixed {}', 'function f(): ?null {}', 'function f(): void|null {}', 'function f(): ?never {}',
			'function f(): int|INT {}', 'function f(): bool|false {}', 'function f(): true|false {}', 'function f(): mixed|int {}',
			'function f(A|a $a) {}', 'function f(iterable|Traversable $a) {}', 'function f(A|object $a) {}', 'function f(iterable|object $a) {}',
			'function f(false|null $a, ?false $b) {}', 'function f(iterable&A $a) {}', 'function f(int&A $a) {}', 'function f(A&B&a $a) {}',
			'trait T { function f(A&self $a) {} }', 'class A { function f(A&self $a) {} }', 'function f((A&B)|A $a) {}',
			'function f(A|(A&B) $a) {}', 'function f((A&B)|(B&A) $a) {}', 'function f((A&B)|(A&B&C) $a) {}', 'function f((A&B&C)|(A&B) $a) {}',
			'function f((A&B)|(C&D) $a) {}', 'function f(\\int $a) {}', 'function f(namespace\\INT $a) {}', 'function f(A\\int $a) {}',
			'function f(\\self $a) {}', 'function f(self $a) {}', 'class A { function f(parent $a) {} }', 'function f(): static|object {}',
			'$f = function (): static {};', 'function f(_ $a) {}', 'function f(boolean $a) {}', 'function f(double $a) {}',
			'namespace A; function f(resource $a) {}', 'use A\\B as integer; function f(integer $a) {}', 'function f(Integer $a, \\integer $b) {}',
			'use X\\Y as Z; function f(Z $a = 1) {}', 'use X\\Y as Z; function f(Z\\W $a = 1) {}', 'use X\\{Y, function g}; function f(Y $a = 1) {}',
			'namespace N; use X\\Y; function f(Y|y $a) {}', 'namespace N { use X\\Y; } namespace { function f(Y $a = 1) {} }',
			'namespace N; function f(namespace\\A $a = 1) {}', 'class A extends B { function m(parent $a = 1) {} }',
			'trait T { function f(self $a = 1) {} }', 'function f(int $a = true) {}', "function f(callable \$a = 'strlen') {}",
			'function f(iterable|null $a = 1) {}', 'function f(A|B|null|int|false $a = 1.5) {}', 'function f((A&B)|null $a = 1) {}',
			'function f(string $a = __LINE__) {}', 'function f(float $a = 1, int $b = FOO) {}', 'function f(int ...$a = 1) {}',
			'function f(...$a, $b) {}', 'function f(void $a) {}', 'function f(never $a) {}', 'function f(int $a = null, int $b = 1.5) {}',
			'namespace N; function f(A&B $a = null) {}', 'class A { function __construct(public int $a = null) {} }',
			'new class { function m(int $a = null) {} };', 'function f(int $a = 1, $b = 2, $c, ...$d) {}',
			'interface I { function f(int $a = null, $b); }', 'function f(mixed $a = null, $b) {}', 'class A { public int $a = true; }',
			'class A { public int $a = 1, $b = false; }', 'class A { public int|string $a = null; }', 'class A { public A&B $a = null; }',
			"class A { public float \$a = 1; public ?float \$b = null; public \$c = 'x'; }", 'new class { public callable $a; };',
			'class A { public ?callable $a; }', 'class A { const int B = true; }', 'class A { const self B = 1; }',
			'class A { const static B = 1; }', 'new class { const int B = 1.5; };', 'class A { const mixed B = 1; const ?callable C = 1; }',
			'class A { const float B = 1, C = self::B; }', 'function f(): void { return null; }', 'function f(): void { return \\NULL; }',
			'function f(): void { if (1) { return; } return 2; }', 'class A { function f() { return function (): void { return 1; }; } }',
			'class A { function f() { function g(): void { return 1; } } }', 'function f(): never { return; }', 'function f(): ?int { return; }',
			'function f(): int { return; }', 'class A { function __TOSTRING() { return; } }', 'function f(): int { $g = function () { return; }; return 1; }',
			'$f = fn(): void => 1;', '$f = fn(): void => null;', '$f = fn(): never => 1; return;', 'function &f(): void {}',
			'abstract class A { abstract function f(): void; }', 'function f(): void { return 1; yield; }', 'function f(): ?int { yield; }',
			'class A { function f(): static { yield; } }', 'class A { function __toString() { yield; } }', 'function f(): Generator { return; yield; }',
			'function f(): iterable { yield; } function g(): Traversable|int { yield; }', 'function f(): mixed { yield; } function g(): object { yield; }',
			'function &f() { yield from g(); }', 'function &f() { yield; }', "function f(\n\tint \$a = null,\n\t\$b = 1,\n\t\$c\n) {}",
			"function f(): void {\n\treturn\n\t\tnull;\n}",
		),
		'namespaces'         => array(
			'namespace A; namespace B {}', 'namespace A {} namespace B;', 'namespace A { namespace B {} }', 'echo 1; namespace A;',
			'declare(ticks=1); namespace A;', 'namespace A {} echo 1;', 'namespace A {} namespace {}', 'namespace A; echo 1; namespace B;',
			'namespace A {} function f() {}', 'namespace A {} use B;', 'namespace A {} __halt_compiler();', 'use A;', 'namespace N; use A;',
			'use A\\B, C\\B;', 'use A\\B; use C\\b;', 'use function A\\f, B\\F;', 'use const A\\C, B\\C;', 'use const A\\C, B\\c;',
			'use A\\self;', 'use A\\B as parent;', 'use function A\\self;', 'class B {} use A\\B;', 'class B {} use B;',
			'namespace N; class B {} use A\\B;', 'namespace N; class B {} use N\\B;', 'function f() {} use function A\\f;',
			'const C = 1; use const A\\C;', 'const C = 1; use const A\\c;', 'namespace N { use A\\B; } namespace M { use C\\B; }',
			'use A\\{B, C\\B};', 'use A\\{B, function B};', 'use function A\\{f, const F};', 'use A\\B; class B {}', 'use A\\B; class b {}',
			'use B; class B {}', 'namespace N; use N\\B; class B {}', 'use function A\\f; function f() {}', 'use function f; function f() {}',
			'use const A\\C; const C = 1;', 'use const C; const C = 1;', 'use const A\\c; const C = 1;', 'function __autoload() {}',
			'function __AutoLoad($a) {}', 'namespace N; function __autoload() {}', 'function assert() {}', 'namespace N; function ASSERT() {}',
			'function f() {} function f() {}', 'function f() {} function F() {}', 'namespace N; function f() {} function f() {}',
			'function f() { function g() {} function g() {} }', 'if (1) { function f() {} } function f() {}', "function f() {}\n\nfunction f() {\n}",
			'class A { function f() { class B {} } }', 'class A { function f() { function g() { class B {} } } }', 'class self {}',
			'interface iterable {}', 'trait mixed {}', 'enum never {}', 'class _ {}', 'interface _ {}', 'enum _ {}', 'trait _ {}',
			'$f = function (int $a = null) {};', 'function f() { $g = function (int $a = null) {}; }',
			'class A { function m() { $g = function (int $a = null) {}; } }', 'namespace N; function f() { $g = function (int $a = null) {}; }',
			"\$f = function () {\n\t\$g = function (int \$a = null) {};\n};", 'new class { function m() { $g = function (int $a = null) {}; } };',
			'class A { const B = __FUNCTION__; } function f() { return function () { static $a = __FUNCTION__; }; }',
			'declare(ticks=1); ; namespace A;', 'namespace A {} namespace {} namespace B {}', 'namespace A {} const C = 1;',
			'namespace N; class B {} use N\\b;', 'use A\\{function f, const F, function F as G, g};', 'use A\\{B, function B, C\\B};',
			'use function A\\f; function F() {}', 'namespace N; use function N\\f; function f() {}', 'namespace N; use const A\\C; const C = 1;',
			'namespace N; use const N\\C; const C = 1;', 'class A { function f() { new class {}; } }',
			'new class { function m() { $g = function () { $h = function (int $a = null) {}; }; } };',
			"namespace N; function f() {\n\$g = fn(int \$a = null) => 1; }",
			'function f() { return function () { static $a = __FUNCTION__ . __METHOD__; }; }',
		),
		'class references'   => array(
			'namespace\\self::f();', 'class A { function f() { namespace\\parent::f(); } }', 'class A { function f() { \\self::class; } }',
			'function f() { \\self::class; }', 'function f() { "self"::f(); }', 'function f() { "\\\\self"::f(); }', 'function f() { "\\\\A"::f(); }',
			'class A extends namespace\\self {}', 'class A extends \\self {}', 'try {} catch (\\self $e) {}', 'try {} catch (namespace\\self $e) {}',
			'try {} catch (A|Static $e) {}', "try {\n} catch (\nA\n|\nself \$e\n) {\n}", '(1+1)::class;', '"A"::class;', '(true)::class;',
			'(-1)::class;', '("a" . "b")::class;', 'function f() { "self"::class; }', 'class A { function f() { "parent"::class; } }',
			'(1)::f();', '(1)::$a;', 'new (1);', 'new ("self");', 'function f() { new ("se" . "lf"); }', '$a instanceof (2);',
			'1 instanceof self;', '(1.5)::C;', 'function f() { "parent"::C; }', 'function f() { return static::class; }',
			'function f() { new static; }', 'function f() { $a instanceof self; }', '$f = function () { self::f(); };',
			'class A { function f() { self::class::class; } }', 'class A extends B { function f() { parent::class::class; } }',
			'trait T { function f() { self::class::class; } }', 'interface I { use self; }', 'interface I { use T; }',
			'class A extends B implements self {}', '#[Attr(self::class)] class A extends self {}', 'enum E: int implements self {}',
			'interface I extends A, Parent {}', 'class A { use T, static; }', 'class A { use \\self; }', 'class A implements namespace\\static {}',
			'namespace N; use X\\Y; class A extends Y\\Z implements Y { use Y; function f() { Y::class::class; } }',
			'function f() { A::$a; B::C; new D; $a instanceof E; static::$b; }', 'function f() { $a::class; ($a . "b")::class; }',
		),
		'class members'      => array(
			'class A { function f() {} function f() {} }', 'class A { function F() {} function f() {} }', 'class A { public $a; public $a; }',
			'class A { public $a; public static $a; }', 'class A { public $a; protected $a; }', 'class A { abstract function f(); }',
			'class A { abstract function f() {} }', 'abstract class A { abstract private function f(); }', 'abstract class A { abstract function f() {} }',
			'class A { function f(); }', 'abstract class A { function f(); }', 'interface I { function f() {} }', 'interface I { private function f(); }',
			'interface I { protected function f(); }', 'interface I { final function f(); }', 'interface I { abstract function f(); }',
			'interface I { static function f(); }', 'interface I { public $a; }', 'interface I { public $a { get; } }', 'interface I { abstract public $a { get; } }',
			'interface I { final public $a { get; } }', 'interface I { protected $a { get; } }', 'interface I { private $a { get; } }',
			'interface I { public static $a { get; } }', 'interface I { use T; }', 'interface I { public $a { get {} } }',
			'enum E { public $a; }', 'enum E { public static $a; }', 'enum E { function __get($a) {} }', 'enum E { function __construct() {} }',
			'enum E { function __toString() {} }', 'enum E { function __invoke() {} }', 'enum E { function __call($a, $b) {} }', 'enum E: float {}',
			'enum E: int|string {}', 'enum E: ?int {}', 'enum E: A {}', 'enum E { case A = 1; }', 'enum E: int { case A; }', 'class A { case B; }',
			'interface I { case A; }', 'trait T { case A; }', 'enum E { case A; case A; }', 'enum E { case A; const A = 1; }',
			'enum E implements Serializable {}', 'enum E { case A; } enum E {}', 'class A { public abstract $a; }', 'abstract class A { public abstract $a; }',
			'class A { final private $a; }', 'class A { final public $a; }', 'class A { readonly $a; }', 'class A { readonly int $a = 1; }',
			'class A { static readonly int $a; }', 'readonly class A { public $a; }', 'readonly class A { public static int $a; }',
			'class A { public callable $a; }', 'class A { public ?callable $a; }', 'class A { public void $a; }', 'class A { public never $a; }',
			'class A { public static function __construct() {} }', 'class A { function __destruct($a) {} }', 'class A { function __get() {} }',
			'class A { function __get(&$a) {} }', 'class A { private function __get($a) {} }', 'class A { protected function __call($a, $b) {} }',
			'class A { function __toString(): int {} }', 'class A { function __get(int $a) {} }', 'class A { function __construct(): void {} }',
			'class A { static function __call($a, $b) {} }', 'class A { function __callStatic($a, $b) {} }', 'class A { function __clone(): int {} }',
			'class A { function __set($a, $b): int {} }', 'class A { function __set(string $a, $b): void {} }', 'class A { function __isset($a): int {} }',
			'class A { function __unset($a): int {} }', 'class A { function __sleep(): int {} }', 'class A { function __wakeup(): int {} }',
			'class A { function __serialize(): int {} }', 'class A { function __unserialize(int $a): void {} }', 'class A { function __set_state($a) {} }',
			'class A { static function __set_state(int $a) {} }', 'class A { function __invoke() {} }', 'class A { function __debugInfo(): int {} }',
			'class A { function __get(...$a) {} }', 'class A { function __get($a = 1) {} }', 'class A { function __call($a) {} }',
			'class A { function __clone($a) {} }', 'class A { static function __clone() {} }', 'class A { function __destruct(): void {} }',
			'class A { final private function f() {} }', 'class A { private final function __construct() {} }', 'class A { final private static function f() {} }',
			'class A { readonly function f() {} }', 'class A { public public $a; }', 'class A { public protected function f() {} }',
			'class A { abstract final function f(); }', 'class A { static static function f() {} }', 'class A { final final function f() {} }',
			'class A { function f() { class B {} } }', 'function f() { class A {} }', 'class A { function f() { $a = function () { class B {} }; } }',
			'class A { function f() { $a = new class {}; } }', 'class A { function __construct(public $a, public $a) {} }', 'class A { function f(public $a) {} }',
			'abstract class A { abstract function __construct(public $a); }', 'class A { function __construct(public ...$a) {} }',
			'interface I { function __construct(public $a); }', 'class A { function __construct(public callable $a) {} }', 'function f(public $a) {}',
			'class A { function __construct(public $a) {} public $a; }', 'class A { public $a; function __construct(public $a) {} }',
			'class A { function __construct(public readonly $a) {} }', 'class A { function __construct(readonly int $a) {} }',
			'class A { function __construct(static $a) {} }', 'trait T { function __construct(public $a) {} }', 'enum E { function __construct(public $a) {} }',
			'class A { function __construct(public $a = new B) {} }', 'new class(1) { function __construct(public $a) {} };',
			'class A { function __construct(private(set) int $a) {} }', 'class A { function __construct(public private(set) $a) {} }',
			'class A { function __construct(final public $a) {} }', 'class A { function __construct(public $a { get => 1; }) {} }',
			'class A { #[\Override] function f() {} }', 'class A { #[\Override] public $a; }', 'abstract class A { abstract function f() {} }',
			'class A { public function f() {} } class B extends A { private function f() {} }', 'class A { var $a; var $a; }',
			'trait T { abstract private function f(); }', 'trait T { public $a; public $a; }', 'new class { abstract function f(); };', 'class A { function A() {} }', 'class A { const class = 1; }', 'class A { function class() {} }',
		),
		'property hooks'     => array(
			'class A { public $a { get; set; get; } }', 'class A { public $a { get {} get {} } }', 'class A { public $a { foo {} } }',
			'class A { public $a { GET {} } }', 'class A { public static $a { get {} } }', 'class A { public readonly int $a { get {} } }',
			'class A { public $a { get($x) {} } }', 'class A { public $a { get() {} } }', 'class A { public $a { set($x, $y) {} } }',
			'class A { public $a { set() {} } }', 'class A { public $a { set(&$x) {} } }', 'class A { public $a { set(...$x) {} } }',
			'class A { public $a { set($x = 1) {} } }', 'class A { public int $a { set(string $x) {} } }', 'class A { public int $a { set(int|string $x) {} } }',
			'class A { public $a { set(int $x) {} } }', 'class A { public int $a { set(mixed $x) {} } }', 'class A { public $a { abstract get; } }',
			'class A { public $a { get; } }', 'abstract class A { public abstract $a { get {} } }', 'abstract class A { abstract public $a { get => 1; } }',
			'abstract class A { abstract public $a { abstract get; } }', 'abstract class A { public $a { abstract get; } }',
			'class A { public $a { final private get {} } }', 'class A { public $a { abstract final get; } }', 'abstract class A { public $a { abstract private get; } }',
			'class A { public $a { private get {} } }', 'class A { public $a { static get {} } }', 'class A { public $a = 1 { get => 1; } }',
			'class A { public $a = 1 { get => $this->a; } }', 'class A { public $a = 1 { set => 1; } }', 'class A { private(set) $a; }',
			'class A { protected public(set) int $a; }', 'class A { private public(set) int $a; }', 'class A { public private(set) int $a; }',
			'class A { public $a { get => parent::$b::get(); } }', 'class A { public $a { get => parent::$a::set(); } }',
			'class A { public $a { get => parent::$a::get(...); } }', 'class A { function f() { return parent::$a::get(); } }', 'parent::$a::get();',
			'class A { public $a { get => parent::$a::foo(); } }', 'class A { public $a { set { return parent::$a::get(); } } }',
			'class A { public $a { &get {} set {} } }', 'class A { public $a { &get => $this->a; set {} } }', 'class A { public $a { get { yield; } } }',
			'class A { public $a { set => 1; } }', 'class A { public $a { set($x) => 1; } }', 'class A { public $a { get { return; } } }',
			'class A { public int $a { get { return; } } }', 'class A { public $a { set { return 1; } } }', 'class A { public $a { #[\Override] get {} } }',
			'class A { public $a { get => 1; } public $a; }', 'class A { public private(set) $a { get => 1; } }', 'class A { public final $a { get => 1; } }',
			'interface I { public $a { set; } } ', 'interface I { public $a { get => 1; } }', 'class A { public $a { get; set; } }',
			'class A { var $a { get => 1; } }', 'class A { public $a { get => 1; set; } }', 'class A { public $a, $b { get => 1; } }',
		),
		'attributes'         => array(
			'#[Attribute] #[Attribute] class A {}', '#[Attribute, Attribute] class A {}', '#[Attribute] function f() {}', '#[A(...$a)] class B {}',
			'#[A(strlen(...))] class B {}', '#[A(1, b: 2, 3)] class B {}', '#[A(b: 1, b: 2)] class B {}', '#[\NoDiscard] function f(): void {}',
			'#[\NoDiscard] function f(): never {}', '#[\NoDiscard] function f(): int {}', '#[\Deprecated] class A {}', '#[\Deprecated] function f() {}',
			'#[\Override] function f() {}', '#[\Override] class A {}', '#[A($a)] class B {}', '#[A(f())] class B {}', '#[A(new B)] class C {}',
			'#[A(static fn() => 1)] class B {}', '#[A(fn() => 1)] class B {}', '#[A(static function () {})] class B {}', '#[\Attribute(1)] class A {}',
			'#[\AllowDynamicProperties] trait T {}', '#[\AllowDynamicProperties] interface I {}', '#[\AllowDynamicProperties] enum E {}',
			'#[\AllowDynamicProperties] readonly class A {}', '#[\SensitiveParameter] function f() {}', 'function f(#[\SensitiveParameter] $a) {}',
			'#[\ReturnTypeWillChange] class A {}', '#[\Attribute] abstract class A {}', '#[\Attribute] enum E {}', '#[\Attribute] interface I {}',
			'#[\Attribute] trait T {}', '#[\Deprecated] const A = 1;', 'class A { #[\Deprecated] const B = 1; }', 'class A { #[\Deprecated] public $a; }',
			'#[\Deprecated(1)] function f() {}', '#[\Deprecated("a", "b", "c")] function f() {}', '#[\NoDiscard("a")] function f(): int {}',
			'#[A] #[A] class B {}', '#[A(...)] class B {}', '#[A(...$a, b: 1)] class B {}', '#[\Override] public $a;', 'class A { #[\Override] const B = 1; }',
			'#[\Attribute(\Attribute::TARGET_FUNCTION)] class A {} #[A] class B {}', '#[\Attribute(\Attribute::TARGET_ALL)] #[\Attribute] class A {}',
			'enum E { #[\Deprecated] case A; }', '#[\Deprecated] fn() => 1;', '#[\NoDiscard] fn(): void => 1;', '#[\NoDiscard] function &f(): void {}',
		),
		'traits'             => array(
			'class A { use T { f as static; } }', 'class A { use T { f as abstract; } }', 'class A { use T { f as final; } }',
			'class A { use T { f as readonly g; } }', 'class A { use T { f as protected; } }', 'class A { use T { T::f insteadof T; } }',
			'class A { use T, U { f insteadof U; } }', 'class A { use T { f as g; f as g; } }', 'trait T { use T; }', 'trait T { function __construct() {} }',
			'trait T { static function f() {} } T::f();', 'trait T { const A = 1; }', 'trait T { abstract static function f(); }',
		),
		'lines'              => array(
			"\n\n\$a[];", "f(\n1,\n\$a[]\n);", "class A {\n  function f() {}\n\n  function f() {}\n}", "class A {\n  abstract\n  function f();\n}",
			"function f(\n  int \$a = null,\n  \$b\n) {}", "\$this\n=\n1;", "while (1) {\n\n  break 2;\n}", "goto\na;", "const A = 1,\n  A = 2;",
			"function f(): void {\n  return\n    1;\n}", "\"\n\${a}\n\${b}\";", "use A\\B;\n\nclass B {}", "#[A]\nconst B = 1,\n C = 2;",
			"class A {\n  function __get(\n    \$a,\n    \$b\n  ) {}\n}", "switch (1) {\n  case 1;\n  case 2;\n}", "match (1) {\n  default => 1,\n  default => 2,\n};",
			"f(\n  a: 1,\n  2\n);", "abstract class A {\n  abstract function f() {\n  }\n}", "class A {\n  readonly\n  \$a;\n}",
			"try {\n}\n\n", "declare(\n  strict_types=2\n);", "namespace A;\necho 1;\nnamespace B {\n}", "class A {\n  public \$a {\n    get {}\n    get {}\n  }\n}",
		),
	);
}

/** Every case as [name, source]; the same list in every run. */
function compiler_cases(): array {
	$cases = array();
	$seen  = array();
	foreach ( compiler_topic_cases() as $topic => $codes ) {
		foreach ( $codes as $index => $code ) {
			$source = '<?php ' . $code;
			if ( ! isset( $seen[ $source ] ) ) {
				$seen[ $source ] = true;
				$cases[]         = array( $topic . ' ' . ( $index + 1 ), $source );
			}
		}
	}
	foreach ( parser_cases() as list( $name, $source ) ) {
		if ( ! isset( $seen[ $source ] ) && null === parser_parse( $source )[0] ) {
			$seen[ $source ] = true;
			$cases[]         = array( 'parser ' . $name, $source );
		}
	}
	return $cases;
}

/**
 * Compiles every source in lint mode and returns, per source in the same order, its diagnostics as
 * [severity, message, line] triples.
 */
function compiler_lint( array $sources ): array {
	$dir = sys_get_temp_dir() . '/compiler-oracle-' . getmypid();
	if ( ! is_dir( $dir ) && ! mkdir( $dir ) ) {
		throw new RuntimeException( 'cannot create ' . $dir );
	}
	$results = array();
	foreach ( array_chunk( $sources, COMPILER_BATCH ) as $chunk ) {
		$files = array();
		foreach ( $chunk as $source ) {
			$file = sprintf( 'case-%05d.php', count( $results ) + count( $files ) );
			file_put_contents( $dir . '/' . $file, $source );
			$files[] = $file;
		}
		$found = compiler_lint_files( $dir, $files );
		foreach ( $files as $file ) {
			$results[] = $found[ $file ] ?? array();
			unlink( $dir . '/' . $file );
		}
	}
	rmdir( $dir );
	return $results;
}

/** Lints $files (names relative to $dir) in one process; returns file => diagnostics. */
function compiler_lint_files( string $dir, array $files ): array {
	$command = array_merge(
		array( PHP_BINARY, '-d', 'display_errors=stderr', '-d', 'log_errors=0', '-d', 'html_errors=0', '-d', 'error_reporting=-1', '-d', 'fatal_error_backtraces=0', '-l' ),
		$files
	);
	$process = proc_open( $command, array( 1 => array( 'file', '/dev/null', 'w' ), 2 => array( 'pipe', 'w' ) ), $pipes, $dir );
	if ( false === $process ) {
		throw new RuntimeException( 'cannot start the linter' );
	}
	$output = stream_get_contents( $pipes[2] );
	fclose( $pipes[2] );
	proc_close( $process );
	$pattern = '/^(Fatal error|Parse error|Warning|Deprecated|Notice): (.*) in (case-\d{5}\.php) on line (\d+)$/m';
	preg_match_all( $pattern, $output, $matches, PREG_SET_ORDER );
	if ( '' !== trim( preg_replace( $pattern, '', $output ) ) ) {
		throw new RuntimeException( 'unexpected linter output: ' . $output );
	}
	$found = array();
	foreach ( $matches as list( , $severity, $message, $file, $line ) ) {
		$found[ $file ][] = array( $severity, oracle_bytes( str_replace( $file, COMPILER_FILE, $message ) ), (int) $line );
	}
	return $found;
}

$cases       = compiler_cases();
$diagnostics = compiler_lint(
	array_map(
		function ( array $case ): string {
			return $case[1];
		},
		$cases
	)
);
$records     = array();
foreach ( $cases as $index => list( $name, $source ) ) {
	$records[] = array(
		'name'        => $name,
		'source'      => oracle_bytes( $source ),
		'diagnostics' => $diagnostics[ $index ],
	);
}

echo json_encode(
	array(
		'schema'          => COMPILER_SCHEMA,
		'php_version'     => PHP_VERSION,
		'short_open_tag'  => (bool) ini_get( 'short_open_tag' ),
		'zend_assertions' => (int) ini_get( 'zend.assertions' ),
		'cases'           => $records,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
), "\n";
