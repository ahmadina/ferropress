<?php
/*
 * Ferropress lab lexer oracle: prints how the reference PHP splits source into tokens, as JSON.
 *
 * Each case is a source text, hand-written or generated from a fixed seed by joining fragments. For
 * every case the oracle records the tokenizer's view (each token's name, byte length and line), the
 * warnings raised while tokenizing, the error compiling the same source reports (scanner errors only
 * surface there, the tokenizer discards them) and the value or error of every constant string literal.
 * Cases containing "<?" are recorded a second time with short_open_tag disabled, in a child process
 * because the setting cannot change at runtime.
 *
 * Run by `cargo xtask lab lexer`; never part of Ferropress.
 */

declare(strict_types=1);

const ORACLE_SCHEMA = 1;
const RANDOM_SEED = 20261023;
const RANDOM_CASES = 2500;

/** Hand-written cases: name => source. */
function oracle_hand_cases(): array {
	$cases = array(
		'empty'                 => '',
		'inline only'           => "hello\nworld",
		'inline lt'             => 'a < b <',
		'open tag space'        => '<?php echo 1;',
		'open tag newline'      => "<?php\necho 1;",
		'open tag crlf'         => "<?php\r\necho 1;",
		'open tag cr'           => "<?php\recho 1;",
		'open tag tab'          => "<?php\techo 1;",
		'open tag eof'          => '<?php',
		'open tag upper'        => '<?PHP echo 1;',
		'open tag glued'        => '<?phpecho 1;',
		'short open'            => '<? echo 1;',
		'echo tag'              => '<?= $a ?>',
		'inline then tags'      => "a<?php b ?>c<?= d ?>\ne",
		'inline lt question'    => 'x <?xml ?> y',
		'inline before php eof' => 'x<?php',
		'close crlf'            => "<?php ?>\r\nx",
		'close cr'              => "<?php ?>\rx",
		'close twice newline'   => "<?php ?>\n\nx",
		'whitespace'            => "<?php  \t\n\r\n\r  ;",
		'variables'             => '<?php $a $_b $é $ $1 $$a;',
		'names'                 => '<?php foo \foo foo\bar \foo\bar namespace\foo NameSpace\X foo\ \ a\1;',
		'keywords'              => '<?php abstract and array as break callable case catch class clone const continue declare default die do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile eval exit extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public readonly require require_once return static switch throw trait try unset use var while xor yield;',
		'keywords upper'        => '<?php FUNCTION Class ECHO NeW Or AnD XoR;',
		'keyword prefixes'      => '<?php functions classy echo_ newer iff;',
		'magic constants'       => '<?php __CLASS__ __TRAIT__ __FUNCTION__ __PROPERTY__ __METHOD__ __LINE__ __FILE__ __DIR__ __NAMESPACE__ __class__ __LINE;',
		'halt compiler'         => "<?php __halt_compiler(); raw \x00 bytes ( [",
		'halt compiler spaced'  => "<?php __HALT_COMPILER /* c */ ( ) ?>\nrest",
		'halt compiler eof'     => '<?php __halt_compiler(',
		'halt compiler lines'   => "<?php\n__halt_compiler\n(\n)\n;\nrest",
		'yield from'            => '<?php yield from $a; yield  from; yield/**/from x; yield#c' . "\n" . 'from; YIELD FROM y; yield fromage; yield' . "\n" . 'from',
		'yield from eof'        => '<?php yield from',
		'yield from hash split' => "<?php yield #\nfrom\n;",
		'yield then comment'    => '<?php yield // x',
		'enum'                  => '<?php enum Foo {} enum extends; enum implements; enum /* c */ Bar; enum; enum(1); enum
Baz; enum $x; ENUM Foo; enum # c
Foo; enum extendsX;',
		'enum hash newline'     => "<?php enum #\nextends\nFoo",
		'asymmetric visibility' => '<?php public(set) protected(set) private(set) PUBLIC(SET) public (set) public(get);',
		'casts'                 => '<?php (int) (integer) (bool) (boolean) (float) (double) (real) (string) (binary) (array) (object) (unset) (void) ( int ) (	string	) (INT) (Int ) (int (mixed) (int)(x);',
		'operators'             => '<?php + - * / % ** . = += -= *= /= .= %= **= &= |= ^= <<= >>= ??= == != <> === !== < > <= >= <=> && || ! & | ^ ~ << >> ? ?? : :: => -> ?-> ... ++ -- |> @ , ;',
		'ampersand'             => '<?php &$a & $a &...$a & ... &/* c */$a & // c' . "\n" . '$a &$ &= && &a &.. &' . "\n" . '#x' . "\n" . '$a',
		'ampersand eof'         => '<?php &',
		'brackets'              => '<?php ( [ { } ] ) #[A] ${ } {$a}',
		'unmatched close'       => '<?php )',
		'unmatched bracket'     => '<?php ]',
		'unmatched brace'       => '<?php }',
		'mismatch same line'    => '<?php ( ]',
		'mismatch other line'   => "<?php (\n]",
		'mismatch brace'        => "<?php {\n\n)",
		'unclosed'              => '<?php (',
		'unclosed other line'   => "<?php [\n\n",
		'unclosed attribute'    => '<?php #[A',
		'unclosed brace nested' => '<?php function f() { if (1) {',
		'close tag in brackets' => '<?php ( ?> x <?php )',
		'numbers'               => '<?php 0 1 123 1_000 1__0 1_ 0x1F 0XaB 0x 0b101 0B2 0o17 0O8 0777 09 0_8 1.5 1. .5 1e5 1E-5 1.5e+3 1e 1e+ 0.0 07.5 08.5 1_2.3_4e5_6;',
		'number limits'         => '<?php 9223372036854775807 9223372036854775808 0x7FFFFFFFFFFFFFFF 0x8000000000000000 0777777777777777777777 01000000000000000000000 0o777777777777777777777 0o1000000000000000000000 0b111111111111111111111111111111111111111111111111111111111111111 0b1111111111111111111111111111111111111111111111111111111111111111;',
		'octal invalid long'    => '<?php 0123456789012345678901234;',
		'single quoted'         => "<?php 'a' '' 'it\\'s' 'back\\\\slash' 'line\nbreak' b'x' B'y' '\\n';",
		'single unterminated'   => "<?php 'abc\ndef",
		'single unterminated b' => "<?php b'abc\\",
		'double constant'       => '<?php "a" "" "\\n\\t\\"" "\\$a" "$" "{" "{ $" "$1" b"x" B"\\x41" "\\u{1F600}" "\\101\\0\\7777";',
		'double escapes'        => '<?php "\\e\\f\\v\\r\\\\\\q\\x\\xg\\X4f\\u\\u{41}\\u{00000041}\\u{D800}";',
		'double bad codepoint'  => "<?php \"\\u{zz}\n\";\n\$x;",
		'double empty codepoint' => '<?php "\\u{}";',
		'double unclosed codepoint' => '<?php "\\u{41";',
		'double big codepoint'  => "<?php \"\\u{110000}\n\";\n\$y;",
		'double octal overflow' => '<?php "\\400 \\777 \\1234 \\477";',
		'double interpolation'  => '<?php "a $b c";',
		'double var property'   => '<?php "$a->b $a->1 $a-> $a?->b $a?->$c $a->b->c";',
		'double var offset'     => '<?php "$a[0] $a[12] $a[-1] $a[x] $a[$b] $a[0x1F] $a[0b1] $a[0o7] $a[01] $a[1_0] $a[9223372036854775808]";',
		'double offset bad'     => "<?php \"\$a[ ] \$a[\n] \$a['x'] \$a[\\x] \$a[#] \$a[\x01] \$a[{] \$a[\"]\"",
		'double offset eof'     => '<?php "$a[',
		'double dollar brace'   => '<?php "${a} ${a[0]} ${a . b} ${ a} ${1}";',
		'double curly'          => '<?php "{$a} {$a[0]} {$a->b} { $a} {$a{$b}}";',
		'double nested string'  => '<?php "{$a["k"]}";',
		'double unterminated'   => "<?php \"abc \$d\nef",
		'double unterminated plain' => "<?php \"abc\nef",
		'double backslash end'  => '<?php "a $b\\',
		'double lines'          => "<?php \"a\n\$b\nc\";\n\$d;",
		'double segment bad codepoint' => "<?php \"\$a \\u{zz}\nb\";\n\$c;",
		'backquote'             => '<?php `ls $dir -l` `` `a\\`b` `\\u{41}`;',
		'backquote unterminated' => '<?php `a $b',
		'heredoc'               => "<?php <<<EOT\nhello \$name\nEOT;\n",
		'heredoc quoted'        => "<?php <<<\"EOT\"\nhi\nEOT;\n",
		'nowdoc'                => "<?php <<<'EOT'\nhi \$name {\$x}\nEOT;\n",
		'heredoc empty'         => "<?php <<<EOT\nEOT;\n",
		'nowdoc empty'          => "<?php <<<'EOT'\nEOT;\n",
		'heredoc indented'      => "<?php <<<EOT\n    a\n      b\n    EOT;\n",
		'heredoc indented var'  => "<?php <<<EOT\n    \$a\n    EOT;\n",
		'heredoc under indented var' => "<?php <<<EOT\n\$a\n    EOT;\n",
		'heredoc under indented curly' => "<?php <<<EOT\n{\$a}\n  EOT;\n",
		'heredoc under indented dollar brace' => "<?php <<<EOT\n\${a}\n\tEOT;\n",
		'heredoc mixed indent'  => "<?php <<<EOT\n  a\n \tEOT;\n",
		'heredoc empty mixed'   => "<?php <<<EOT\n\t EOT;\n",
		'nowdoc mixed indent'   => "<?php <<<'EOT'\n  a\n\t EOT;\n",
		'heredoc spaced label'  => "<?php <<<  EOT\nx\nEOT\n",
		'heredoc tab label'     => "<?php <<<\tEOT\nx\nEOT\n",
		'heredoc crlf'          => "<?php <<<EOT\r\na\r\nEOT;\r\n",
		'heredoc cr'            => "<?php <<<EOT\ra\rEOT;\r",
		'heredoc label prefix'  => "<?php <<<EOT\nEOTX\nEOT\n",
		'heredoc label eof'     => "<?php <<<EOT\nEOT",
		'heredoc label in text' => "<?php <<<EOT\na EOT b\nEOT;",
		'heredoc unterminated'  => "<?php <<<EOT\nabc \$x\n",
		'heredoc eof after start' => "<?php <<<EOT\n",
		'heredoc eof indent'    => "<?php <<<EOT\n   ",
		'heredoc nested'        => "<?php <<<A\n{\$x(<<<B\n  inner\n  B)}\n    A;\n",
		'heredoc nested reversed' => "<?php <<<A\n  {\$x(<<<B\n      inner\n      B)}\n  A;\n",
		'heredoc error in body' => "<?php <<<A\n{\$a[}\n    A;\n",
		'heredoc escapes'       => "<?php <<<A\n\\\$x \\{\$y} \\\nz \\u{zz}\nA;\n",
		'heredoc interpolation' => "<?php <<<A\n\$a[0] \$a->b \${c} {\$d} \$\nA;\n",
		'heredoc b prefix'      => "<?php b<<<A\nx\nA;\nB<<<'B'\ny\nB;\n",
		'heredoc not a heredoc' => '<?php <<<A $x; <<< ; <<<"A ; <<<\'A',
		'heredoc in double'     => "<?php \"{\$a(<<<B\nx\nB)}\";",
		'comments'              => "<?php # hash\n// slash\n/* block */ /** doc */ /**/ /***/ /**\n*/ #[Attr] #\n//\n",
		'comment close tag'     => '<?php // c ?> x <?php # d ?>',
		'comment cr'            => "<?php // a\rb",
		'comment unterminated'  => "<?php\n\n/* never",
		'doc unterminated'      => '<?php /** never',
		'comment star slash'    => '<?php /*/ x */',
		'property'              => '<?php $a->b->c?->d $a -> b $a->/* c */b $a->class $a->123 $a->$b $a->{b} $a->
b',
		'property comment hash' => "<?php \$a->#[x]\nb;",
		'property eof'          => '<?php $a->',
		'double colon'          => '<?php A::b A::class static::$x;',
		'bad characters'        => "<?php \x00 \x01 \x08 \x0b \x0c \x0e \x1f \x7f ;",
		'high bytes'            => "<?php \x80 \xff \$\xe9t\xe9 caf\xc3\xa9;",
		'match'                 => '<?php match($a) { 1, 2 => "x", default => null };',
		'attributes'            => '<?php #[A(1), B] function f(#[C] $p) {}',
		'pipe'                  => '<?php $x |> strlen(...) |> f(...);',
		'void cast'             => '<?php (void) f();',
		'line counting'         => "<?php\n\$a\n\n\r\n\r\$b /*\n\n*/ \$c // x\r\n\$d 'x\ny' \"\nz\" ?>\n\n<?php \$e",
		'shebang'               => "#!/usr/bin/env php\n<?php echo 1;",
		'deep nesting'          => '<?php ' . str_repeat( '(', 40 ) . str_repeat( ')', 40 ),
		'state mix'             => '<?php "{$a["b{$c}"]}" `{$x}` "${y[0]}" "$z[k]";',
		'curly in code returns' => '<?php "{$a}" } "x"',
		'string varname bad'    => '<?php "${a b}";',
		'interpolation property chain' => '<?php "$a->b->c";',
		'offset then text'      => '<?php "$a[b]c";',
		'offset minus'          => '<?php "$a[-0] $a[--1]";',
		'php tag case inline'   => 'x <?PhP y',
		'utf8 identifiers'      => "<?php function \xc3\xa9t\xc3\xa9() { return \$\xc3\xa9; }",
		'numbers then names'    => '<?php 1abc 0x1g 1e5e5 1..2 1.2.3;',
		'dollar brace in code'  => '<?php ${a} $$a ${"a"};',
		'question marks'        => '<?php ?? ??= ?-> ?->a ?- ?',
		'close tag variants'    => "<?php ?>\r",
		'tokens only'           => '<?php ;:,.|^&+-/*=%!~$<>?@',
	);
	return $cases;
}

