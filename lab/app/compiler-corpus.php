<?php
/*
 * Ferropress lab compiler corpus: prints what the reference PHP reports while compiling every source
 * file of the upstream checkouts in lint mode, one JSON line per file.
 *
 * The files are the lexer corpus's: every `.php` file under the platform checkout and the FILE
 * section of every `.phpt` test under the PHP checkout. Each text is written to a temporary file and
 * compiled with `php -l`, many files per process, exactly as the compiler oracle does. A line holds
 * the file's path below the upstream root, the SHA-256 of the compiled text and the diagnostics
 * compiling it reported, in order: severity, message (with the temporary file's name replaced by
 * `%file%`) and line. The sources themselves are never copied into the repository.
 *
 * Run by `cargo xtask lab compiler-corpus` with the upstream directory mounted read-only; never part
 * of Ferropress.
 */

declare(strict_types=1);

require __DIR__ . '/lexer-corpus.php';
require __DIR__ . '/compiler-oracle.php';

/** Lints one batch of path => text and prints its lines. */
function corpus_compile_batch( string $dir, array $batch ): void {
	$files = array();
	foreach ( array_keys( $batch ) as $index => $path ) {
		$file = sprintf( 'case-%05d.php', $index );
		file_put_contents( $dir . '/' . $file, $batch[ $path ] );
		$files[ $path ] = $file;
	}
	$found = compiler_lint_files( $dir, array_values( $files ) );
	foreach ( $files as $path => $file ) {
		echo json_encode(
			array(
				'path'        => corpus_bytes( (string) $path ),
				'sha256'      => hash( 'sha256', $batch[ $path ] ),
				'diagnostics' => $found[ $file ] ?? array(),
			),
			JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES
		), "\n";
		unlink( $dir . '/' . $file );
	}
}

ini_set( 'memory_limit', '1G' );

$dir = sys_get_temp_dir() . '/compiler-corpus-' . getmypid();
if ( ! is_dir( $dir ) && ! mkdir( $dir ) ) {
	throw new RuntimeException( 'cannot create ' . $dir );
}
$batch = array();
foreach ( corpus_sources() as $path => $source ) {
	$batch[ $path ] = $source;
	if ( count( $batch ) === COMPILER_BATCH ) {
		corpus_compile_batch( $dir, $batch );
		$batch = array();
	}
}
if ( array() !== $batch ) {
	corpus_compile_batch( $dir, $batch );
}
rmdir( $dir );
