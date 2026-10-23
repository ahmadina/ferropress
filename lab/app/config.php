<?php
/**
 * Lab-only site settings, required by the generated site config before the platform boots. Every query
 * is recorded for captures and cron never spawns background requests, keeping runs deterministic.
 *
 * One database holds two installations, selected by the LAB_PROFILE server variable (set per port by
 * Apache, or by the lab's CLI scripts): `single`, one site with the default table prefix, and `network`,
 * a subdirectory network of sites with its own table prefix.
 */

define( 'SAVEQUERIES', true );
define( 'DISABLE_WP_CRON', true );

( static function (): void {
	$profile = $_SERVER['LAB_PROFILE'] ?? null;
	if ( 'single' === $profile ) {
		return;
	}
	if ( 'network' !== $profile ) {
		throw new RuntimeException( 'LAB_PROFILE must be `single` or `network`' );
	}
	$GLOBALS['table_prefix'] = 'net_';
	define( 'WP_ALLOW_MULTISITE', true );
	// The installer converts the installed site into a network with multisite still off.
	if ( defined( 'LAB_CONVERTING_TO_NETWORK' ) ) {
		return;
	}
	define( 'MULTISITE', true );
	define( 'SUBDOMAIN_INSTALL', false );
	define( 'DOMAIN_CURRENT_SITE', '127.0.0.1:8081' );
	define( 'PATH_CURRENT_SITE', '/' );
	define( 'SITE_ID_CURRENT_SITE', 1 );
	define( 'BLOG_ID_CURRENT_SITE', 1 );
} )();

require __DIR__ . '/instrument.php';
