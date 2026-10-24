<?php
/*
 * Ferropress lab operator oracle: prints what the reference PHP's operators and conversions return,
 * as JSON.
 *
 * Every unary operation is applied to every operand of a fixed grid, and every binary operator to
 * every ordered pair of grid operands. Each case records the result, every warning, notice or
 * deprecation raised (in order), and the error thrown, if any.
 *
 * Values are tagged lists: ["null"], ["bool", b], ["int", n], ["float", "<16 hex digits of the
 * IEEE 754 bits>"], ["str", s] with one character per byte, ["array", [[key, value], ...]].
 *
 * Run by `cargo xtask lab operators`; never part of Ferropress.
 */

declare(strict_types=0);

const ORACLE_SCHEMA = 1;

/** The operands every operation is applied to. */
function operator_operands(): array {
	return array(
		null, false, true,
		0, 1, -1, 2, 3, 7, -7, 255, PHP_INT_MAX, PHP_INT_MIN,
		0.0, -0.0, 0.5, 1.5, -1.5, 2.5, 0.1, 1e15, 1e100, -1e100, 9.2233720368547758E18, 1e-10,
		INF, -INF, NAN,
		'', '0', '1', '-1', '7', ' 1', '1 ', "\t\n1\r\v\f", '1abc', 'abc', '1.5', '-1.5e3', '1e3', '.5', '5.',
		'0x1A', '012', '-0', '+1', '9223372036854775807', '9223372036854775808', '1e1000', 'INF', ' ',
		"1\0", 'a', 'z', 'Z', 'Az', 'zz', 'a9', 'A', '00',
		array(), array( 1 ), array( 1, 2 ), array( 'a' => 1 ), array( 1 => 1, 0 => 2 ),
	);
}

/** Unary operations by name. */
function operator_unary(): array {
	return array(
		'neg'       => static fn ( $a ) => -$a,
		'plus'      => static fn ( $a ) => +$a,
		'bitnot'    => static fn ( $a ) => ~$a,
		'not'       => static fn ( $a ) => ! $a,
		'int'       => static fn ( $a ) => (int) $a,
		'float'     => static fn ( $a ) => (float) $a,
		'string'    => static fn ( $a ) => (string) $a,
		'bool'      => static fn ( $a ) => (bool) $a,
		'array'     => static fn ( $a ) => (array) $a,
		'preinc'    => static function ( $a ) {
			return ++$a;
		},
		'predec'    => static function ( $a ) {
			return --$a;
		},
	);
}

/** Binary operators by name. */
function operator_binary(): array {
	return array(
		'add'       => static fn ( $a, $b ) => $a + $b,
		'sub'       => static fn ( $a, $b ) => $a - $b,
		'mul'       => static fn ( $a, $b ) => $a * $b,
		'div'       => static fn ( $a, $b ) => $a / $b,
		'mod'       => static fn ( $a, $b ) => $a % $b,
		'pow'       => static fn ( $a, $b ) => $a ** $b,
		'shl'       => static fn ( $a, $b ) => $a << $b,
		'shr'       => static fn ( $a, $b ) => $a >> $b,
		'bitor'     => static fn ( $a, $b ) => $a | $b,
		'bitand'    => static fn ( $a, $b ) => $a & $b,
		'bitxor'    => static fn ( $a, $b ) => $a ^ $b,
		'concat'    => static fn ( $a, $b ) => $a . $b,
		'eq'        => static fn ( $a, $b ) => $a == $b,
		'identical' => static fn ( $a, $b ) => $a === $b,
		'lt'        => static fn ( $a, $b ) => $a < $b,
		'le'        => static fn ( $a, $b ) => $a <= $b,
		'spaceship' => static fn ( $a, $b ) => $a <=> $b,
	);
}

/** A string as JSON-safe text, one character per byte. */
function operator_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

function operator_encode( mixed $value ): array {
	if ( null === $value ) {
		return array( 'null' );
	}
	if ( is_bool( $value ) ) {
		return array( 'bool', $value );
	}
	if ( is_int( $value ) ) {
		return array( 'int', $value );
	}
	if ( is_float( $value ) ) {
		return array( 'float', bin2hex( pack( 'E', $value ) ) );
	}
	if ( is_string( $value ) ) {
		return array( 'str', operator_bytes( $value ) );
	}
	if ( is_array( $value ) ) {
		$entries = array();
		foreach ( $value as $key => $item ) {
			$entries[] = array( is_int( $key ) ? $key : operator_bytes( $key ), operator_encode( $item ) );
		}
		return array( 'array', $entries );
	}
	return array( 'other', get_debug_type( $value ) );
}

/** The index of `$entry` in `$table`, added when new. */
function operator_intern( array &$table, array &$index, array $entry ): int {
	$key = serialize( $entry );
	if ( ! isset( $index[ $key ] ) ) {
		$index[ $key ] = count( $table );
		$table[]       = $entry;
	}
	return $index[ $key ];
}

/**
 * Runs `$operation` and records `[result, [message index, ...], error index]`; the result is null
 * when an error was thrown, the error index null when none was.
 */
function operator_run( callable $operation, array $operands, array &$tables ): array {
	$raised = array();
	set_error_handler(
		static function ( int $level, string $message ) use ( &$raised ): bool {
			$names    = array(
				E_WARNING    => 'Warning',
				E_NOTICE     => 'Notice',
				E_DEPRECATED => 'Deprecated',
			);
			$raised[] = array( $names[ $level ] ?? (string) $level, operator_bytes( $message ) );
			return true;
		}
	);
	$result = null;
	$error  = null;
	try {
		$result = operator_encode( $operation( ...$operands ) );
	} catch ( Throwable $thrown ) {
		$error = array( get_class( $thrown ), operator_bytes( $thrown->getMessage() ) );
	}
	restore_error_handler();
	$messages = array();
	foreach ( $raised as $message ) {
		$messages[] = operator_intern( $tables['messages'], $tables['message_index'], $message );
	}
	if ( null !== $error ) {
		$error = operator_intern( $tables['errors'], $tables['error_index'], $error );
	}
	return array( $result, $messages, $error );
}

ini_set( 'memory_limit', '1G' );
ini_set( 'precision', '14' );
ini_set( 'serialize_precision', '-1' );

$operands = operator_operands();
$tables   = array(
	'messages'      => array(),
	'message_index' => array(),
	'errors'        => array(),
	'error_index'   => array(),
);
$lines    = array();
foreach ( operator_unary() as $name => $operation ) {
	$results = array();
	foreach ( $operands as $operand ) {
		$results[] = operator_run( $operation, array( $operand ), $tables );
	}
	$lines[] = array( 'op' => $name, 'results' => $results );
}
foreach ( operator_binary() as $name => $operation ) {
	foreach ( $operands as $left_index => $left ) {
		$results = array();
		foreach ( $operands as $right ) {
			$results[] = operator_run( $operation, array( $left, $right ), $tables );
		}
		$lines[] = array( 'op' => $name, 'left' => $left_index, 'results' => $results );
	}
}

$flags = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
echo json_encode(
	array(
		'schema'      => ORACLE_SCHEMA,
		'php_version' => PHP_VERSION,
		'operands'    => array_map( 'operator_encode', $operands ),
		'messages'    => $tables['messages'],
		'errors'      => $tables['errors'],
	),
	$flags
), "\n";
foreach ( $lines as $line ) {
	echo json_encode( $line, $flags ), "\n";
}
