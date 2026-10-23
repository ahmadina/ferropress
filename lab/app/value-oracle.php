<?php
/*
 * Ferropress lab value-model oracle: prints how the reference PHP stores values, as JSON.
 *
 * Each scenario is a list of operations on a few variables (assignments, reference bindings, unsets,
 * property writes, copies) followed by dumps of every variable. Scenarios are hand-written or generated
 * from a fixed seed. Every scenario runs in a fresh PHP process, so object ids start at 1, and records
 * each dump, thrown error and emitted message in order.
 *
 * A dump shows key order, key types, values, object ids and classes, and which array elements are bound
 * to a reference that has another binding (numbered by first appearance within the dump). An object or
 * reference met again inside itself is shown as a recursion marker instead of being expanded, and containers
 * nested deeper than DUMP_DEPTH as a depth marker.
 *
 * Run by `cargo xtask lab value`; never part of Ferropress.
 */

declare(strict_types=1);

const ORACLE_SCHEMA = 1;
const RANDOM_SEED = 20261023;
const RANDOM_SCENARIOS = 600;
const VARIABLES = array( 'a', 'b', 'c' );
const RANDOM_KEYS = array( 0, 1, 2, -1, 'x', '5', '05' );
const RANDOM_PROPERTIES = array( 'p', 'q' );
/** Containers nested deeper than this are shown as a depth marker, so cycles stay finite. */
const DUMP_DEPTH = 8;