/** Fragments joined at random into generated cases. */
const FRAGMENTS = array(
	'<?php ', "<?php\n", '<?', '<?=', '?>', "?>\n", "\n", ' ', "\t", "\r\n", "\r", '$a', '$', '${', '{$', '{', '}',
	'(', ')', '[', ']', '#[', '"', "'", '`', '\\', '\\\\', '->', '?->', '::', '&', '&$x', '& ...', '&&', '=>',
	"<<<A\n", "<<<'A'\n", "<<<\"A\"\n", 'A', 'A;', "\nA\n", "\n  A", "\n\tA", '<<<', '0', '09', '0x1F', '0b2',
	'1_000', '1.5e3', '.5', '1e', '9223372036854775808', '0o17', "'a\\'b'", '"x\\u{41}"', '"\\u{zz}"', '"\\400"',
	'"$a[0]"', '"$a->b"', '"${a}"', '"{$a}"', '/*', '*/', '/** ', '//', '#', "# x\n", "// c\n", 'yield', 'from',
	'yield from', 'enum', 'extends', 'Foo', 'namespace', 'namespace\\X', '\\X\\Y', 'X\\Y', 'public(set)', '(int)',
	'( string )', '(real)', '__halt_compiler', 'die', 'fn', '|>', '<>', '<=>', '**=', '??=', '...', "\x00", "\x7f",
	"\x80", "\xc3\xa9", "b'x'", 'b"y"', "B<<<A\n", '$a[', '$a[-1]', '$a[x]', '$a[$b]', '$a[ ', '$a[0x1]', '${a[',
	'@', '~', ';', ',', '!', '!==', '=', '==', '===', '+', '++', '-', '--', '.', '.=', '%', '^', '|', '||', ':', '?',
	'??', '<', '<<', '<<=', '>', '>>', '>>=', '<?php', '<?PHP ', '<?phpx', '\\u{', '"', '"', "'", 'x', 'b', '$b',
);

