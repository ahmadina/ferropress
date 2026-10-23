<?php
/**
 * Plugin Name: Ferropress Lab Capture
 * Description: Lab-only oracle instrumentation. For requests carrying the X-Ferropress-Capture header it
 *              records the ordered hook trace, SQL statements, outgoing mail and the cron schedule, and
 *              writes them as JSON for `cargo xtask lab capture`. Outgoing mail is never delivered.
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

( static function (): void {
	$state = array(
		'recording' => false,
		'hooks'     => array(),
		'mail'      => array(),
	);

	// The lab never delivers mail: every message is accepted and, while recording, kept for the capture.
	add_filter(
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

	// Actions bump their run counter before the catch-all hook fires; filters never do.
	$action_runs = array();
	add_action(
		'all',
		static function ( ...$args ) use ( &$state, &$action_runs ): void {
			if ( ! $state['recording'] ) {
				return;
			}
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
		}
	);

	register_shutdown_function(
		static function () use ( $id, &$state ): void {
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
				'schema'  => 1,
				'hooks'   => $state['hooks'],
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
		}
	);
} )();