/** Hand-written scenarios; final dumps of every variable are added to each. */
function oracle_scenarios(): array {
	$keys = array( '0', '-0', '00', '01', ' 1', '1 ', '9223372036854775807', '-9223372036854775808', '9223372036854775808', '+1', '1e3', '', '-1', '-01', '1.0' );
	$normalization = array();
	foreach ( $keys as $key ) {
		$normalization[] = array( 'assign', 'a', array( $key ), array( 'int', 1 ) );
	}
	return array(
		$normalization,
		array( array( 'assign', 'a', array( -5 ), array( 'int', 1 ) ), array( 'assign', 'a', array( null ), array( 'int', 2 ) ) ),
		array( array( 'assign', 'a', array( -5 ), array( 'int', 1 ) ), array( 'assign', 'a', array( -10 ), array( 'int', 1 ) ), array( 'assign', 'a', array( null ), array( 'int', 2 ) ) ),
		array( array( 'assign', 'a', array( null ), array( 'int', 1 ) ), array( 'assign', 'a', array( null ), array( 'int', 2 ) ), array( 'unset', 'a', array( 1 ) ), array( 'assign', 'b', array(), array( 'copy', 'a', array() ) ), array( 'assign', 'b', array( null ), array( 'int', 3 ) ) ),
		array( array( 'assign', 'a', array( '5' ), array( 'int', 1 ) ), array( 'assign', 'a', array( null ), array( 'int', 2 ) ), array( 'assign', 'a', array( 'x' ), array( 'int', 3 ) ) ),
		array( array( 'assign', 'a', array( 9223372036854775807 ), array( 'int', 1 ) ), array( 'assign', 'a', array( null ), array( 'int', 2 ) ), array( 'assign', 'a', array( 0 ), array( 'int', 3 ) ), array( 'assign', 'a', array( null ), array( 'int', 4 ) ) ),
		array( array( 'assign', 'a', array( 'a' ), array( 'int', 1 ) ), array( 'assign', 'a', array( 'b' ), array( 'int', 2 ) ), array( 'assign', 'a', array( 'c' ), array( 'int', 3 ) ), array( 'assign', 'a', array( 'a' ), array( 'int', 9 ) ), array( 'unset', 'a', array( 'b' ) ), array( 'assign', 'a', array( 'b' ), array( 'int', 5 ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'int', 1 ) ), array( 'bind', 'b', array(), 'a', array( 0 ) ), array( 'assign', 'c', array(), array( 'copy', 'a', array() ) ), array( 'assign', 'c', array( 0 ), array( 'int', 2 ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'int', 1 ) ), array( 'bind', 'b', array(), 'a', array( 0 ) ), array( 'unset', 'b', array() ), array( 'assign', 'c', array(), array( 'copy', 'a', array() ) ), array( 'assign', 'c', array( 0 ), array( 'int', 2 ) ) ),
		array( array( 'assign', 'a', array( 0, 0 ), array( 'int', 1 ) ), array( 'bind', 'b', array(), 'a', array( 0, 0 ) ), array( 'assign', 'c', array(), array( 'copy', 'a', array() ) ), array( 'assign', 'c', array( 0, 0 ), array( 'int', 2 ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'int', 1 ) ), array( 'bind', 'a', array( 1 ), 'a', array( 0 ) ), array( 'assign', 'b', array(), array( 'copy', 'a', array() ) ), array( 'assign', 'b', array( 0 ), array( 'int', 5 ) ) ),
		array( array( 'assign', 'a', array(), array( 'new' ) ), array( 'assign', 'b', array( 0 ), array( 'copy', 'a', array() ) ), array( 'assign', 'c', array(), array( 'copy', 'b', array() ) ), array( 'assign', 'c', array( 0, array( 'prop', 'x' ) ), array( 'int', 1 ) ) ),
		array( array( 'bind', 'b', array(), 'a', array( 'k' ) ), array( 'bind', 'c', array(), 'a', array( null ) ) ),
		array( array( 'assign', 'a', array(), array( 'null' ) ), array( 'assign', 'a', array( 1 ), array( 'int', 2 ) ), array( 'assign', 'b', array( 'a', 'b', null ), array( 'int', 1 ) ) ),
		array( array( 'assign', 'a', array(), array( 'int', 5 ) ), array( 'assign', 'a', array( 1 ), array( 'int', 2 ) ), array( 'assign', 'b', array(), array( 'new' ) ), array( 'assign', 'b', array( 0 ), array( 'int', 2 ) ), array( 'unset', 'a', array( 0 ) ), array( 'unset', 'b', array( 0 ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'int', 1 ) ), array( 'unset', 'a', array( 5 ) ), array( 'unset', 'a', array( 'x', 'y' ) ), array( 'unset', 'c', array( 0 ) ) ),
		array( array( 'assign', 'a', array(), array( 'new' ) ), array( 'assign', 'a', array( array( 'prop', '5' ) ), array( 'int', 1 ) ), array( 'assign', 'a', array( array( 'prop', 'b' ) ), array( 'int', 2 ) ), array( 'assign', 'a', array( array( 'prop', 'a' ) ), array( 'int', 3 ) ), array( 'unset', 'a', array( array( 'prop', 'b' ) ) ), array( 'assign', 'a', array( array( 'prop', 'b' ) ), array( 'int', 4 ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'new' ) ), array( 'assign', 'a', array( 1 ), array( 'new' ) ), array( 'assign', 'a', array( 2 ), array( 'new' ) ), array( 'unset', 'a', array() ), array( 'assign', 'b', array( 0 ), array( 'new' ) ), array( 'assign', 'b', array( 1 ), array( 'new' ) ) ),
		array( array( 'assign', 'a', array(), array( 'new' ) ), array( 'assign', 'a', array( array( 'prop', 'c' ) ), array( 'new' ) ), array( 'assign', 'a', array( array( 'prop', 'd' ) ), array( 'new' ) ), array( 'unset', 'a', array() ), array( 'assign', 'b', array( 0 ), array( 'new' ) ), array( 'assign', 'b', array( 1 ), array( 'new' ) ), array( 'assign', 'b', array( 2 ), array( 'new' ) ) ),
		array( array( 'assign', 'a', array(), array( 'new' ) ), array( 'assign', 'a', array( array( 'prop', 'self' ) ), array( 'copy', 'a', array() ) ) ),
		array( array( 'assign', 'a', array(), array( 'array' ) ), array( 'assign', 'a', array( 0 ), array( 'copy', 'a', array() ) ) ),
		array( array( 'assign', 'a', array( 'k' ), array( 'int', 1 ) ), array( 'bind', 'b', array(), 'a', array() ), array( 'assign', 'a', array( null ), array( 'copy', 'b', array() ) ) ),
		array( array( 'assign', 'a', array( 0 ), array( 'int', 1 ) ), array( 'bind', 'a', array( null ), 'a', array() ), array( 'assign', 'b', array(), array( 'copy', 'a', array() ) ), array( 'unset', 'a', array() ) ),
		array( array( 'assign', 'a', array(), array( 'str', "x\0\xff" ) ), array( 'assign', 'b', array( 'f' ), array( 'float', '3ff8000000000000' ) ), array( 'assign', 'b', array( 't' ), array( 'bool', true ) ), array( 'assign', 'c', array(), array( 'copy', 'b', array( 'missing' ) ) ), array( 'assign', 'c', array(), array( 'copy', 'z', array() ) ), array( 'assign', 'c', array(), array( 'copy', 'a', array( array( 'prop', 'p' ) ) ) ) ),
		array( array( 'assign', 'a', array(), array( 'new' ) ), array( 'assign', 'b', array(), array( 'copy', 'a', array( 0 ) ) ), array( 'assign', 'b', array(), array( 'copy', 'a', array( array( 'prop', 'p' ) ) ) ), array( 'assign', 'c', array( array( 'prop', 'x' ) ), array( 'int', 1 ) ), array( 'assign', 'c', array( 0, array( 'prop', 'x' ) ), array( 'int', 1 ) ) ),
	);
}

