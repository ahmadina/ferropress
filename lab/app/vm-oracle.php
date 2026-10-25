<?php
/*
 * Ferropress lab VM oracle: runs short scripts with the reference PHP CLI and prints what each one
 * printed and its exit status, as JSON.
 *
 * Every case is written to a file of its own and run in a fresh process with no php.ini and fixed
 * settings: display_errors=1, error_reporting=-1, log_errors=0, html_errors=0. Output goes to stdout
 * and must leave stderr empty. The case's file path in the output is replaced by `%file%`.
 *
 * Cases only use what the Ferropress VM runs so far; each one is replayed by `crates/php-vm`.
 *
 * Run by `cargo xtask lab vm`; never part of Ferropress.
 */

declare(strict_types=1);

const VM_SCHEMA = 1;
const VM_FILE   = '%file%';

/** Hand-written cases: topic => list of code following "<?php\n". */
function vm_topic_cases(): array {
	return array(
		'echo'          => array(
			'echo 1, " ", -2, " ", 1.5, " ", 0.1 + 0.2, " ", 1e100, " ", -0.0, " ", 7.0, " ", true, "|", false, "|", null, "|";',
			'echo 9223372036854775807 + 1, " ", -9223372036854775807 - 2, " ", 2 ** 63, " ", 2 ** -1, " ", 10 / 4, " ", 10 / 5;',
			'echo "a" . 1 . 2.5 . true . null . false, "\n"; print "p"; echo print "q";',
			'echo 1e15, " ", 1e14, " ", 123456789012345678, " ", 0.00001, " ", 1/3, " ", -1/3;',
			"echo 'single \$x\\n', \"double\\t\\x41\\101\\u{1F600}\";",
		),
		'arithmetic'    => array(
			'echo 7 % 3, " ", -7 % 3, " ", 7 % -3, " ", 2 ** 10, " ", 2 ** 0.5, " ", 7 <=> 3, " ", "a" <=> "b", " ", 1 <=> 1.0;',
			'echo 1 << 3, " ", -8 >> 1, " ", 6 & 3, " ", 6 | 3, " ", 6 ^ 3, " ", ~5, " ", ~"a" === "\x9e" ? "y" : "n";',
			'echo "10" + 5, " ", "1.5" + 1, " ", "0x1A" + 0, " ", " 12" + 1, " ", "12 " + 1, " ", "1e3" + 0;',
			'echo "5 apples" + 1;',
			'echo "apples" + 1;',
			'echo 1 / 0;',
			'echo 1 % 0;',
			'echo 1 << -1;',
			'echo 1.5 % 1, " ", 7.9 % 2;',
			'echo 0 ** -1;',
		),
		'comparison'    => array(
			'echo (1 == "1") ? "t" : "f", (1 === "1") ? "t" : "f", ("abc" == 0) ? "t" : "f", (null == false) ? "t" : "f", ("1e3" == "1000") ? "t" : "f";',
			'echo ("abc" < "abd") ? "t" : "f", (2 > 1) ? "t" : "f", (1 >= 1.0) ? "t" : "f", (null < -1) ? "t" : "f", ("10" < "9") ? "t" : "f", ("10" < "9a") ? "t" : "f";',
			'echo (1 != 2) ? "t" : "f", (1 !== 1.0) ? "t" : "f", (true xor true) ? "t" : "f", (true xor false) ? "t" : "f", !0, !1, "|";',
		),
		'variables'     => array(
			'echo $u;',
			'echo $u + 1, "|", $v . "x", "|", -$w;',
			'$a = 1; $b = $a; $b = 2; echo $a, $b;',
			'$a = 1; echo $a + ($a = 5), " ", $a . ($a = 2), " ", ($a += 1) + ($a = 5);',
			'$i = 1; echo $i + $i++, " ", $i, " ", $i-- - --$i, " ", $i;',
			'$a = "5"; $a .= 5; echo $a; $a += 1; echo " ", $a; $a -= 0.5; echo " ", $a; $a *= 2; echo " ", $a; $a /= 4; echo " ", $a;',
			'$a = 7; $a %= 4; $a **= 3; $a <<= 1; $a >>= 2; $a |= 8; $a &= 13; $a ^= 1; echo $a;',
			'$i++; echo $i; $j--; echo $j; ++$k; echo $k; --$l; echo $l === null ? "null" : "x";',
			'$s = "a"; $s++; echo $s; $s = "Az"; $s++; echo " ", $s; $s = "zz"; $s++; echo " ", $s; $s = "a9"; $s++; echo " ", $s;',
			'$s = ""; $s--; echo $s === -1 ? "m" : "x"; $n = null; $n--; echo $n === null ? "n" : "x"; $b = true; $b++; echo $b;',
			'$f = 1.5; $f++; echo $f; $m = 9223372036854775807; $m++; echo " ", $m;',
			'$a;',
		),
		'strings'       => array(
			'$a = 1; $b = "x"; echo "v=$a, {$b}!", "\n", "$a$b", "\n", "{$a}", "\n";',
			'$a = 1.0; $b = null; $c = false; echo "[$a][$b][$c]";',
			'echo "$u-{$v}";',
		),
		'control flow'  => array(
			'if (0) { echo "a"; } elseif ("0") { echo "b"; } elseif ("0.0") { echo "c"; } else { echo "d"; }',
			'$i = 0; while ($i < 5) { $i++; if ($i == 2) continue; if ($i == 4) break; echo $i; }',
			'$i = 0; do { echo $i; $i++; } while ($i < 3); do { echo "once"; } while (false);',
			'for ($i = 0, $j = 10; $i < $j; $i += 3, $j -= 3) { echo $i, ":", $j, " "; }',
			'for ($i = 0; ; $i++) { if ($i > 2) { break; } echo $i; } for (;;) { echo "x"; break; }',
			'for ($i = 0; $i < 3; $i++) { for ($j = 0; $j < 3; $j++) { if ($j == 1) continue 2; if ($i == 2) break 2; echo $i, $j, " "; } }',
			'$i = 0; while (true) { while (true) { $i++; if ($i > 3) break 2; continue 2; } }  echo $i;',
			'echo 0 ?: "zero", " ", 5 ?: "five", " ", null ? "a" : (false ? "b" : "c");',
			'echo (1 && "0") ? "t" : "f", (0 || "a") ? "t" : "f", (1 and 0) ? "t" : "f", (0 or 1) ? "t" : "f";',
			'function t() { echo "t"; return true; } function f() { echo "f"; return false; } echo (f() && t()) ? "Y" : "N", (t() || f()) ? "Y" : "N";',
			'if ($u) { echo "set"; } else { echo "unset"; }',
		),
		'functions'     => array(
			'function add($a, $b) { return $a + $b; } echo add(1, 2), add("3", 4.5);',
			'function fib($n) { if ($n < 2) { return $n; } return fib($n - 1) + fib($n - 2); } echo fib(20);',
			'function nothing() {} function early($x) { if ($x) { return "yes"; } echo "no"; } echo nothing() === null ? "null" : "x"; echo early(1); early(0);',
			'function inc(&$x) { $x++; } $a = 1; inc($a); inc($a); echo $a; inc($b); echo $b;',
			'function set(&$x, $v) { $x = $v; } set($n, "new"); echo $n;',
			'function swap(&$a, &$b) { $t = $a; $a = $b; $b = $t; } $x = 1; $y = 2; swap($x, $y); echo $x, $y;',
			'function keep($x) { $x = 5; return $x; } $a = 1; echo keep($a), $a;',
			'function sum(...$n) { $t = 0; $i = 0; return $n; } echo count2(1);',
			'function F() { return "called"; } echo f(), FOO(), "never";',
			'function late() { return early2(); } function early2() { return "ok"; } echo late();',
			'function g($a) { echo $a; } g($u);',
			'function two($a, $b) { return $a . $b; } echo two(1);',
			'function more($a, $b) { return $a . $b; } echo more(1, 2, 3, 4);',
			'function v($a, ...$rest) { return $a; } echo v(1, 2, 3), v();',
			'function deep($n) { if ($n == 0) { return 1 % 0; } return deep($n - 1); } deep(3);',
			'function args($a, $b) { $a = "changed and long string"; return 1 % 0; } args(1.0, 2.5, null, true, "a\nb\\c\x01é");',
			'function r($x) { return $x; } echo r(r(r("nested")));',
			"function lines(\n  \$a,\n  \$b\n) {}\nlines(1);",
			'function rec($n) { return $n <= 0 ? 0 : 1 + rec($n - 1); } echo rec(5000);',
		),
		'closures'      => array(
			'$f = function($x) { return $x + 1; }; echo $f(2);',
			'$f = fn($x) => $x * 2; echo $f(3);',
			'$a = 10; $f = function($x) use ($a) { return $x + $a; }; $a = 1; echo $f(2);',
			'$a = 10; $f = function() use (&$a) { $a++; }; $f(); echo $a;',
			'$a = 3; $f = fn($x) => $x + $a; $a = 0; echo $f(1);',
			'$f = function() { return function($x) { return $x; }; }; $g = $f(); echo $g(7);',
			'function add2($a, $b) { return $a + $b; } $n = "ADD2"; echo $n(2, 3);',
			'echo (function($x) { return $x; })(5);',
			'$f = 1; $f();',
			'$f = function() {}; echo $f;',
			'$u = function() use ($missing) { return $missing; }; echo $u();',
			'$f = function() { echo 1 % 0; }; $f();',
			'$n = "missing"; $n();',
			'$f();',
		),
		'classes'       => array(
			'class C { public $a = 1; function m($x) { return $x + $this->a; } } $o = new C; echo $o->m(2); $o->a = 10; echo $o->m(3);',
			'class C { public $a; public $b = 1.5; public $c = "x"; public $d = true; public $e = false; } $o = new C; echo $o->a === null ? "n" : "x", $o->b, $o->c, $o->d, $o->e;',
			'class C { public $a; function __construct($n) { $this->a = $n; } } $o = new C(7); echo $o->a;',
			'class C {} $o = new C; echo $o->z;',
			'class C {} $i = 0; new C($i += 1); echo $i;',
			'class C { function __construct() { echo "c"; } } new C(1);',
			'new Missing();',
			'class C {} $o = new C; $o->m();',
			'$a = 1; $a->p = 2;',
			'class C { function M() { return 1; } } $o = new c; echo $o->m();',
			'class C { public $a = 1; } $x = new C; $y = new C; $x->a = 2; echo $x->a, $y->a;',
			'echo (1)->z;',
			'$a = 1; $a->m();',
			'$a = true; $a->p = 1;',
			'interface A {} interface B extends A {} class C implements B {} $o = new C; echo $o instanceof C ? "c" : "n", $o instanceof B ? "b" : "n", $o instanceof A ? "a" : "n", $o instanceof Missing ? "m" : "n", 1 instanceof C ? "y" : "n";',
			'$e = new Exception("x"); echo $e instanceof Exception ? "e" : "n", $e instanceof Throwable ? "t" : "n", $e instanceof Error ? "r" : "n";',
			'interface I {} new I;',
			'trait U { function m() { echo "u"; } } trait T { use U; } class C { use T; function n() { echo "c"; } } $o = new C; $o->m(); $o->n();',
			'trait T { function m() { echo "t"; } } class C { use T; function m() { echo "c"; } } (new C)->m();',
			'trait T {} new T;',
			'enum E { case A; case B; } $a = E::A; echo $a instanceof E ? "y" : "n", $a->name, $a === E::A ? "s" : "d", $a === E::B ? "b" : "n";',
			'enum E { case A; } new E;',
			'enum E { case A; } echo E::Z;',
		),
		'exceptions'    => array(
			'try { echo 1; throw new Exception("x"); echo 2; } catch (Exception $e) { echo "c"; }',
			'throw new Exception("x");',
			'try { echo 1 % 0; } catch (Error $e) { echo "z"; }',
			'throw 1;',
			'class E {} throw new E;',
			'function f() { throw new Exception("in"); } try { f(); echo "n"; } catch (Exception $e) { echo "c"; }',
			'try { throw new Exception("x"); } catch (Error $e) { echo "e"; } catch (Exception $e) { echo "x"; }',
			'try { throw new Exception("x"); } catch (Throwable $e) { echo "t"; }',
			'try { echo 1; } finally { echo 2; }',
			'try { throw new Exception("x"); } catch (Exception $e) { echo "c"; } finally { echo "f"; }',
			'try { throw new Exception("x"); } finally { echo "f"; }',
			'function f() { try { return 1; } finally { echo "f"; } } echo f();',
		),
		'builtins'      => array(
			'echo strlen("AbC"), strtolower("AbC"), strtoupper("AbC");',
			'echo count(1);',
			'echo gettype(1), gettype(1.5), gettype(true), gettype(null), gettype("a"); echo is_int(1) ? "y" : "n", is_string(1) ? "y" : "n", is_numeric("12") ? "y" : "n", is_numeric("12x") ? "y" : "n";',
			'function f() {} echo function_exists("f") ? "y" : "n", function_exists("strlen") ? "y" : "n", function_exists("missing") ? "y" : "n";',
			'class C {} echo class_exists("C") ? "y" : "n", class_exists("Exception") ? "y" : "n", class_exists("Missing") ? "y" : "n";',
			'strlen();',
			'echo count(null);',
			'echo strlen(12);',
			'echo is_null(null) ? "y" : "n", is_bool(false) ? "y" : "n", is_float(1.5) ? "y" : "n";',
			'$n = "strlen"; echo $n("ab");',
		),
		'generators'    => array(
			'function g() { yield 1; yield 2; } $g = g(); echo $g->current(); $g->next(); echo $g->current(); echo $g->valid() ? "y" : "n"; $g->next(); echo $g->valid() ? "y" : "n";',
			'function g() { yield 1; return 9; } $g = g(); echo $g->current(); $g->next(); echo $g->getReturn();',
			'function g() { yield "a" => 1; } $g = g(); echo $g->key(), $g->current();',
			'function g() { $x = (yield 1); yield $x; } $g = g(); echo $g->send("z");',
			'function g() { yield 1; } echo g() instanceof Generator ? "y" : "n";',
			'function g() { yield 1; } $g = g(); $g->getReturn();',
			'function g() { yield 1; } $g = g(); $g->current(); $g->rewind();',
			'function g() { yield 7; } $g = g(); $g->rewind(); echo $g->current();',
			'function g() { try { yield 1; } catch (Exception $e) { yield "c"; } } $g = g(); $g->current(); $g->throw(new Exception("x")); echo $g->current();',
			'new Generator;',
			'function g() { yield 1; } g()->missing();',
		),
		'fibers'        => array(
			'$f = new Fiber(function() { Fiber::suspend(1); return 2; }); echo $f->start(); echo $f->resume(); echo $f->getReturn();',
			'$f = new Fiber(function() { $x = Fiber::suspend(1); Fiber::suspend($x); }); echo $f->start(); echo $f->resume("z");',
			'$f = new Fiber(function() { Fiber::suspend(1); }); echo $f->isStarted() ? "y" : "n"; $f->start(); echo $f->isSuspended() ? "y" : "n"; $f->resume(); echo $f->isTerminated() ? "y" : "n";',
			'$f = new Fiber(function() { return 9; }); echo $f->start() === null ? "n" : "x"; echo $f->getReturn();',
			'Fiber::suspend(1);',
			'$f = new Fiber(function() {}); $f->resume();',
			'$f = new Fiber(function() { Fiber::suspend(1); }); $f->start(); $f->start();',
			'$f = new Fiber(function() { Fiber::suspend(1); }); $f->start(); $f->getReturn();',
			'new Fiber(1);',
			'$f = new Fiber(function() { throw new Exception("x"); }); try { $f->start(); echo "n"; } catch (Exception $e) { echo "c"; }',
			'$f = new Fiber(function() { try { Fiber::suspend(1); } catch (Exception $e) { Fiber::suspend("c"); } }); echo $f->start(); echo $f->throw(new Exception("x"));',
			'function f() { Fiber::suspend(3); } $fiber = new Fiber("f"); echo $fiber->start();',
			'$f = new Fiber(function() {}); echo $f instanceof Fiber ? "y" : "n"; echo (new FiberError("x")) instanceof Error ? "y" : "n";',
		),
		'includes'      => array(
			'echo include "missing-ferropress.php";',
			'require "missing-ferropress.php";',
			'include_once "missing-ferropress.php"; include_once "missing-ferropress.php";',
		),
		'errors'        => array(
			"echo 1;\necho 1 % 0;\necho 2;",
			'function f($x) { return $x / 0; } echo "before"; f("1");',
			'echo -"abc";',
			'echo "x" * "y";',
			'echo 1.5 | 1;',
			'echo 1e30 | 1;',
		),
	);
}

