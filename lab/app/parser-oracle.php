<?php
/*
 * Ferropress lab parser oracle: prints how the reference PHP parses source text, as JSON.
 *
 * For every case the oracle records the error parsing the source raises (class, message and line),
 * the warnings raised while parsing (severity and message), and the engine's own rendering of the
 * syntax tree. That
 * rendering is the message of a failed assertion: the source is placed inside the body of a closure
 * passed to assert(), and the engine prints the closure's syntax tree into the AssertionError
 * message. Code that cannot appear inside a function body (namespaces, imports, top-level constants)
 * has no rendering. Compiling the closure can end the process with a fatal error, so renderings are
 * produced by a worker process that is restarted whenever that happens.
 *
 * Cases are hand-written, the lexer oracle's cases, random expressions built from a fixed seed, and
 * hand-written cases with one token removed, repeated, swapped or inserted.
 *
 * Run by `cargo xtask lab parser`; never part of Ferropress.
 */

declare(strict_types=1);

require __DIR__ . '/lexer-oracle.php';

const PARSER_SCHEMA         = 1;
const PARSER_SEED           = 20261024;
const PARSER_RANDOM_EXPRS   = 3000;
const PARSER_MUTATIONS      = 2500;
const PARSER_WORKER_FLAG    = '--render-worker';

