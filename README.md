# Ferropress

Ferropress is a Rust-native web platform compatible with WordPress. Its goal is to run existing WordPress
sites, plugins and themes **unchanged, without any PHP runtime installed**: WordPress behavior is
implemented natively in Rust, and plugin/theme PHP source is executed by a PHP engine written in Rust.

> **Status:** pre-implementation. The project is completing its feasibility, legal and contract-extraction
> gates. Nothing here is production-ready, and no plugin or theme is certified yet.

Ferropress is an independent project. It is not affiliated with, sponsored by, or endorsed by WordPress,
the WordPress Foundation or Automattic. "WordPress" is a trademark of the WordPress Foundation and is used
here only to describe compatibility.

## How it works

- **Native core:** users, content, queries, hooks, REST, cron, multisite and the rest of WordPress's
  server-side behavior are implemented in Rust.
- **PHP engine in Rust:** plugin and theme PHP files are inputs to a Rust-implemented PHP engine; no Zend
  Engine, PHP-FPM or `php` binary is used in production.
- **Compatibility bridge:** plugins see the WordPress functions, classes, hooks and globals they expect,
  backed by the native Rust implementation.
- **Own admin, full plugin admin support:** Ferropress has its own admin application that hosts everything
  plugins and themes add to the admin.
- **Proven by tests:** every behavior is compared against a reference WordPress installation, and plugin
  and theme compatibility is published per certified version.

Ferropress does not copy WordPress code. Behavior is studied, specified in our own words, and implemented
independently; see the clean implementation policy in the master plan (§0.5).

## Documentation

- [Engineering master plan](WordPress_Rust_Full_Product_Engineering_Master_Plan.md) — scope, architecture,
  phases and release gates
- [AGENTS.md](AGENTS.md) — engineering rules for contributors and coding agents
- [CONTRIBUTING.md](CONTRIBUTING.md) — how to contribute
- [SECURITY.md](SECURITY.md) — reporting vulnerabilities

## Requirements

- Rust (pinned in `rust-toolchain.toml` once the workspace is created)
- Docker, for the reference lab used by differential tests

## License

To be confirmed before the first release (GPL-2.0-or-later recommended; see master plan §0.3).