/** A random path into a variable: dims (`null` appends when allowed), optionally ending in a property. */
function oracle_random_path( bool $writable, bool $property ): array {
	$path = array();
	for ( $n = mt_rand( 0, 2 ); $n > 0; $n-- ) {
		$path[] = ( $writable && 0 === mt_rand( 0, 6 ) ) ? null : RANDOM_KEYS[ mt_rand( 0, count( RANDOM_KEYS ) - 1 ) ];
	}
	if ( $property && 0 === mt_rand( 0, 9 ) ) {
		$path[] = array( 'prop', RANDOM_PROPERTIES[ mt_rand( 0, count( RANDOM_PROPERTIES ) - 1 ) ] );
	}
	return $path;
}

function oracle_random_variable(): string {
	return VARIABLES[ mt_rand( 0, count( VARIABLES ) - 1 ) ];
}

function oracle_random_expression(): array {
	switch ( mt_rand( 0, 9 ) ) {
		case 0:
			return array( 'null' );
		case 1:
		case 2:
			return array( 'array' );
		case 3:
			return array( 'new' );
		case 4:
		case 5:
			return array( 'copy', oracle_random_variable(), oracle_random_path( false, true ) );
		default:
			return array( 'int', mt_rand( 0, 9 ) );
	}
}

function oracle_random_scenario(): array {
	$ops = array();
	for ( $n = mt_rand( 3, 12 ); $n > 0; $n-- ) {
		$choice = mt_rand( 0, 19 );
		if ( $choice < 10 ) {
			$path       = oracle_random_path( true, true );
			$expression = oracle_random_expression();
			if ( array() !== $path && 'copy' === $expression[0] && array() === $expression[2] ) {
				// Avoids `$x[…] = $y` with $y bound to the same reference as $x, which makes an array contain
				// itself; that case is recorded by a hand-written scenario.
				$expression[2] = array( RANDOM_KEYS[ mt_rand( 0, count( RANDOM_KEYS ) - 1 ) ] );
			}
			$ops[] = array( 'assign', oracle_random_variable(), $path, $expression );
		} elseif ( $choice < 14 ) {
			$ops[] = array( 'bind', oracle_random_variable(), oracle_random_path( true, false ), oracle_random_variable(), oracle_random_path( true, false ) );
		} elseif ( $choice < 18 ) {
			$ops[] = array( 'unset', oracle_random_variable(), oracle_random_path( false, true ) );
		} else {
			$ops[] = array( 'dump', oracle_random_variable() );
		}
	}
	return $ops;
}