/** Hand-written cases: name => code following "<?php ". */
function parser_code_cases(): array {
	return array(
		// Expressions: literals and names.
		'int literals'          => '1; 0x1F; 0b11; 0o17; 017; 1_000; 9223372036854775807; 9223372036854775808; 0x8000000000000000;',
		'float literals'        => '1.5; .5; 1.; 1e3; 1E-7; 0.1; 1e100; 123456789012345678; 1.0e15; 1e15; 1e14; 0.0001; 0.00001; 1.5e-5; 7E+20;',
		'float precision'       => '0.1 + 0.2; 3.14159265358979; 2.718281828459045; 1.23456789012345678; 100000000000000000000.0; 1e-10;',
		'strings'               => "'a'; 'it\\'s'; 'back\\\\'; \"x\\ny\"; \"tab\\t\"; \"q\\\"q\"; \"\\\$a\"; \"\\x41\\101\\u{1F600}\"; 'a\nb'; \"\\0\\1\\x7f\";",
		'string escapes print'  => "\"\\e\\f\\v\\r\"; \"\\x01\\x1f\"; 'single \\n stays'; \"dollar \\$ and \\\\\"; \"\\x80\\xff\";",
		'interpolation'         => '"a $b c"; "$a[0] $a[x] $a[$i] $a->b $a?->b"; "${a} ${a[1]} {$a} {$a->b()} {$a[1][2]}"; "$a$b"; "$a[-1]"; "$a[0x1]";',
		'interpolation braces'  => '"$a[b]c"; "{$a}[x]"; "$a" . "[x]"; "{$a}b"; "$a b"; "{$a}_"; "$a->b->c"; "${a . b}";',
		'heredoc'               => "<<<EOT\nline \$a\n  two {\$b}\nEOT;\n<<<'N'\nraw \$x\nN;\n<<<EOT\nEOT;\n<<<\"Q\"\nq\nQ;",
		'heredoc indented'      => "<<<EOT\n    a\n      b \$c\n    d\n    EOT;\n<<<'N'\n  x\n   y\n  N;",
		'heredoc whitespace lines' => "<<<EOT\n    a\n\n  \n    b\n    EOT;",
		'heredoc escapes'       => "<<<EOT\n\\t\\x41\\\"\\'\\\\\\\$ \\u{48}\nEOT;",
		'heredoc only var'      => "<<<EOT\n\$a\nEOT;\n<<<EOT\n  {\$a} x\n  EOT;",
		'backticks'             => '`ls`; `ls $dir`; `a\\`b`;',
		'constants'             => 'A; \\A; A\\B; \\A\\B; namespace\\A; true; FALSE; null; NULL; __LINE__; __file__; __DIR__; __CLASS__; __TRAIT__; __FUNCTION__; __METHOD__; __PROPERTY__; __NAMESPACE__;',
		'arrays'                => '[]; [1, 2,]; [1 => 2, "k" => $v]; array(); array(1, 2); [&$a, ...$b, ...[1]]; [[1], [2, [3]]]; ["a" => &$b];',
		'array skipped'         => '[, $a] = $b; [$a, , $c] = $d; list(, $x) = $y; [[, $a]] = $b;',
		'destructuring'         => '[$a, $b] = $c; list($a, list($b)) = $c; ["k" => $a, "j" => [$b]] = $c; [&$a, [$b]] = $c; foreach ($x as [$a, $b]) {}',
		'variables'             => '$a; $$a; $$$a; ${"a"}; ${$a . "b"}; $a::$b; $this;',
		'dim and property'      => '$a[0]; $a[]; $a[0][1]; $a->b; $a->b->c; $a?->b; $a->$b; $a->{$b . "c"}; $a->{"x"}; $a->b[0]; $a::$b[0]; $a{0} ?? 1;',
		'dim forms'             => '$a[0]; "abc"[1]; [1, 2][0]; A[0]; A::B[0]; (expr)[0]; f()[0]; $a->b()[0];',
		'calls'                 => 'f(); f(1, 2); \\f(); A\\f(); namespace\\f(); $f(); $a->m(); $a?->m(1); A::m(); A::$m(); $a::m(); static::m(); parent::m(); self::m(); f()(); $a->b()->c();',
		'call arguments'        => 'f(...$a); f(a: 1, b: 2); f(1, ...$b); f(1,); f(x: ...$y); f(&$a);',
		'first class callable'  => 'f(...); $a->m(...); A::m(...); $f(...); strlen(...); $a->$m(...); "f"(...);',
		'new callable'          => 'new A(...);',
		'new'                   => 'new A; new A(); new A(1); new \\A\\B; new $a; new $a->b; new $a[0]; new $a::$b; new static; new self(); new (f()); new class {}; new class(1) extends A implements B, C { public $x; };',
		'new dereference'       => 'new A()->b; new A()->b(); new A()[0]; new A()::C; (new A)->b;',
		'class constants'       => 'A::B; A::class; $a::class; static::B; A::{"B"}; A::{$b}; \\A::B; A::CLASS; a::Class;',
		'static properties'     => 'A::$b; A::$b[0]; A::$$b; A::${"b"}; $a::$b; static::$b;',
		'unary'                 => '!$a; -$a; +$a; ~$a; @$a; -1; - -1; -(-1); !!$a; @f(); +-+$a;',
		'casts'                 => '(int) $a; (integer) $a; (bool) $a; (boolean) $a; (float) $a; (double) $a; (string) $a; (binary) $a; (array) $a; (object) $a; (int)(float) $a; -(int) $a;',
		'unset cast'            => '(unset) $a;',
		'increment'             => '$a++; $a--; ++$a; --$a; $a->b++; ++$a[0]; A::$b--;',
		'binary'                => '$a + $b; $a - $b; $a * $b; $a / $b; $a % $b; $a ** $b; $a . $b; $a << $b; $a >> $b; $a & $b; $a | $b; $a ^ $b;',
		'comparison'            => '$a == $b; $a != $b; $a <> $b; $a === $b; $a !== $b; $a < $b; $a <= $b; $a > $b; $a >= $b; $a <=> $b;',
		'logical'               => '$a && $b; $a || $b; $a and $b; $a or $b; $a xor $b; $a ?? $b; $a |> $b;',
		'precedence mix'        => '$a + $b * $c; ($a + $b) * $c; $a - $b - $c; $a - ($b - $c); $a ** $b ** $c; ($a ** $b) ** $c; -$a ** 2; (-$a) ** 2; !$a instanceof B; $a . $b + $c; $a << $b . $c;',
		'precedence logical'    => '$a && $b || $c; $a || $b && $c; $a and $b or $c; $a = $b and $c; $a = $b && $c; $a ?? $b ?? $c; ($a ?? $b) ?? $c; $a | $b & $c ^ $d;',
		'assign right binding'  => '$a + $b = 3; !$a = 1; @$x = f(); $a && $b = $c; $a ?? $b = 1; $a . $b .= "x"; -$a = 1;',
		'assignments'           => '$a = 1; $a = $b = $c; $a += 1; $a -= 1; $a *= 1; $a /= 1; $a .= 1; $a %= 1; $a **= 1; $a &= 1; $a |= 1; $a ^= 1; $a <<= 1; $a >>= 1; $a ??= 1;',
		'assign by reference'   => '$a = &$b; $a = &$b->c; $a = &f(); $a = &new A; $a[] = &$b; $a->b = &$c;',
		'ternary'               => '$a ? $b : $c; $a ?: $b; $a ? $b : ($c ? $d : $e); ($a ? $b : $c) ? $d : $e; $a ?: $b ?: $c; $a ? $b ?: $c : $d; $a ?? $b ? $c : $d;',
		'instanceof'            => '$a instanceof A; $a instanceof $b; $a instanceof \\A\\B; $a instanceof static; !$a instanceof A; $a instanceof (A::class);',
		'isset empty'           => 'isset($a); isset($a, $b, $c); isset($a[0]->b); empty($a); empty(f()); isset($a,);',
		'include family'        => 'include "a.php"; include_once $a; require ("b"); require_once __DIR__ . "/c.php"; eval("1;"); include "x" or die;',
		'exit'                  => 'exit; exit(); exit(1); die; die("x"); exit(...); $f = exit(...);',
		'print and echo'        => 'print 1; print $a and $b; echo 1, 2, $a; echo(1); print(1) . 2;',
		'clone'                 => 'clone $a; clone($a); clone $a->b; clone(); clone(...); clone($a, ["x" => 1]); clone($a,); clone(object: $a); clone(...$args);',
		'yield'                 => 'function g() { yield; yield 1; yield $k => $v; $x = yield; $x = yield 1; $x = yield $k => $v; yield from $g; f(yield 1); [yield 1 => 2]; yield yield 1; return yield; }',
		'yield precedence'      => 'function g() { yield $a + 1; yield $a and $b; $x = yield $a ? 1 : 2; yield $a ?? $b; yield from $a or $b; yield 1 => 2 + 3; }',
		'throw expr'            => 'throw $e; $a ?? throw new E; $f = fn() => throw $e; $a = $b ?: throw new E();',
		'arrow functions'       => 'fn() => 1; fn($a) => $a + 1; fn&($a) => $a; static fn() => 1; fn(int $a = 1, &...$b): int => $a; fn() => fn() => 1; (fn() => 1)(); fn() => $a or $b;',
		'closures'              => 'function () {}; function ($a, $b) use ($c, &$d) { return $c; }; static function &() {}; function (): ?int { return 1; }; function () use ($a,) {}; (function () {})();',
		'match'                 => 'match ($a) { 1 => "a", 2, 3 => "b", default => "c" }; match ($a) {}; match (true) { $a > 1, => 1, }; match($a) { default, => 1 };',
		'pipe'                  => '$a |> f(...); $a |> f(...) |> g(...); $a |> (fn($x) => $x + 1); $a + 1 |> f(...); $a |> f(...) == $b; $a . $b |> f(...); $a |> f(...) . $b;',
		'pipe arrow function'   => '$a |> fn($x) => $x + 1;',
		'list in foreach'       => 'foreach ($a as list($b, $c)) {} foreach ($a as $k => list(, $v)) {} foreach ($a as &$v) {} foreach ($a as $k => &$v) {}',
		'nested parens'         => '((($a))); ($a)(); ($a)[0]; ($a)->b; ($a)::$b; (A)::B; ("f")(); (function () {})(); (fn() => 1)();',
		'string calls'          => '"f"(); "A::m"(); [$a, "m"](); "abc"->x ?? 1;',
		'void cast'             => '(void) f(); (void) $a->m();',
		'magic and keywords as names' => '$a->class; $a->list; A::new(); A::function; $a->fn(); A::NULL; f(array: 1, class: 2); $a->namespace;',
		'semi reserved'         => 'class C { function list() {} function new() {} const FOREACH = 1; public $class; function print() {} }',
		// Statements.
		'echo statements'       => "echo 'a'; echo \$a, 'b';",
		'inline html'           => 'echo 1; ?>text<?php echo 2; ?>more',
		'inline html start'     => '?>only html',
		'empty statements'      => ';;; { ; } if ($a) ; while (0) ;',
		'blocks'                => '{ $a; { $b; } } {}',
		'if forms'              => 'if ($a) $b; if ($a) { $b; } elseif ($c) { $d; } else { $e; } if ($a) {} else if ($b) {} else {} if ($a): $b; elseif ($c): $d; else: $e; endif;',
		'if else if nested'     => 'if ($a) {} else if ($b) {} else if ($c) {} if ($a) {} else { if ($b) {} } if ($a) {} else { if ($b) {} $c; }',
		'while'                 => 'while ($a) { $b; } while ($a): $b; endwhile; while ($a) $b;',
		'do while'              => 'do { $a; } while ($b); do $a; while ($b);',
		'for'                   => 'for ($i = 0; $i < 10; $i++) {} for (;;) {} for ($a, $b; $c, $d; $e, $f) {} for (; $a;): endfor;',
		'foreach'               => 'foreach ($a as $v) {} foreach ($a as $k => $v) {} foreach ($a as $v): endforeach; foreach (f() as $a->b) {} foreach ($a as $k => [$x, $y]) {}',
		'switch'                => 'switch ($a) { case 1: $b; break; case 2; case 3: default: $c; } switch ($a): case 1: endswitch; switch ($a) {} switch ($a) {; case 1: }',
		'break continue'        => 'while (1) { while (1) { break; break 2; continue; continue 2; break(1); } }',
		'return'                => 'return; return 1; return $a ?? $b;',
		'global static'         => 'function f() { global $a, $b; static $c, $d = 1, $e = [1]; static $x = 1 + 2; }',
		'unset'                 => 'unset($a); unset($a, $b[0], $c->d); unset($a,);',
		'goto and labels'       => 'a: goto a; b: $x; goto b;',
		'try'                   => 'try { $a; } catch (E $e) { $b; } try {} catch (A|B $e) {} finally {} try {} finally {} try {} catch (E) {} try {} catch (\\A\\B $e) {}',
		'declare'               => 'declare(ticks=1); declare(ticks=1) { $a; } declare(ticks=1): enddeclare; declare(ticks=1, ticks=2);',
		'declare strict types'  => 'declare(strict_types=1); $a;',
		'declare encoding'      => 'declare(encoding="UTF-8"); $a;',
		'declare encoding nonliteral' => 'declare(encoding=A);',
		'halt compiler'         => '$a; __halt_compiler(); raw ( [ {',
		'halt compiler close'   => '$a; __halt_compiler() ?> raw',
		'halt compiler nested'  => 'if (1) { __halt_compiler(); }',
		'namespace braced'      => 'namespace A { $a; } namespace { $b; }',
		'namespace plain'       => 'namespace A\\B; $a;',
		'use imports'           => 'use A; use A\\B as C; use function f; use const C; use A\\{B, C as D, function e, const F}; use function A\\{b, c};',
		'top const'             => 'const A = 1, B = 2;',
		// Declarations.
		'functions'             => 'function f() {} function &g($a, &$b, ...$c) {} function h(int $a = 1, ?string $b = null, A|B $c = new A): ?A {} function i(): iterable {} function j(): never {} function k(): ?array {} function l(): callable|null {}',
		'function types'        => 'function f(int|string|null $a, A&B $b, (A&B)|null $c, array $d, callable $e, iterable $f, mixed $g, B $h, ?\\A\\B $i, INT $j, (A&B)|(C&D) $k, null|false $l): void {}',
		'function attributes'   => '#[A] function f(#[B(1)] $a, #[C, D(x: 2)] int $b) {} #[A(1, 2)] #[B] function g() {}',
		'function return ref'   => 'function &f(): array { static $a = []; return $a; }',
		'classes'               => 'class A {} abstract class B extends A implements I, J {} final class C {} readonly class D {} final readonly class E {} #[Attr] class F {}',
		'class members'         => 'abstract class A { const X = 1; public const Y = 2, Z = 3; final protected const int W = 4; public $a; protected static ?int $b = 1; private readonly string $c; var $d, $e = 2; public function f() {} abstract protected function g(); final public static function h(): void {} }',
		'class modifiers order' => 'abstract class A { static public function f() {} public static $a; final public function g() {} public final const B = 1; readonly public int $c; public abstract function h(); }',
		'constructor promotion' => 'class A { public function __construct(public int $a, private readonly ?string $b = null, protected $c = 1, public private(set) int $d = 0, final public $e = 1) {} }',
		'asymmetric visibility' => 'class A { public private(set) int $a; protected(set) string $b; public protected(set) ?array $c = null; private(set) int $d; }',
		'property hooks'        => 'abstract class A { public int $a { get => 1; set => $value; } public int $b { get { return 1; } set(int $v) { $this->b = $v; } } public $c { &get => $this->c; } public $f { final set { } } abstract public $d { get; } public $e { set => strtoupper($value); } }',
		'property hooks attrs'  => 'class A { public $a { #[A] get => 1; final set {} } public function __construct(public $b { get => 2; }) {} }',
		'interfaces'            => 'interface I {} interface J extends I, K { const A = 1; public function f(); public static function g(int $a): void; }',
		'interface properties'  => 'interface I { public int $a { get; } public $b { set; } }',
		'traits'                => 'trait T { public $a; abstract public function f(); public static function g() {} } class A { use T; use T, U; use T { f as g; f as protected h; T::f insteadof U; U::f as public; g as private; } use T {} }',
		'enums'                 => 'enum E { case A; case B; } enum F: int { case A = 1; case B = 2 * 3; const C = self::A; public function f() {} } enum G: string implements I { case A = "a"; use T; }',
		'enum case expressions' => 'enum E: int { case A = 1 << 2; case B = PHP_INT_SIZE; }',
		'anonymous classes'     => 'new class {}; new class($a) {}; new class extends A {}; new class implements I {}; new #[A] class {}; new readonly class {}; new class(...$a) {};',
		'nested functions'      => 'function f() { function g() {} class A {} }',
		'class constant types'  => 'class A { const int A = 1; const ?string B = null; const A|B|null C = null; const ?self D = null; const array E = [1]; }',
		'static closures in class' => 'class A { function f() { return static fn() => self::class; } }',
		'method calls on new'   => 'new A()->b()->c; new A()->b::$c; new A()?->b;',
		'attribute groups'      => '#[A, B(1), \\C\\D(x: [1, 2])] class E { #[F] const G = 1; #[H] public $i; #[J] public function k(#[L] $m) {} }',
		'attribute on closure'  => '#[A] function () {}; #[A] fn() => 1; #[A] static function () {}; #[A] static fn() => 1;',
		'by reference methods'  => 'abstract class A { public function &f() {} public static function &g() {} abstract protected function &h(); private function i() {} }',
		'final constants'       => 'class A { final public const B = 1; }',
		'readonly function'     => 'readonly(); readonly function f() {}',
		'static keyword uses'   => 'static::a(); static fn() => 1; static function () {}; function f() { static $a; } new static; $a instanceof static;',
		'labels vs constants'   => 'A: B; A ? B : C; A::B;',
		'line numbers'          => "\$a\n+\n\$b;\nf(\n1,\n2\n);",
		'comments everywhere'   => '/* a */ $a /* b */ = /* c */ 1 /* d */; // e' . "\n" . '# f' . "\n" . '/** g */ function f() {}',
		'long program'          => 'function f($n) { if ($n <= 1) { return 1; } return $n * f($n - 1); } $r = []; for ($i = 0; $i < 10; $i++) { $r[] = f($i); } echo implode(",", $r), "\n";',
		// Errors the parser raises.
		'error unexpected end'  => '$a = ',
		'error unexpected token' => '$a = ;',
		'error missing semicolon' => "\$a = 1\n\$b = 2;",
		'error unexpected identifier' => '$a b;',
		'error unexpected variable' => 'f($a $b);',
		'error unexpected integer' => '$a 1;',
		'error unexpected float' => '$a 1.5;',
		'error unexpected string' => '$a "x";',
		'error unexpected long string' => '$a "this string is rather long and goes on and on";',
		'error unexpected multiline string' => "\$a 'first\nsecond';",
		'error unexpected interpolated' => '$a "x $y";',
		'error unexpected heredoc' => "\$a <<<A\nx\nA;",
		'error unexpected names' => 'f(\\A\\B C);',
		'error unexpected relative' => 'f(1 namespace\\A);',
		'error unexpected qualified' => 'f(1 A\\B);',
		'error unexpected cast' => '$a (int) $b;',
		'error unexpected die'  => '$a die;',
		'error unexpected close tag' => '$a ?>',
		'error unexpected inline html' => 'if ($a) ?>html<?php',
		'error unexpected quote' => '$a "',
		'error unexpected backslash' => '$a \\;',
		'error unexpected ampersand' => '$a & & $b;',
		'error unexpected bad char' => "\$a \x01;",
		'error assign to constant' => 'A = 1;',
		'error assign to parens' => '($a) = 1;',
		'error assign to literal' => '1 = $a;',
		'error assign to new'   => 'new A = 1;',
		'error ref assign twice' => '$a = &$b = $c;',
		'error list compound'   => '[$a] += 1;',
		'error list without assign' => 'list($a);',
		'error nonassoc equality' => '$a == $b == $c;',
		'error nonassoc comparison' => '$a < $b < $c;',
		'error nonassoc mixed'  => '$a < $b > $c;',
		'equality of comparisons' => '$a < $b == $c > $d;',
		'error closure call'    => 'function () {}();',
		'error new member'      => 'new A->b;',
		'error new static call' => 'new A::b();',
		'error clone empty args' => 'clone($a, , $b);',
		'error missing paren'   => 'if ($a { }',
		'error extra paren'     => 'f(1));',
		'error unclosed brace eof' => 'function f() {',
		'error else without if' => 'else {}',
		'error elseif after else' => 'if (1) {} else {} elseif (2) {}',
		'error mixed alt syntax' => 'if (1): else { }',
		'error case outside'    => 'case 1:',
		'error default token'   => 'default;',
		'error break expr'      => 'break $a;',
		'error class in expr'   => '$a = class {};',
		'error function name'   => 'function 1() {}',
		'error double modifiers' => 'class A { public public $a; }',
		'error multiple access' => 'class A { public private $a; }',
		'error multiple static' => 'class A { static static function f() {} }',
		'error multiple abstract class' => 'abstract abstract class A {}',
		'error multiple final class' => 'final final class A {}',
		'error multiple readonly class' => 'readonly readonly class A {}',
		'error abstract final class' => 'abstract final class A {}',
		'error final abstract class' => 'final abstract class A {}',
		'error abstract anonymous' => 'new abstract class {};',
		'error final anonymous' => 'new final class {};',
		'error abstract final method' => 'class A { abstract final function f(); }',
		'error abstract final property' => 'class A { abstract final $a { get; } }',
		'error modifier on constant' => 'class A { static const B = 1; }',
		'error abstract constant' => 'class A { abstract const B = 1; }',
		'error readonly method' => 'class A { readonly function f() {} }',
		'error var modifier'    => 'class A { var public $a; }',
		'error set visibility method' => 'class A { private(set) function f() {} }',
		'error modifier on param' => 'function f(static $a) {}',
		'error abstract param'  => 'class A { function __construct(abstract $a) {} }',
		'error multiple readonly param' => 'class A { function __construct(readonly readonly $a) {} }',
		'error static hook'     => 'class A { public $a { static get; } }',
		'error public hook'     => 'class A { public $a { public get; } }',
		'error multiple final hook' => 'class A { public $a { final final get; } }',
		'error multiple set visibility' => 'class A { public(set) private(set) $a; }',
		'error halt nested'     => 'function f() { __halt_compiler(); }',
		'error namespace nested' => 'function f() { namespace A; }',
		'error use nested'      => 'if (1) { use A; }',
		'error const nested'    => 'if (1) { const A = 1; }',
		'error real cast'       => '(real) $a;',
		'error invalid octal'   => '$a = 09;',
		'error unterminated comment' => "\$a;\n/* x",
		'error unmatched paren' => '$a);',
		'error unclosed paren'  => "f(\$a\n\n",
		'error mismatched brackets' => "f(\$a];",
		'error bad codepoint'   => '"\\u{zz}";',
		'error heredoc indentation' => "<<<A\n  x\n y\n  A;",
		'error heredoc mixed indentation' => "<<<A\n\t x\n\t A;",
		'error heredoc nowdoc indentation' => "<<<'A'\n x\n  A;",
		'error interpolation var indentation' => "<<<A\n\$x\n  A;",
		'error octal escape warning' => '"\\400";',
		'error unexpected end in class' => 'class A {',
		'error unexpected token in class' => 'class A { $a; }',
		'error unexpected end in string' => '"abc',
		'error yield outside parens' => 'f(yield);',
		'error match no arms comma' => 'match ($a) { , };',
		'error match default twice' => 'match ($a) { default => 1, default => 2 };',
		'error static in expr'  => '$a = static;',
		'error fn without arrow' => 'fn($a) $a;',
		'error enum without name' => 'enum { }',
		'error enum backing'    => 'enum E: { }',
		'error interface body'  => 'interface I { public $a; }',
		'error trait adaptation' => 'class A { use T { f; } }',
		'error goto missing label' => 'goto;',
		'error unset expr'      => 'unset(1);',
		'error isset expr'      => 'isset(1);',
		'error empty no args'   => 'empty();',
		'error isset no args'   => 'isset();',
		'error array spread key' => '[...$a => 1];',
		'error by ref spread'   => '[&...$a];',
		'error named before positional' => 'f(a: 1, 2);',
		'error spread then positional' => 'f(...$a, 1);',
		'error double arrow outside' => '$a => $b;',
		'error dangling operator' => '$a +;',
		'error dangling unary'  => '!;',
		'error empty parens'    => '();',
		'error double semicolon in for' => 'for (;;;) {}',
		'error foreach without as' => 'foreach ($a) {}',
		'error foreach key ref' => 'foreach ($a as &$k => $v) {}',
		'error switch case without colon' => 'switch ($a) { case 1 }',
		'error try without catch' => 'try {}',
		'error catch without type' => 'try {} catch ($e) {}',
		'error declare without parens' => 'declare ticks=1;',
		'error return type'     => 'function f(): {}',
		'error param default'   => 'function f($a = ) {}',
		'error param without name' => 'function f(int) {}',
		'error nullable union'  => 'function f(?int|string $a) {}',
		'error class extends twice' => 'class A extends B extends C {}',
		'error interface implements' => 'interface I implements J {}',
		'error const without value' => 'class A { const B; }',
		'error property without var' => 'class A { public int; }',
		'error method without parens' => 'class A { function f {} }',
		'error attribute target' => '#[A] $a;',
		'error attribute unclosed' => '#[A function f() {}',
		'error new without class' => 'new;',
		'error instanceof literal' => '$a instanceof 1;',
		'error pipe missing rhs' => '$a |> ;',
		'error clone named and positional' => 'clone($a, b: 1, 2);',
		'error clone no parens spread' => 'clone ...$a;',
		'error exit named'      => 'exit(status: 1, 2);',
		'error ternary missing else' => '$a ? $b;',
		'error coalesce assign to call' => 'f() ??= 1;',
		'error increment literal' => '1++;',
		'error increment constant' => '++A;',
		'error unexpected end of heredoc' => "<<<A\nx",
		'error unexpected nested function name' => 'function f() { function () {} }',
		'error unexpected keyword' => '$a = function;',
		'error unexpected token class' => 'class;',
		'error unexpected endif' => 'endif;',
		'error lone open brace'  => '{',
		'error lone close brace' => '}',
		'error elseif alt mixed' => 'if (1): elseif (2) {} endif;',
		'error label on keyword' => 'if: $a;',
		'error global expr'     => 'global $a->b;',
		'error static var expr' => 'static $a->b;',
		'error abstract function' => 'abstract function f();',
		'error readonly property no type' => 'class A { readonly function; }',
		'error property hook list empty' => 'class A { public $a {} }',
		'error property hook name' => 'class A { public $a { 1 => 2; } }',
		'error line after newlines' => "\$a\n\n\n\$b;",
		'error token on later line' => "f(\n\n1\n2);",
		'error multiline token' => "f(1 \"a\nb\nc\");",
		'error multiline comment before token' => "f(1 /*\n\n*/ 2);",
		'error doc comment before token' => "f(1 /**\n*/ 2);",
		'error unexpected token after heredoc' => "f(<<<A\nx\nA 1);",
		'error unexpected end after halt' => '__halt_compiler',
		'error unexpected halt args' => '__halt_compiler(1);',
	);
}