function oracle_random_case(): string {
	$source = mt_rand( 0, 99 ) < 85 ? '<?php ' : '';
	$count  = mt_rand( 1, 14 );
	for ( $i = 0; $i < $count; $i++ ) {
		$source .= FRAGMENTS[ mt_rand( 0, count( FRAGMENTS ) - 1 ) ];
	}
	return $source;
}

/** Every case, hand-written first, as [name, source]; the same list in every process. */
function oracle_cases(): array {
	$cases = array();
	foreach ( oracle_hand_cases() as $name => $source ) {
		$cases[] = array( $name, $source );
	}
	mt_srand( RANDOM_SEED, MT_RAND_MT19937 );
	$seen = array();
	foreach ( $cases as $case ) {
		$seen[ $case[1] ] = true;
	}
	while ( count( $cases ) < count( oracle_hand_cases() ) + RANDOM_CASES ) {
		$source = oracle_random_case();
		if ( ! isset( $seen[ $source ] ) ) {
			$seen[ $source ] = true;
			$cases[]         = array( 'random ' . ( count( $cases ) - count( oracle_hand_cases() ) + 1 ), $source );
		}
	}
	return $cases;
}

function oracle_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

/**
 * Runs $work, returning its result and the messages of the warnings it raised. Scanner warnings are
 * compile warnings, which never reach a user error handler, so they are read back from the displayed
 * output instead.
 */
