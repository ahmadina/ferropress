# Reference lab

The lab runs the pinned upstream reference platform on PHP and MySQL in Docker. It is the **oracle** that
Ferropress output is compared against, and it exists only for development and CI. Nothing here is part of
Ferropress or its production builds.

## Usage

```bash
cargo xtask lab up      # build and start the stack, wait until healthy
cargo xtask lab down    # stop it, keeping the database
cargo xtask lab reset   # stop it, delete the database, start from scratch and install the site
cargo xtask lab install # install the site with fixed settings (no-op when already installed)
cargo xtask lab fetch   # clone pinned upstream sources into upstream/, download archives into packages/
cargo xtask lab capture home              # record GET / into captures/home.json
cargo xtask lab capture first-post '/?p=1'
cargo xtask lab check front-page          # run contracts/scenarios/front-page.toml twice and compare
cargo xtask lab inventory                 # record the reference symbol inventory into contracts/inventory/
```

`lab fetch` checks every clone against its pinned commit and every archive against its SHA-256 before
moving it into place, and refuses to overwrite a checkout that is at a different commit.

The application is published on <http://127.0.0.1:8080/> only. Credentials in `compose.yaml` are lab-only.

## Captures

`lab capture` dumps the database, sends one GET request (no redirects followed) tagged with a capture id,
dumps the database again and writes `captures/<name>.json` containing:

- the response status, headers, body and body SHA-256;
- the ordered hook trace (action or filter, name, argument count, nesting depth);
- every SQL query run during the request;
- outgoing mail, intercepted and never delivered;
- scheduled cron events, without timestamps;
- the rows added and removed per table.

Runs are deterministic: every process in the app container sees a wall-clock time that starts at a
fixed instant and advances one microsecond per clock read, never with real time (`FAKETIME` in
`compose.yaml`, provided by the pinned `libfaketime` package), the secret keys in the
generated config are fixed lab-only values, and cron never runs on its own. Two captures of the same
request from the same database state are therefore identical.

The trace comes from our own must-use plugin (`app/capture.php`), which records only requests carrying the
capture header. The site config enables query logging and disables request-triggered cron so captures are
repeatable. Captures are git-ignored; reviewed fixtures derived from them belong in `contracts/`.

## Checking scenarios

`lab check <id>` runs `contracts/scenarios/<id>.toml` twice, each time from a freshly reset and installed
site, writes every step to `captures/<id>.<run>.<step>.json` and compares the two runs with the
scenario's aspects and reviewed normalizations. It fails on any mismatch: a scenario must be
deterministic on the reference before Ferropress can be compared against it.

## Inventory

`lab inventory` resets the lab and runs our own script (`app/inventory.php`) inside the app container.
It tokenizes every core PHP file (themes, plugins and the site config are excluded) and asks the booted
platform for its runtime registries, then writes the interoperability facts to `contracts/inventory/`,
one JSON Lines file per section. Each file starts with a header line
(`{"schema", "platform_version", "section"}`), followed by one entry per line, sorted:

| Section | One entry per |
|---|---|
| `functions` | function name, with every declaration's location, parameters (type, default, by-reference, variadic) and return type |
| `classes` | class, interface, trait or enum: parents, interfaces, traits, constant names, properties and method signatures |
| `hooks` | action or filter name (`{expr}` marks a dynamic part), deprecation, and every call site with its argument count |
| `constants` | constant name, declaration sites and whether it is defined after boot |
| `globals` | name of a variable declared `global`, the files doing so and whether it is set after boot |
| `rest-routes` | REST route with its namespace and methods |
| `options` | option name and autoload flag after a fresh install (transients excluded) |
| `tables` | table name, scope, columns and indexes |

The inventory holds names, signatures and locations only: no code bodies, comments or docblocks. A second
run produces byte-identical files, and a test rejects committed files that are not valid and canonical.
Regenerate it whenever the pinned platform version changes.

## Pinning

Versions, image digests, the release archive URL and its SHA-256 come from
`contracts/reference-manifest.toml`; `cargo xtask` passes them to Docker, so the compose file and Dockerfile
contain no versions. The image build downloads the archive itself and fails if its hash differs.

## Clean-room rules

Upstream sources and archives are fetched into git-ignored directories (`upstream/`, `packages/`) and are
never committed. See `.cursor/rules/clean-room-copyright.mdc`.
