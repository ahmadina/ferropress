<?php
/*
 * Ferropress lab parser corpus: prints how the reference PHP parses every source file of the
 * upstream checkouts, one JSON line per file.
 *
 * The files are the lexer corpus's: every `.php` file under the platform checkout and the FILE
 * section of every `.phpt` test under the PHP checkout. A line holds the file's path below the
 * upstream root, the SHA-256 of the parsed text, the error parsing raised (class, message and line,
 * or null) and the warnings raised while parsing (severity and message). The sources themselves are
 * never copied anywhere.
 *
 * Run by `cargo xtask lab parser-corpus` with the upstream directory mounted read-only; never part
 * of Ferropress.
 */

declare(strict_types=1);

require __DIR__ . '/lexer-corpus.php';

/** The error parsing $source raises (null when it parses) and the warnings, read back from the output. */
function corpus_parse( string $source ): array {
	$error = null;
	ob_start();
	try {
		PhpToken::tokenize( $source, TOKEN_PARSE );
	} catch ( CompileError $thrown ) {
		$line  = __FILE__ === $thrown->getFile() ? null : $thrown->getLine();
		$error = array( get_class( $thrown ), corpus_bytes( $thrown->getMessage() ), $line );
	} finally {
		$output = (string) ob_get_clean();
	}
	preg_match_all( '/^(Warning|Deprecated|Notice): (.*?) in .* on line \d+$/m', $output, $matches, PREG_SET_ORDER );
	$warnings = array_map(
		function ( array $match ): array {
			return array( $match[1], corpus_bytes( $match[2] ) );
		},
		$matches
	);
	return array( $error, $warnings );
}

ini_set( 'display_errors', 'stdout' );
ini_set( 'html_errors', '0' );
ini_set( 'memory_limit', '1G' );

foreach ( corpus_sources() as $path => $source ) {
	list( $error, $warnings ) = corpus_parse( $source );
	echo json_encode(
		array(
			'path'     => corpus_bytes( $path ),
			'sha256'   => hash( 'sha256', $source ),
			'error'    => $error,
			'warnings' => $warnings,
		),
		JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
	), "\n";
}
