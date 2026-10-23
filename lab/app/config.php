<?php
/**
 * Lab-only site settings, required by the generated site config before the platform boots. Every query
 * is recorded for captures and cron never spawns background requests, keeping runs deterministic.
 */

define( 'SAVEQUERIES', true );
define( 'DISABLE_WP_CRON', true );

require __DIR__ . '/instrument.php';