function oracle_with_messages( callable $work ): array {
	ini_set( 'display_errors', 'stdout' );
	ini_set( 'html_errors', '0' );
	ini_set( 'error_prepend_string', '' );
	ini_set( 'error_append_string', '' );
	ob_start();
	try {
		$result = $work();
	} finally {
		$output = (string) ob_get_clean();
	}
	preg_match_all( '/^(?:Warning|Deprecated|Notice): (.*?) in .* on line \d+$/m', $output, $matches );
	$unexplained = trim( preg_replace( '/^(?:Warning|Deprecated|Notice): .* on line \d+$/m', '', $output ) );
	if ( '' !== $unexplained ) {
		throw new RuntimeException( 'unexpected output: ' . $unexplained );
	}
	return array( $result, array_map( 'oracle_bytes', $matches[1] ) );
}

function oracle_literal( string $text ): array {
	try {
		list( $value ) = oracle_with_messages(
			function () use ( $text ) {
				return eval( 'return ' . $text . ';' );
			}
		);
		return array( oracle_bytes( $value ), null );
	} catch ( ParseError $error ) {
		return array( null, oracle_bytes( $error->getMessage() ) );
	}
}

function oracle_record( string $name, string $source ): array {
	list( $tokens, $warnings ) = oracle_with_messages(
		function () use ( $source ) {
			return PhpToken::tokenize( $source );
		}
	);
	$recorded = array();
	$literals = array();
	foreach ( $tokens as $token ) {
		$recorded[] = array( $token->id < 256 ? chr( $token->id ) : token_name( $token->id ), strlen( $token->text ), $token->line );
		if ( T_CONSTANT_ENCAPSED_STRING === $token->id ) {
			$literals[] = oracle_literal( $token->text );
		}
	}
	$parse_error = null;
	try {
		oracle_with_messages(
			function () use ( $source ) {
				return PhpToken::tokenize( $source, TOKEN_PARSE );
			}
		);
	} catch ( ParseError $error ) {
		$parse_error = oracle_bytes( $error->getMessage() );
	}
	return array(
		'name'           => $name,
		'short_open_tag' => (bool) ini_get( 'short_open_tag' ),
		'source'         => oracle_bytes( $source ),
		'tokens'         => $recorded,
		'warnings'       => $warnings,
		'parse_error'    => $parse_error,
		'literals'       => $literals,
	);
}

