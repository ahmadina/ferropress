<?php
/*
 * Ferropress lab serialized-format oracle: prints how the reference PHP reads and writes serialized values, as JSON.
 *
 * Builds a corpus from hand-written seed inputs and deterministic edits of them (every truncation, byte
 * substitutions and seeded random edits), reads each input with unserialize() while no class may be instantiated,
 * and records whether it was refused, the messages emitted and what the value serializes back to. Also records
 * serialize() for selected and seeded random floats.
 *
 * Byte strings are emitted with each byte as the character of the same code point, so JSON stays readable and
 * every byte round-trips.
 *
 * Run by `cargo xtask lab serialize`; never part of Ferropress.
 */

declare(strict_types=1);

const ORACLE_SCHEMA = 1;
const RANDOM_SEED = 20260101;
const SUBSTITUTIONS_PER_BYTE = 2;
const RANDOM_EDITS_PER_SEED = 16;
const RANDOM_FLOATS = 3000;
/** Binary exponents where odd mantissas often lie exactly halfway between two shortest decimal candidates. */
const TIE_BINADES = array( 49, 50, 56, 57, 58, 59 );
const TIES_PER_BINADE = 64;
const EDIT_BYTES = array( '0', '1', '9', ';', ':', '{', '}', '"', '-', '+', '.', 'e', 'N', 'R', 'r', 'a', 'i', 's', "\0" );

/** An enum that `E:` seeds can name. */
enum FerropressFixtureSuit {
	case Hearts;
	case Spades;
}

/** Seed inputs, each with the `max_depth` option to read it with (null: the default). */
function oracle_seeds(): array {
	$plain = array(
		'N;', 'b:0;', 'b:1;', 'i:0;', 'i:-1;', 'i:+05;', 'i:9223372036854775807;', 'i:-9223372036854775808;',
		'i:99999999999999999999;', 'i:-9223372036854775809;',
		'd:0;', 'd:-0;', 'd:0.1;', 'd:1.5E+300;', 'd:-1.0E-5;', 'd:.5;', 'd:1.;', 'd:+.5e3;', 'd:NAN;', 'd:INF;',
		'd:-INF;', 'd:1e999;', 'd:2.4703282292062328e-324;', 'd:123456789012345678;',
		's:0:"";', 's:5:"hello";', "s:5:\"a\"b\0;\";", "s:3:\"\x80\xff\x01\";", 'S:3:"\61bc";', 's:18446744073709551617:"a";',
		'a:0:{}', 'a:2:{i:0;s:1:"a";i:1;s:1:"b";}', 'a:2:{s:3:"key";i:1;s:1:"5";i:2;}', 'a:3:{i:0;i:1;i:0;i:2;i:1;N;}',
		'a:1:{i:0;a:1:{i:0;a:0:{}}}', 'a:1:{i:0;R:1;}', 'a:2:{i:0;i:1;i:1;R:2;}', 'a:2:{i:0;N;i:0;R:2;}',
		'a:2:{i:-5;d:0.5;s:2:"-0";s:0:"";}', 'a:2:{i:0;i:1;s:1:"0";R:2;}',
		'a:3:{s:4:"name";s:5:"Alice";s:5:"roles";a:2:{i:0;s:6:"editor";i:1;s:6:"author";}s:6:"active";b:1;}',
		'O:8:"stdClass":0:{}', "O:8:\"stdClass\":2:{s:1:\"a\";i:1;s:4:\"\0*\0b\";b:1;}",
		"O:4:\"Shop\":1:{s:9:\"\0Shop\0sku\";s:3:\"A-1\";}", 'O:1:"A":1:{i:0;r:1;}', 'O:8:"stdClass"::{}',
		'O:8:"stdClass":+1:{s:1:"a";i:1;}', 'O:9:"Shop\\Item":0:{}', "O:5:\"Caf\xc3\xa9\":0:{}", 'O:1:"A":1:{i:0;i:0;}',
		'C:4:"Blob":3:{abc}', 'C:4:"Blob":0:{}', 'a:2:{i:0;C:1:"A":0:{}i:1;r:2;}',
		'E:28:"FerropressFixtureSuit:Hearts";', 'E:27:"FerropressFixtureSuit:Clubs";', 'E:12:"Missing:Case";',
		'E:3:"ABC";', 'a:2:{i:0;E:28:"FerropressFixtureSuit:Spades";i:1;r:2;}',
		'a:2:{i:0;O:8:"stdClass":0:{}i:1;r:2;}', 'a:3:{i:0;O:1:"A":0:{}i:1;r:2;i:2;r:3;}', 'a:2:{i:0;a:0:{}i:1;R:2;}',
		'i:5;junk', 'a:0:{}}',
	);
	$seeds = array();
	foreach ( $plain as $input ) {
		$seeds[] = array( $input, null );
	}
	$nested = str_repeat( 'a:1:{i:0;', 4 ) . 'N;' . str_repeat( '}', 4 );
	foreach ( array( 1, 3, 4, 0 ) as $depth ) {
		$seeds[] = array( $nested, $depth );
	}
	$seeds[] = array( 'a:1:{i:0;O:1:"A":0:{}}', 1 );
	return $seeds;
}

