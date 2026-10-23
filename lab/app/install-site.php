<?php
/**
 * Installs the lab site with fixed settings. Run inside the app container by `cargo xtask lab install`;
 * does nothing if the site is already installed.
 */

$_SERVER['HTTP_HOST']   = '127.0.0.1:8080';
$_SERVER['SERVER_NAME'] = '127.0.0.1';
$_SERVER['SERVER_PORT'] = '8080';
$_SERVER['REQUEST_URI'] = '/';

define( 'WP_INSTALLING', true );
define( 'WP_SITEURL', 'http://127.0.0.1:8080' );

require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/upgrade.php';

if ( is_blog_installed() ) {
	fwrite( STDOUT, "already installed\n" );
	exit( 0 );
}

$result = wp_install( 'Reference Site', 'admin', 'admin@example.test', false, '', 'lab-admin-password' );
if ( empty( $result['user_id'] ) ) {
	fwrite( STDERR, "installation failed\n" );
	exit( 1 );
}
fwrite( STDOUT, "installed\n" );