/** Hand-written cases whose source is written out in full: name => source. */
function parser_source_cases(): array {
	return array(
		'empty source'        => '',
		'only open tag'       => '<?php',
		'html only'           => "hello\nworld",
		'short tag'           => '<? echo 1;',
		'echo tag'            => '<?= $a, $b ?>x<?= 1 ?>',
		'echo tag eof'        => '<?= $a',
		'echo tag empty'      => '<?= ?>',
		'close tag newline'   => "<?php echo 1 ?>\n<?php echo 2;",
		'html between'        => "a<?php if (1): ?>b<?php else: ?>c<?php endif; ?>d",
		'html in function'    => "<?php function f() { ?>body<?php } ?>",
		'tags with comments'  => "<?php // c ?>x",
		'shebang source'      => "#!/usr/bin/env php\n<?php echo 1;",
		'crlf lines'          => "<?php\r\n\$a\r\n=\r\n;",
		'cr lines'            => "<?php\r\$a\r=\r;",
		'halt then data'      => "<?php __halt_compiler();\x00\x01 garbage ?> <?php",
		'error close tag in expr' => '<?php $a = ?>',
		'error open tag in code' => '<?php $a = <?php 1;',
	);
}

/** Operands, binary operators and prefix operators the random expressions are built from. */
const PARSER_OPERANDS = array( '$a', '$b', '$c', '1', '2.5', "'s'", '"d$a"', 'A', 'f()', '$a[0]', '$a->b', 'A::B', 'A::$c', '[1, $b]', 'null', '$a->m()', 'new A', 'true' );
const PARSER_BINARY   = array( '+', '-', '*', '/', '%', '**', '.', '<<', '>>', '&', '|', '^', '&&', '||', 'and', 'or', 'xor', '??', '==', '!=', '===', '!==', '<', '<=', '>', '>=', '<=>', '|>', 'instanceof' );
const PARSER_PREFIX   = array( '!', '-', '+', '~', '@', '(int) ', '(string) ', '(bool) ', 'clone ', 'print ', 'throw ', 'include ', 'yield ', 'yield from ' );
const PARSER_ASSIGN   = array( '=', '+=', '.=', '??=', '**=', '=&' );
const PARSER_TARGETS  = array( '$a', '$b', '$a[0]', '$a->b', 'A::$c', '$a[]', '[$a, $b]', 'list($a)', '$a()' );

