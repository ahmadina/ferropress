<?php
/**
 * Lab-only oracle instrumentation, loaded from the site config before the platform boots.
 *
 * Callbacks are pre-registered through the `$wp_filter` global, which the platform turns into hooks as
 * soon as its plugin API loads. The trace therefore starts at the very first hook, and nothing of ours
 * appears in the plugin lifecycle being recorded.
 *
 * Outgoing mail is never delivered. For requests carrying the X-Ferropress-Capture header, the ordered
 * hook trace, the files loaded between hooks, SQL statements, outgoing mail and the cron schedule are
 * written as JSON for `cargo xtask lab capture`.
 */

( static function (): void {
	$state = array(
		'recording'  => false,
		'hooks'      => array(),
		'files'      => array(),
		'seen_files' => 0,
		'mail'       => array(),
	);

	$register = static function ( string $hook, callable $callback, int $priority, int $accepted_args ): void {
		$GLOBALS['wp_filter'][ $hook ][ $priority ][] = array(
			'function'      => $callback,
			'accepted_args' => $accepted_args,
		);
	};

	// Every message is accepted and, while recording, kept for the capture.
	$register(
		'pre_wp_mail',
		static function ( $short_circuit, $atts ) use ( &$state ) {
			if ( null !== $short_circuit ) {
				return $short_circuit;
			}
			if ( $state['recording'] ) {
				$as_list         = static function ( $value ): array {
					if ( is_array( $value ) ) {
						return array_values( array_map( 'strval', $value ) );
					}
					return array_values( array_filter( array_map( 'trim', explode( "\n", (string) $value ) ), 'strlen' ) );
				};
				$state['mail'][] = array(
					'to'          => $as_list( $atts['to'] ?? array() ),
					'subject'     => (string) ( $atts['subject'] ?? '' ),
					'message'     => (string) ( $atts['message'] ?? '' ),
					'headers'     => $as_list( $atts['headers'] ?? array() ),
					'attachments' => array_map( 'basename', $as_list( $atts['attachments'] ?? array() ) ),
				);
			}
			return true;
		},
		PHP_INT_MAX,
		2
	);

	$id = $_SERVER['HTTP_X_FERROPRESS_CAPTURE'] ?? '';
	if ( ! is_string( $id ) || 1 !== preg_match( '/\A[a-z0-9][a-z0-9-]{0,63}\z/', $id ) ) {
		return;
	}
	$state['recording'] = true;

	// Files outside the installation root (this instrumentation) are never part of the trace.
	$record_files = static function () use ( &$state ): void {
		if ( ! defined( 'ABSPATH' ) ) {
			return;
		}
		$included = get_included_files();
		$count    = count( $included );
		for ( $i = $state['seen_files']; $i < $count; $i++ ) {
			if ( str_starts_with( $included[ $i ], ABSPATH ) ) {
				$state['files'][] = array(
					'path'        => substr( $included[ $i ], strlen( ABSPATH ) ),
					'before_hook' => count( $state['hooks'] ),
				);
			}
		}
		$state['seen_files'] = $count;
	};

	// Actions bump their run counter before the catch-all hook fires; filters never do.
	$action_runs = array();
	$register(
		'all',
		static function ( ...$args ) use ( &$state, &$action_runs, $record_files ): void {
			if ( ! $state['recording'] ) {
				return;
			}
			$record_files();
			$name   = (string) $args[0];
			$runs   = $GLOBALS['wp_actions'][ $name ] ?? 0;
			$action = $runs > ( $action_runs[ $name ] ?? 0 );
			if ( $action ) {
				$action_runs[ $name ] = $runs;
			}
			$state['hooks'][] = array(
				'kind'  => $action ? 'action' : 'filter',
				'name'  => $name,
				'args'  => count( $args ) - 1,
				'depth' => count( $GLOBALS['wp_current_filter'] ?? array() ),
			);
		},
		10,
		1
	);

	$write = static function () use ( $id, &$state, $record_files ): void {
		$record_files();
		$state['recording'] = false;

		$queries = array();
		if ( isset( $GLOBALS['wpdb'] ) && is_array( $GLOBALS['wpdb']->queries ?? null ) ) {
			foreach ( $GLOBALS['wpdb']->queries as $query ) {
				$queries[] = (string) $query[0];
			}
		}

		$cron = array();
		$jobs = function_exists( '_get_cron_array' ) ? _get_cron_array() : array();
		foreach ( is_array( $jobs ) ? $jobs : array() as $hooks ) {
			foreach ( $hooks as $hook => $events ) {
				foreach ( $events as $event ) {
					$cron[] = array(
						'hook'     => (string) $hook,
						'schedule' => false === $event['schedule'] ? null : (string) $event['schedule'],
						'interval' => isset( $event['interval'] ) ? (int) $event['interval'] : null,
						'args'     => array_values( (array) $event['args'] ),
					);
				}
			}
		}
		// Run times are wall-clock dependent; order by content so captures are comparable.
		usort(
			$cron,
			static fn ( array $a, array $b ): int => strcmp( json_encode( $a ), json_encode( $b ) )
		);

		$capture = array(
			'schema'  => 2,
			'hooks'   => $state['hooks'],
			'files'   => $state['files'],
			'queries' => $queries,
			'mail'    => $state['mail'],
			'cron'    => $cron,
		);
		$dir     = sys_get_temp_dir() . '/ferropress-capture';
		if ( ! is_dir( $dir ) ) {
			mkdir( $dir, 0700, true );
		}
		file_put_contents(
			$dir . '/' . $id . '.json',
			json_encode( $capture, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE | JSON_THROW_ON_ERROR ),
			LOCK_EX
		);
	};

	// Shutdown functions run in registration order and this one is registered before the platform's own,
	// so the writer re-registers itself to run after the shutdown hooks.
	register_shutdown_function(
		static function () use ( $write ): void {
			register_shutdown_function( $write );
		}
	);
} )();
