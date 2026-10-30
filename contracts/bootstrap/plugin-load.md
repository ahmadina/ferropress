# Plugin load order

Which plugin files run on one request, and in what order. Implemented by `crates/cms-bootstrap`
(`contracts/entries/plugin-load-order.toml`). Provenance: the load helpers in the pinned reference
(inventory names only). Expected sequences are the `load` / `action` lines in
`contracts/lifecycle/*.jsonl`.

## Groups

Three groups run in this order:

1. **Must-use.** Every file whose name ends in `.php` that sits directly in the must-use directory.
   Subdirectories are not searched. They sort by the full path, ascending byte order, and always load.
2. **Network-wide**, only on a network request. The stored network-active plugin paths, sorted
   ascending. A path that is also in the site list still loads only in this group.
3. **Site.** The stored site-active list, in the order it is stored. Activating a plugin re-sorts that
   list in ascending byte order, so the lab fixtures (activated newest-first) still load alphabetically.

A site or network path is skipped unless it ends in `.php`, contains no `..`, is not absolute (`/` or a
`:` drive prefix), and names a file that exists under the plugins directory. While the platform is
installing, the site group is empty. In recovery mode, paused plugins are skipped.

When the `hack_file` option is on and a legacy hacks file exists at the installation root (the
interoperability name `my-hacks.php`), that file loads immediately before the site group.

## After each file

Loading a must-use file fires `mu_plugin_loaded` with its path. A network-wide file fires
`network_plugin_loaded`. A site file (and the hacks file) fires `plugin_loaded`. After the must-use
and network groups, `muplugins_loaded` fires. After the site group, `plugins_loaded` fires.