function parser_random_expr( int $depth ): string {
	if ( $depth <= 0 || mt_rand( 0, 99 ) < 25 ) {
		return PARSER_OPERANDS[ mt_rand( 0, count( PARSER_OPERANDS ) - 1 ) ];
	}
	$sub = function () use ( $depth ): string {
		return parser_random_expr( $depth - 1 );
	};
	switch ( mt_rand( 0, 11 ) ) {
		case 0:
		case 1:
		case 2:
		case 3:
			return $sub() . ' ' . PARSER_BINARY[ mt_rand( 0, count( PARSER_BINARY ) - 1 ) ] . ' ' . $sub();
		case 4:
			return PARSER_PREFIX[ mt_rand( 0, count( PARSER_PREFIX ) - 1 ) ] . $sub();
		case 5:
			return $sub() . ' ? ' . $sub() . ' : ' . $sub();
		case 6:
			return $sub() . ' ?: ' . $sub();
		case 7:
			return '(' . $sub() . ')';
		case 8:
			$target = mt_rand( 0, 3 ) === 0 ? $sub() : PARSER_TARGETS[ mt_rand( 0, count( PARSER_TARGETS ) - 1 ) ];
			$op     = PARSER_ASSIGN[ mt_rand( 0, count( PARSER_ASSIGN ) - 1 ) ];
			return '=&' === $op ? $target . ' = &' . $sub() : $target . ' ' . $op . ' ' . $sub();
		case 9:
			return mt_rand( 0, 1 ) ? '$a++' : '--' . PARSER_TARGETS[ mt_rand( 0, 4 ) ];
		case 10:
			return 'fn() => ' . $sub();
		default:
			return 'f(' . $sub() . ', ' . $sub() . ')';
	}
}

