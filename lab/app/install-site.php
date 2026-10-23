<?php
/**
 * Installs one lab profile with fixed settings. Run inside the app container by `cargo xtask lab install`
 * once per phase, in this order:
 *
 * - `single`: the single site, with the fixture plugins activated;
 * - `network`: the network's main site, converted into a network while multisite is still off;
 * - `network-sites`: with multisite on, a second site and the network-wide fixture plugin.
 *
 * Each phase does nothing if its work is already done. A phase that fails midway needs `lab reset`.
 */

const LAB_PHASES = array(
	'single'        => array( 'single', '127.0.0.1:8080' ),
	'network'       => array( 'network', '127.0.0.1:8081' ),
	'network-sites' => array( 'network', '127.0.0.1:8081' ),
);

$phase = $argv[1] ?? '';
if ( ! isset( LAB_PHASES[ $phase ] ) ) {
	fwrite( STDERR, "usage: install-site.php single|network|network-sites\n" );
	exit( 2 );
}
[ $profile, $host ] = LAB_PHASES[ $phase ];

$_SERVER['LAB_PROFILE'] = $profile;
$_SERVER['HTTP_HOST']   = $host;
$_SERVER['SERVER_NAME'] = '127.0.0.1';
$_SERVER['SERVER_PORT'] = substr( $host, strpos( $host, ':' ) + 1 );
$_SERVER['REQUEST_URI'] = '/';
$_SERVER['REMOTE_ADDR'] = '127.0.0.1';

if ( 'network-sites' !== $phase ) {
	define( 'WP_INSTALLING', true );
	define( 'WP_SITEURL', 'http://' . $host );
}
if ( 'network' === $phase ) {
	define( 'LAB_CONVERTING_TO_NETWORK', true );
}

// The platform ends some failures (such as an unresolvable network) with an error page and status 0.
register_shutdown_function(
	static function (): void {
		if ( ! defined( 'LAB_PHASE_DONE' ) ) {
			fwrite( STDERR, "the install phase did not complete\n" );
			exit( 1 );
		}
	}
);

require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/upgrade.php';
require_once ABSPATH . 'wp-admin/includes/plugin.php';

function lab_fail( string $message ): never {
	fwrite( STDERR, $message . "\n" );
	exit( 1 );
}

function lab_done( string $message ): never {
	define( 'LAB_PHASE_DONE', true );
	fwrite( STDOUT, $message . "\n" );
	exit( 0 );
}

function lab_activate( string $plugin, bool $network_wide ): void {
	$activated = activate_plugin( $plugin, '', $network_wide );
	if ( is_wp_error( $activated ) ) {
		lab_fail( "activating {$plugin} failed: {$activated->get_error_message()}" );
	}
}

const LAB_ALPHA = 'ferropress-fixture-alpha.php';
const LAB_BETA  = 'ferropress-fixture-beta/ferropress-fixture-beta.php';

if ( 'network-sites' === $phase ) {
	if ( get_sites( array( 'domain' => $host, 'path' => '/second/', 'count' => true ) ) ) {
		lab_done( 'already installed' );
	}
	$site = wp_insert_site(
		array(
			'domain'  => $host,
			'path'    => '/second/',
			'title'   => 'Second Site',
			'user_id' => 1,
		)
	);
	if ( is_wp_error( $site ) ) {
		lab_fail( "creating the second site failed: {$site->get_error_message()}" );
	}
	lab_activate( LAB_ALPHA, true );
	lab_done( 'installed' );
}

if ( is_blog_installed() ) {
	lab_done( 'already installed' );
}

$title  = 'single' === $phase ? 'Reference Site' : 'Reference Network';
$result = wp_install( $title, 'admin', 'admin@example.test', false, '', 'lab-admin-password' );
if ( empty( $result['user_id'] ) ) {
	lab_fail( 'installation failed' );
}

if ( 'single' === $phase ) {
	// Activated in reverse name order, so traces show the order the platform loads them in.
	lab_activate( LAB_BETA, false );
	lab_activate( LAB_ALPHA, false );
	lab_done( 'installed' );
}

// The main site keeps one plugin of its own; the other becomes network-wide in the next phase.
lab_activate( LAB_BETA, false );
// With multisite off the database object knows no network-wide tables; name them before creating them.
foreach ( $wpdb->tables( 'ms_global' ) as $table => $prefixed_table ) {
	$wpdb->$table = $prefixed_table;
}
install_network();
$network = populate_network( 1, $host, 'admin@example.test', 'Reference Network', '/', false );
if ( is_wp_error( $network ) ) {
	lab_fail( "creating the network failed: {$network->get_error_message()}" );
}
if ( '1' !== $wpdb->get_var( "SELECT COUNT(*) FROM {$wpdb->blogs}" ) ) {
	lab_fail( 'creating the network failed: the main site is not registered' );
}
lab_done( 'installed' );
