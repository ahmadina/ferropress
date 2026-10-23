# ADR-0006: Admin application and block-editor hosting

- **Status:** Accepted for the admin application; Proposed for block-editor hosting (owner decision after
  the contract inventory, roadmap step 12).
- **Date:** 2026-10-23

## Context

Plugins and themes extend the administration area heavily: they add menus and submenus, settings pages,
meta boxes, list tables, admin notices, dashboard widgets, AJAX and form-post endpoints, enqueued scripts
and styles, inline HTML and JavaScript, and editor blocks. Copying the upstream admin screens is neither
wanted nor allowed (owner mandate and clean-room policy), but a redesigned admin that ignores this
extension surface would make those plugins unusable.

## Decision

**Own admin application (accepted).**

- Ferropress ships its own admin application with its own design, code and assets. No upstream admin
  markup, CSS, JavaScript, images or text is copied.
- The admin **extension surface** is a hard compatibility requirement: every registration API, hook,
  request endpoint, capability check, browser global, localized script object and CSS class contract that
  plugins use to add things to the admin is supported.
- Plugin-rendered screens and fragments run inside the Ferropress admin shell. The shell provides the
  page frame, navigation built from registered menus, notice areas, screen options and the markup
  conventions plugin output relies on, so plugin HTML and scripts render and behave as they do in the
  reference.
- The owning crate is `cms-admin` (domain: registries and routing of admin extensions) with the shell UI
  served by the platform layer; plugin-facing names are registered as data by `php-bindings`.

**Block-editor hosting (proposed — options under evaluation).**

Plugin blocks are written against a large browser-side API (component libraries, data stores, block
registration and editor extension points). Two options are being evaluated:

1. **Use the published upstream editor packages at runtime.** They are GPL-2.0-or-later, compatible with
   the project license. They would never be committed to this repository; the build or the installer
   would fetch pinned versions from their public registry with verified hashes. Highest compatibility;
   the editor's look and behavior would follow upstream inside the Ferropress shell.
2. **Write an independent editor** that implements the same browser-side API clean-room. Full design
   freedom, but the API surface is very large and every gap breaks plugin blocks.

Evaluation criteria: share of the certified plugin set's blocks that work, size of the API surface the
inventory finds plugins actually use, legal review of runtime fetching, maintenance cost when upstream
changes, and consistency with the owner's "own admin, no clone" mandate.

## Consequences

- Admin compatibility is certified per plugin through browser end-to-end tests of its admin workflows; a
  plugin whose admin pages fail is not certified.
- The admin shell must reproduce markup conventions plugins depend on (for example notice and form-table
  structure) as an interoperability contract, implemented in our own code and styles.
- Until the editor decision is made, no editor code is written; classic (non-block) editing and plugin
  meta boxes proceed independently.

## Verification

- Contract inventory (roadmap step 12) lists every admin extension point and browser-side API used by the
  plugin corpus, with usage counts.
- Browser end-to-end tests run plugin admin workflows against both the reference and Ferropress (Phase 7
  and certification in Phase 8).
- The editor decision is recorded by updating this ADR (or superseding it) with the inventory numbers and
  the owner's choice.