/** One-token edits applied to valid hand-written cases. */
const PARSER_INSERTS = array( ';', ',', '(', ')', '[', ']', '{', '}', '$x', 'A', '1', '=', '=>', '?', ':', '::', '->', 'function', 'class', 'static', 'fn', 'new', 'yield', 'else', 'case', 'public', '&', '...', '"s"', 'as', '#[A]' );

function parser_mutation( string $source ): string {
	$tokens = array_values(
		array_filter(
			PhpToken::tokenize( $source ),
			function ( PhpToken $token ): bool {
				return ! $token->isIgnorable();
			}
		)
	);
	$texts  = array_map(
		function ( PhpToken $token ): string {
			return $token->text;
		},
		$tokens
	);
	$count  = count( $texts );
	$at     = mt_rand( 1, max( 1, $count - 1 ) );
	switch ( mt_rand( 0, 3 ) ) {
		case 0:
			array_splice( $texts, $at, 1 );
			break;
		case 1:
			array_splice( $texts, $at, 0, array( $texts[ $at ] ?? ';' ) );
			break;
		case 2:
			if ( $at + 1 < $count ) {
				list( $texts[ $at ], $texts[ $at + 1 ] ) = array( $texts[ $at + 1 ], $texts[ $at ] );
			}
			break;
		default:
			array_splice( $texts, $at, 0, array( PARSER_INSERTS[ mt_rand( 0, count( PARSER_INSERTS ) - 1 ) ] ) );
	}
	return implode( ' ', $texts );
}