/* ---- Child process: compile one scenario to PHP and run it. ---- */

final class OracleEvents {
	public static array $list = array();
	public static array $refs = array();
}

function oracle_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

function oracle_literal( $value ): string {
	if ( is_int( $value ) ) {
		return PHP_INT_MIN === $value ? 'PHP_INT_MIN' : (string) $value;
	}
	return var_export( $value, true );
}

function oracle_path_code( array $path ): string {
	$code = '';
	foreach ( $path as $element ) {
		if ( null === $element ) {
			$code .= '[]';
		} elseif ( is_array( $element ) ) {
			$code .= '->{' . oracle_literal( $element[1] ) . '}';
		} else {
			$code .= '[' . oracle_literal( $element ) . ']';
		}
	}
	return $code;
}

function oracle_expression_code( array $expression ): string {
	switch ( $expression[0] ) {
		case 'null':
			return 'null';
		case 'bool':
			return $expression[1] ? 'true' : 'false';
		case 'int':
			return oracle_literal( $expression[1] );
		case 'float':
			return 'unpack("E", hex2bin(' . oracle_literal( $expression[1] ) . '))[1]';
		case 'str':
			return 'base64_decode(' . oracle_literal( base64_encode( $expression[1] ) ) . ')';
		case 'array':
			return '[]';
		case 'new':
			return 'new stdClass()';
		case 'copy':
			return '$v_' . $expression[1] . oracle_path_code( $expression[2] );
	}
	throw new LogicException( 'unknown expression ' . $expression[0] );
}

function oracle_statement_code( array $op ): string {
	switch ( $op[0] ) {
		case 'assign':
			return '$v_' . $op[1] . oracle_path_code( $op[2] ) . ' = ' . oracle_expression_code( $op[3] ) . ';';
		case 'bind':
			return '$v_' . $op[1] . oracle_path_code( $op[2] ) . ' = &$v_' . $op[3] . oracle_path_code( $op[4] ) . ';';
		case 'unset':
			return 'unset($v_' . $op[1] . oracle_path_code( $op[2] ) . ');';
		case 'dump':
			return 'OracleEvents::$list[] = oracle_dump(' . oracle_literal( $op[1] ) . ', get_defined_vars());';
	}
	throw new LogicException( 'unknown operation ' . $op[0] );
}

function oracle_dump( string $name, array $variables ): array {
	OracleEvents::$refs = array();
	$value = array_key_exists( "v_$name", $variables ) ? oracle_value( $variables[ "v_$name" ], array(), 0 ) : null;
	return array( 'dump', $name, $value );
}

function oracle_value( $value, array $open, int $depth ): array {
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
		return array( 'string', oracle_bytes( $value ) );
	}
	if ( ( is_array( $value ) || is_object( $value ) ) && $depth >= DUMP_DEPTH ) {
		return array( 'deep' );
	}
	if ( is_array( $value ) ) {
		$entries = array();
		foreach ( $value as $key => $element ) {
			$name      = is_int( $key ) ? $key : oracle_bytes( $key );
			$reference = ReflectionReference::fromArrayElement( $value, $key );
			if ( null === $reference ) {
				$entries[] = array( $name, oracle_value( $element, $open, $depth + 1 ) );
				continue;
			}
			$id = $reference->getId();
			if ( ! isset( OracleEvents::$refs[ $id ] ) ) {
				OracleEvents::$refs[ $id ] = count( OracleEvents::$refs ) + 1;
			}
			$number    = OracleEvents::$refs[ $id ];
			$inner     = $open + array( "r$id" => true );
			$entries[] = array( $name, isset( $open[ "r$id" ] ) ? array( 'recursion' ) : oracle_value( $element, $inner, $depth + 1 ), $number );
		}
		return array( 'array', $entries );
	}
	$id = spl_object_id( $value );
	if ( isset( $open[ "o$id" ] ) ) {
		return array( 'object', $id );
	}
	$open[ "o$id" ] = true;
	$properties  = array();
	foreach ( $value as $name => $property ) {
		$properties[] = array( oracle_bytes( (string) $name ), oracle_value( $property, $open, $depth + 1 ) );
	}
	return array( 'object', $id, oracle_bytes( get_class( $value ) ), $properties );
}

