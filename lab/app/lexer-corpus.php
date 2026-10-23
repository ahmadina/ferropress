<?php
/*
 * Ferropress lab lexer corpus: prints a digest of how the reference PHP tokenizes every source file
 * of the upstream checkouts, one JSON line per file.
 *
 * Every `.php` file under the platform checkout and the FILE section of every `.phpt` test under the
 * PHP checkout is tokenized. A line holds the file's path below the upstream root, the SHA-256 of the
 * tokenized text, the token count, the SHA-256 of the token list (one "name TAB length TAB line"
 * line per token, the character itself as the name of a one-character token) and the warnings. The
 * sources themselves are never copied anywhere.
 *
 * Run by `cargo xtask lab lexer-corpus` with the upstream directory mounted read-only; never part of
 * Ferropress.
 */

declare(strict_types=1);

const UPSTREAM = '/opt/upstream';

/** The FILE section of a test: the lines after "--FILE--" up to the next section marker line. */
function corpus_file_section( string $text ): ?string {
	$lines = explode( "\n", $text );
	$start = null;
	foreach ( $lines as $index => $line ) {
		if ( null === $start ) {
			if ( '--FILE--' === rtrim( $line, "\r" ) ) {
				$start = $index + 1;
			}
			continue;
		}
		if ( 1 === preg_match( '/^--[A-Z_]+--\r?$/', $line ) ) {
			$section = '';
			foreach ( array_slice( $lines, $start, $index - $start ) as $kept ) {
				$section .= $kept . "\n";
			}
			return $section;
		}
	}
	return null === $start ? null : implode( "\n", array_slice( $lines, $start ) );
}

/** Paths below UPSTREAM of the files with $extension under $directory, sorted. */
function corpus_files( string $directory, string $extension ): array {
	$files    = array();
	$iterator = new RecursiveIteratorIterator( new RecursiveDirectoryIterator( UPSTREAM . '/' . $directory, FilesystemIterator::SKIP_DOTS ) );
	foreach ( $iterator as $file ) {
		if ( $file->isFile() && ! $file->isLink() && str_ends_with( $file->getFilename(), $extension ) ) {
			$files[] = substr( $file->getPathname(), strlen( UPSTREAM ) + 1 );
		}
	}
	sort( $files, SORT_STRING );
	return $files;
}

function corpus_bytes( string $bytes ): string {
	return mb_convert_encoding( $bytes, 'UTF-8', 'ISO-8859-1' );
}

/** Tokens and the warnings raised; scanner warnings are compile warnings, read back from the output. */
function corpus_tokenize( string $source ): array {
	ob_start();
	try {
		$tokens = PhpToken::tokenize( $source );
	} finally {
		$output = (string) ob_get_clean();
	}
	preg_match_all( '/^(?:Warning|Deprecated|Notice): (.*?) in .* on line \d+$/m', $output, $matches );
	return array( $tokens, array_map( 'corpus_bytes', $matches[1] ) );
}

/** Every source to read, path => text: platform files whole, PHP tests through their FILE section. */
function corpus_sources(): Generator {
	$sources = array();
	foreach ( corpus_files( 'platform', '.php' ) as $path ) {
		$sources[ $path ] = false;
	}
	foreach ( corpus_files( 'php', '.phpt' ) as $path ) {
		$sources[ $path ] = true;
	}
	foreach ( $sources as $path => $is_test ) {
		$text   = (string) file_get_contents( UPSTREAM . '/' . $path );
		$source = $is_test ? corpus_file_section( $text ) : $text;
		if ( null !== $source ) {
			yield (string) $path => $source;
		}
	}
}

if ( realpath( (string) ( $_SERVER['SCRIPT_FILENAME'] ?? '' ) ) !== __FILE__ ) {
	return;
}

ini_set( 'display_errors', 'stdout' );
ini_set( 'html_errors', '0' );
ini_set( 'memory_limit', '1G' );

foreach ( corpus_sources() as $path => $source ) {
	list( $tokens, $warnings ) = corpus_tokenize( $source );
	$digest = hash_init( 'sha256' );
	foreach ( $tokens as $token ) {
		$name = $token->id < 256 ? chr( $token->id ) : token_name( $token->id );
		hash_update( $digest, $name . "\t" . strlen( $token->text ) . "\t" . $token->line . "\n" );
	}
	echo json_encode(
		array(
			'path'     => corpus_bytes( (string) $path ),
			'sha256'   => hash( 'sha256', $source ),
			'tokens'   => count( $tokens ),
			'digest'   => hash_final( $digest ),
			'warnings' => $warnings,
		),
		JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
	), "\n";
}