/** Every case as [name, source]; the same list in every run. */
function parser_cases(): array {
	$cases = array();
	$seen  = array();
	$add   = function ( string $name, string $source ) use ( &$cases, &$seen ): void {
		if ( ! isset( $seen[ $source ] ) ) {
			$seen[ $source ] = true;
			$cases[]         = array( $name, $source );
		}
	};
	$valid = array();
	foreach ( parser_code_cases() as $name => $code ) {
		$add( $name, '<?php ' . $code );
		if ( ! str_starts_with( $name, 'error ' ) ) {
			$valid[] = '<?php ' . $code;
		}
	}
	foreach ( parser_source_cases() as $name => $source ) {
		$add( $name, $source );
	}
	foreach ( oracle_cases() as list( $name, $source ) ) {
		$add( 'lexer ' . $name, $source );
	}
	mt_srand( PARSER_SEED, MT_RAND_MT19937 );
	for ( $i = 1; $i <= PARSER_RANDOM_EXPRS; $i++ ) {
		$add( 'expression ' . $i, '<?php ' . parser_random_expr( mt_rand( 1, 4 ) ) . ';' );
	}
	for ( $i = 1; $i <= PARSER_MUTATIONS; $i++ ) {
		$add( 'mutation ' . $i, parser_mutation( $valid[ mt_rand( 0, count( $valid ) - 1 ) ] ) );
	}
	foreach ( parser_topic_cases() as $topic => $codes ) {
		foreach ( $codes as $index => $code ) {
			$add( $topic . ' ' . ( $index + 1 ), '<?php ' . $code );
		}
	}
	return $cases;
}