/** Every distinct input derived from one seed: the seed, its truncations, substitutions and random edits. */
function oracle_variants( string $seed ): array {
	$variants = array( $seed );
	$length = strlen( $seed );
	for ( $cut = 0; $cut < $length; $cut++ ) {
		$variants[] = substr( $seed, 0, $cut );
	}
	for ( $at = 0; $at < $length; $at++ ) {
		for ( $n = 0; $n < SUBSTITUTIONS_PER_BYTE; $n++ ) {
			$variants[] = substr_replace( $seed, oracle_edit_byte(), $at, 1 );
		}
	}
	for ( $n = 0; $n < RANDOM_EDITS_PER_SEED; $n++ ) {
		$edited = $seed;
		for ( $edits = mt_rand( 1, 3 ); $edits > 0 && '' !== $edited; $edits-- ) {
			$at = mt_rand( 0, strlen( $edited ) - 1 );
			switch ( mt_rand( 0, 2 ) ) {
				case 0:
					$edited = substr_replace( $edited, oracle_edit_byte(), $at, 1 );
					break;
				case 1:
					$edited = substr_replace( $edited, oracle_edit_byte(), $at, 0 );
					break;
				default:
					$edited = substr_replace( $edited, '', $at, 1 );
			}
		}
		$variants[] = $edited;
	}
	return $variants;
}

function oracle_edit_byte(): string {
	return 0 === mt_rand( 0, 3 ) ? chr( mt_rand( 0, 255 ) ) : EDIT_BYTES[ mt_rand( 0, count( EDIT_BYTES ) - 1 ) ];
}

/** Bytes as characters of the same code points. */
function oracle_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

/** Reads `$input` as the runtime does with no class instantiable and records the outcome. */
function oracle_decode( string $input, ?int $max_depth ): array {
	$messages = array();
	set_error_handler(
		static function ( int $level, string $message ) use ( &$messages ): bool {
			$messages[] = array( E_DEPRECATED === $level ? 'deprecated' : ( E_WARNING === $level ? 'warning' : "level-$level" ), oracle_bytes( $message ) );
			return true;
		}
	);
	$options = array( 'allowed_classes' => false );
	if ( null !== $max_depth ) {
		$options['max_depth'] = $max_depth;
	}
	$serialized = null;
	try {
		$value = unserialize( $input, $options );
		$refused = '' === $input;
		foreach ( $messages as $message ) {
			$refused = $refused || str_starts_with( $message[1], 'unserialize(): Error at offset ' );
		}
		if ( ! $refused ) {
			$serialized = oracle_bytes( serialize( $value ) );
		}
	} catch ( Throwable $error ) {
		$messages[] = array( 'exception', oracle_bytes( get_class( $error ) . ': ' . $error->getMessage() ) );
		$refused    = true;
	}
	restore_error_handler();
	$case = array( 'input' => oracle_bytes( $input ) );
	if ( null !== $max_depth ) {
		$case['max_depth'] = $max_depth;
	}
	$case['refused']  = $refused;
	$case['messages'] = $messages;
	if ( null !== $serialized ) {
		$case['serialized'] = $serialized;
	}
	return $case;
}

/** Float bit patterns: special values, powers of ten and their neighbors, notation boundaries, random bits. */
function oracle_float_bits(): array {
	$bits = array();
	$add  = static function ( float $value ) use ( &$bits ): void {
		$pattern = unpack( 'J', pack( 'E', $value ) )[1];
		foreach ( array( $pattern - 1, $pattern, $pattern + 1 ) as $near ) {
			if ( is_int( $near ) ) {
				$bits[] = $near;
			}
		}
	};
	foreach ( array( 0.0, -0.0, 1.0, -1.0, 0.1, 0.5, 1.5, 2.5, 1 / 3, M_PI, PHP_FLOAT_EPSILON, PHP_FLOAT_MIN, PHP_FLOAT_MAX, 5e-324, 9007199254740992.0, 9007199254740993.0 ) as $value ) {
		$add( $value );
	}
	for ( $exponent = -30; $exponent <= 30; $exponent++ ) {
		$add( (float) "1e$exponent" );
		$add( (float) "9.5e$exponent" );
	}
	foreach ( array( INF, -INF, NAN ) as $special ) {
		$bits[] = unpack( 'J', pack( 'E', $special ) )[1];
	}
	foreach ( TIE_BINADES as $binade ) {
		for ( $n = 0; $n < TIES_PER_BINADE; $n++ ) {
			$bits[] = ( ( 1023 + $binade ) << 52 ) | ( mt_rand( 0, 0xFFFFF ) << 32 ) | ( mt_rand( 0, 0x7FFFFFFF ) << 1 ) | 1;
		}
	}
	for ( $n = 0; $n < RANDOM_FLOATS; $n++ ) {
		$bits[] = ( mt_rand( 0, 0xFFFF ) << 48 ) | ( mt_rand( 0, 0xFFFFFF ) << 24 ) | mt_rand( 0, 0xFFFFFF );
	}
	return array_values( array_unique( $bits ) );
}

mt_srand( RANDOM_SEED, MT_RAND_MT19937 );

$decode = array();
$seen   = array();
foreach ( oracle_seeds() as list( $seed, $max_depth ) ) {
	foreach ( oracle_variants( $seed ) as $input ) {
		$key = ( $max_depth ?? 'default' ) . ':' . $input;
		if ( isset( $seen[ $key ] ) ) {
			continue;
		}
		$seen[ $key ] = true;
		$decode[]     = oracle_decode( $input, $max_depth );
	}
}

$floats = array();
foreach ( oracle_float_bits() as $pattern ) {
	$floats[] = array(
		'bits'       => sprintf( '%016x', $pattern ),
		'serialized' => serialize( unpack( 'E', pack( 'J', $pattern ) )[1] ),
	);
}

echo json_encode(
	array(
		'schema'      => ORACLE_SCHEMA,
		'php_version' => PHP_VERSION,
		'decode'      => $decode,
		'floats'      => $floats,
	),
	JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION
), "\n";