function oracle_run_child(): void {
	$ops  = oracle_ops_from_text( json_decode( stream_get_contents( STDIN ), true, 512, JSON_THROW_ON_ERROR ) );
	$code = '';
	foreach ( $ops as $op ) {
		$code .= 'try { ' . oracle_statement_code( $op ) . " } catch (\\Throwable \$e) { OracleEvents::\$list[] = array('error', get_class(\$e), oracle_bytes(\$e->getMessage())); }\n";
	}
	set_error_handler(
		static function ( int $level, string $message ): bool {
			OracleEvents::$list[] = array( E_DEPRECATED === $level ? 'deprecated' : ( E_WARNING === $level ? 'warning' : "level-$level" ), oracle_bytes( $message ) );
			return true;
		}
	);
	( static function ( string $oracle_code ): void {
		eval( $oracle_code );
	} )( $code );
	echo json_encode( OracleEvents::$list, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES ), "\n";
}

/* ---- Parent process: run every scenario in a fresh child. ---- */

function oracle_run_scenario( array $ops ): array {
	foreach ( VARIABLES as $variable ) {
		$ops[] = array( 'dump', $variable );
	}
	$process = proc_open( array( PHP_BINARY, __FILE__, '--child' ), array( 0 => array( 'pipe', 'r' ), 1 => array( 'pipe', 'w' ), 2 => array( 'pipe', 'w' ) ), $pipes );
	if ( false === $process ) {
		throw new RuntimeException( 'cannot start a child process' );
	}
	$ops = oracle_ops_bytes( $ops );
	fwrite( $pipes[0], json_encode( $ops, JSON_THROW_ON_ERROR ) );
	fclose( $pipes[0] );
	$output = stream_get_contents( $pipes[1] );
	$errors = stream_get_contents( $pipes[2] );
	fclose( $pipes[1] );
	fclose( $pipes[2] );
	$status = proc_close( $process );
	if ( 0 !== $status || '' !== $errors ) {
		throw new RuntimeException( "child failed ($status) for " . json_encode( oracle_ops_bytes( $ops ) ) . ": $errors $output" );
	}
	$events = json_decode( $output, true, 512, JSON_THROW_ON_ERROR );
	return array(
		'ops'    => $ops,
		'events' => $events,
	);
}

/** Strings in operations as characters of the same code points, like every other byte string. */
function oracle_ops_bytes( $value ) {
	if ( is_string( $value ) ) {
		return oracle_bytes( $value );
	}
	return is_array( $value ) ? array_map( 'oracle_ops_bytes', $value ) : $value;
}

function oracle_ops_from_text( $value ) {
	if ( is_string( $value ) ) {
		return mb_convert_encoding( $value, 'ISO-8859-1', 'UTF-8' );
	}
	return is_array( $value ) ? array_map( 'oracle_ops_from_text', $value ) : $value;
}

if ( in_array( '--child', $argv ?? array(), true ) ) {
	oracle_run_child();
	return;
}

mt_srand( RANDOM_SEED, MT_RAND_MT19937 );
$scenarios = array();
foreach ( oracle_scenarios() as $ops ) {
	$scenarios[] = oracle_run_scenario( $ops );
}
for ( $n = 0; $n < RANDOM_SCENARIOS; $n++ ) {
	$scenarios[] = oracle_run_scenario( oracle_random_scenario() );
}
echo json_encode(
	array(
		'schema'      => ORACLE_SCHEMA,
		'php_version' => PHP_VERSION,
		'scenarios'   => $scenarios,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
), "\n";