/**
 * Cases grouped by the grammar area they probe: topic => list of code following "<?php ".
 * Listed after the mutations so that adding cases leaves the seeded cases unchanged.
 */
function parser_topic_cases(): array {
	return array(
		'argument keyword' => array(
			'f(new namespace);', 'f(new );', 'f(new 1);', 'f(new);', 'f(new A, new 1);', 'f(1, new namespace);',
			'#[A(new namespace)] function g(){}', 'new namespace;', 'f(a: new namespace);',
			'f(new A);', 'f(new static);', 'f(new class {});', 'f(new #[X] class {});', 'f(new $a);', 'f(new (A));',
			'f(new \\A);', 'f(new namespace\\A);',
			'f(static 1);', 'f(static);', 'f(static $a);', 'f(1, static 1);', 'f(...static 1);',
			'f(static::A);', 'f(static fn() => 1);', 'f(static function() {});',
			'f(function 1);', 'f(fn 1);', 'f(exit 1);', 'f(yield ;);', 'f(__LINE__ 1);',
			'f(match 1);', 'f(list 1);', 'f(array 1);', 'f(isset 1);', 'f(empty 1);', 'f(empty);', 'f(eval 1);',
			'f(array(1));', 'f(list($a) = $b);', 'f(match(1) {});', 'f(isset($a));', 'f(eval("1"));',
			'f(clone ;);', 'f(clone);', 'f(print ;);', 'f(include ;);', 'f(include_once ;);', 'f(require ;);',
			'f(require_once ;);', 'f(throw ;);',
			'f(clone $a);', 'f(clone(...));', 'f(print 1);', 'f(include "a");', 'f(throw $e);',
		),
		'use statement' => array(
			'use 1;', 'use A 1;', 'use A, 1;', 'use A\\B, ;', 'use A as 1;', 'use function 1;', 'use const 1;',
			'use function A, 1;', 'use \\A\\{B};', 'use A\\{B} 1', 'use A\\{B}, C;',
			'use Baz\\{};', 'use Baz\\{,};', 'use Baz\\{Foo,,};', 'use Foo\\Bar\\{\\Baz};', 'use Foo\\{A,};',
			'use function Baz\\{,Foo};', 'use function Baz\\{Foo,,};', 'use function A\\{B, function C};',
			'use const Foo\\{A, const B};', 'use Foo\\{A, const B};', 'use Foo\\{A, function};', 'use Foo\\{const 1};',
			'use Foo\\{A as};', 'use Foo\\{A as 1};', 'use A\\{B as C 1};', 'use A\\{function B as 1};',
			'use Foo\\{A 1};', 'use function Foo\\{A 1};',
		),
		'for void cast' => array(
			'for (;(void)true;);', 'for (;1, (void)2;);', 'for (;(void)1, 2;);', 'for ((void)1;;);',
			'for ((void)1, 2;;);', 'for (;;(void)1);', 'for (;;1, (void)2);',
		),
		'array list assignment' => array(
			'[list($x) = $g];', '[[$x] = $g];', '[$i => list($x) = $g];', '[$i => [$x] = $g];', '[&list($x) = $g];',
			'[...[$x] = $g];', '[1, [$x] = $g];', '[$i => list($x)];', 'f([$x] = $g);',
		),
		'halt in braces' => array(
			"namespace foo {\n    __halt_compiler();\n\n?>", 'namespace foo { __halt_compiler(); }',
			"namespace foo {\n__halt_compiler() ?>\n", 'namespace a { (__halt_compiler());',
			'namespace a; __halt_compiler(); {',
		),
		'heredoc segment indentation' => array(
			"echo <<<END\n a\n\$a\n END;", "echo <<<END\n a\n {\$a}\n END;", "echo <<<END\n  a\n \$a\n  END;",
			"echo <<<END\n\t a\n\t \$a\n\t END;",
		),
	);
}