/** Every case as [name, source]. */
function vm_cases(): array {
	$cases = array();
	foreach ( vm_topic_cases() as $topic => $sources ) {
		foreach ( $sources as $index => $source ) {
			$cases[] = array( sprintf( '%s %02d', $topic, $index + 1 ), "<?php\n" . $source );
		}
	}
	return $cases;
}

function oracle_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

/** Runs the script at $path and returns [stdout, exit status]. */
function vm_run( string $path ): array {
	$command = array( PHP_BINARY, '-n', '-d', 'display_errors=1', '-d', 'error_reporting=-1', '-d', 'log_errors=0', '-d', 'html_errors=0', $path );
	$process = proc_open( $command, array( 1 => array( 'pipe', 'w' ), 2 => array( 'pipe', 'w' ) ), $pipes );
	if ( false === $process ) {
		throw new RuntimeException( 'cannot start PHP' );
	}
	$stdout = stream_get_contents( $pipes[1] );
	$stderr = stream_get_contents( $pipes[2] );
	fclose( $pipes[1] );
	fclose( $pipes[2] );
	$status = proc_close( $process );
	if ( '' !== $stderr ) {
		throw new RuntimeException( "unexpected stderr running $path: $stderr" );
	}
	return array( str_replace( $path, VM_FILE, $stdout ), $status );
}

if ( realpath( (string) ( $_SERVER['SCRIPT_FILENAME'] ?? '' ) ) !== __FILE__ ) {
	return;
}

$dir = sys_get_temp_dir() . '/ferropress-vm-' . getmypid();
if ( ! is_dir( $dir ) && ! mkdir( $dir ) ) {
	throw new RuntimeException( "cannot create $dir" );
}
$records = array();
foreach ( vm_cases() as $index => list( $name, $source ) ) {
	$path = sprintf( '%s/case-%05d.php', $dir, $index );
	file_put_contents( $path, $source );
	list( $output, $status ) = vm_run( $path );
	unlink( $path );
	$records[] = array(
		'name'   => $name,
		'source' => oracle_bytes( $source ),
		'output' => oracle_bytes( $output ),
		'status' => $status,
	);
}
rmdir( $dir );

echo json_encode(
	array(
		'schema'      => VM_SCHEMA,
		'php_version' => PHP_VERSION,
		'cases'       => $records,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
), "\n";