function oracle_short_tag_cases(): array {
	return array_values(
		array_filter(
			oracle_cases(),
			function ( array $case ): bool {
				return str_contains( $case[1], '<?' );
			}
		)
	);
}

if ( in_array( '--short-off', $argv ?? array(), true ) ) {
	$records = array();
	foreach ( oracle_short_tag_cases() as list( $name, $source ) ) {
		$records[] = oracle_record( $name, $source );
	}
	echo json_encode( $records, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES ), "\n";
	return;
}

$records = array();
foreach ( oracle_cases() as list( $name, $source ) ) {
	$records[] = oracle_record( $name, $source );
}

$process = proc_open( array( PHP_BINARY, '-d', 'short_open_tag=0', __FILE__, '--short-off' ), array( 1 => array( 'pipe', 'w' ), 2 => array( 'pipe', 'w' ) ), $pipes );
if ( false === $process ) {
	throw new RuntimeException( 'cannot start a child process' );
}
$output = stream_get_contents( $pipes[1] );
$errors = stream_get_contents( $pipes[2] );
fclose( $pipes[1] );
fclose( $pipes[2] );
$status = proc_close( $process );
if ( 0 !== $status || '' !== $errors ) {
	throw new RuntimeException( "short_open_tag=0 child failed ($status): $errors" );
}
$records = array_merge( $records, json_decode( $output, true, 512, JSON_THROW_ON_ERROR ) );

echo json_encode(
	array(
		'schema'      => ORACLE_SCHEMA,
		'php_version' => PHP_VERSION,
		'cases'       => $records,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
), "\n";