/** The source wrapped as the body of a closure whose syntax tree assert() renders. */
function parser_wrapped( string $source ): string {
	$tokens = PhpToken::tokenize( $source );
	$last   = end( $tokens );
	$html   = false === $last || in_array( $last->id, array( T_INLINE_HTML, T_CLOSE_TAG ), true );
	return "<?php assert(function () {\n?>" . $source . ( $html ? '<?php ' : '' ) . "\n} && false);";
}

/** Worker loop: reads one base64-encoded source per line and prints the rendering (or null) as JSON. */
function parser_render_worker(): void {
	while ( false !== ( $line = fgets( STDIN ) ) ) {
		$source    = base64_decode( trim( $line ), true );
		$rendering = null;
		try {
			eval( '?>' . parser_wrapped( $source ) );
		} catch ( AssertionError $error ) {
			$rendering = $error->getMessage();
		} catch ( Throwable $error ) {
			$rendering = null;
		}
		echo json_encode( null === $rendering ? null : oracle_bytes( $rendering ), JSON_THROW_ON_ERROR ), "\n";
	}
}

/** Renders sources through worker processes, restarting a worker that dies. */
final class ParserRenderer {
	private $process = null;
	private array $pipes = array();

	public function render( string $source ): ?string {
		if ( null === $this->process ) {
			$this->start();
		}
		fwrite( $this->pipes[0], base64_encode( $source ) . "\n" );
		$line = fgets( $this->pipes[1] );
		if ( false === $line ) {
			$this->stop();
			return null;
		}
		return json_decode( $line, true, 2, JSON_THROW_ON_ERROR );
	}

	private function start(): void {
		$command       = array( PHP_BINARY, '-d', 'zend.assertions=1', '-d', 'assert.exception=1', '-d', 'display_errors=0', '-d', 'log_errors=0', __FILE__, PARSER_WORKER_FLAG );
		$this->process = proc_open( $command, array( 0 => array( 'pipe', 'r' ), 1 => array( 'pipe', 'w' ), 2 => array( 'file', '/dev/null', 'w' ) ), $this->pipes );
		if ( false === $this->process ) {
			throw new RuntimeException( 'cannot start a render worker' );
		}
	}

	public function stop(): void {
		if ( null !== $this->process ) {
			fclose( $this->pipes[0] );
			fclose( $this->pipes[1] );
			proc_close( $this->process );
			$this->process = null;
		}
	}
}

/**
 * Parses $source, returning the error raised (class, message and line) and the compile warnings as
 * [severity, message] pairs. Compile warnings never reach a user error handler, so they are read back
 * from the displayed output. An error raised while no source is being compiled carries the line of
 * the code that asked for the parse; its line is recorded as null.
 */
function parser_parse( string $source ): array {
	ini_set( 'display_errors', 'stdout' );
	ini_set( 'html_errors', '0' );
	ini_set( 'error_prepend_string', '' );
	ini_set( 'error_append_string', '' );
	$error = null;
	ob_start();
	try {
		PhpToken::tokenize( $source, TOKEN_PARSE );
	} catch ( CompileError $thrown ) {
		$line  = __FILE__ === $thrown->getFile() ? null : $thrown->getLine();
		$error = array( get_class( $thrown ), oracle_bytes( $thrown->getMessage() ), $line );
	} finally {
		$output = (string) ob_get_clean();
	}
	$pattern = '/^(Warning|Deprecated|Notice): (.*?) in .* on line \d+$/m';
	preg_match_all( $pattern, $output, $matches, PREG_SET_ORDER );
	if ( '' !== trim( preg_replace( $pattern, '', $output ) ) ) {
		throw new RuntimeException( 'unexpected output: ' . $output );
	}
	$warnings = array_map(
		function ( array $match ): array {
			return array( $match[1], oracle_bytes( $match[2] ) );
		},
		$matches
	);
	return array( $error, $warnings );
}

function parser_record( string $name, string $source, ParserRenderer $renderer ): array {
	list( $error, $warnings ) = parser_parse( $source );
	return array(
		'name'      => $name,
		'source'    => oracle_bytes( $source ),
		'error'     => $error,
		'warnings'  => $warnings,
		'rendering' => null === $error ? $renderer->render( $source ) : null,
	);
}

if ( in_array( PARSER_WORKER_FLAG, $argv ?? array(), true ) ) {
	parser_render_worker();
	return;
}

$renderer = new ParserRenderer();
$records  = array();
foreach ( parser_cases() as list( $name, $source ) ) {
	$records[] = parser_record( $name, $source, $renderer );
}
$renderer->stop();

echo json_encode(
	array(
		'schema'         => PARSER_SCHEMA,
		'php_version'    => PHP_VERSION,
		'short_open_tag' => (bool) ini_get( 'short_open_tag' ),
		'cases'          => $records,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
), "\n";
