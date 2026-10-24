# Reference lab

The lab runs the pinned upstream reference platform on PHP and MySQL in Docker. It is the **oracle** that
Ferropress output is compared against, and it exists only for development and CI. Nothing here is part of
Ferropress or its production builds.

## Usage

```bash
cargo xtask lab up      # build and start the stack, wait until healthy
cargo xtask lab down    # stop it, keeping the database
cargo xtask lab reset   # stop it, delete the database, start from scratch and install both profiles
cargo xtask lab install # install both profiles with fixed settings (no-op for what is installed)
cargo xtask lab fetch   # clone pinned upstream sources into upstream/, download archives into packages/
cargo xtask lab capture home              # record GET / into captures/home.json
cargo xtask lab capture first-post '/?p=1'
cargo xtask lab capture second-site /second/ --profile network
cargo xtask lab check front-page          # run contracts/scenarios/front-page.toml twice and compare
cargo xtask lab inventory                 # record the reference symbol inventory into contracts/inventory/
cargo xtask lab lifecycle                 # record the reference lifecycle into contracts/lifecycle/
cargo xtask lab serialize                 # record serialized-format outcomes into contracts/serialize/
cargo xtask lab value                     # record value-model scenarios into contracts/value/
cargo xtask lab lexer                     # record lexer cases into contracts/lexer/
cargo xtask lab lexer-corpus              # compare our lexer with the reference on the upstream sources
cargo xtask lab parser                    # record parser cases into contracts/parser/
cargo xtask lab parser-corpus             # compare our parser with the reference on the upstream sources
cargo xtask lab compiler                  # record compile-time diagnostics into contracts/compiler/
cargo xtask lab operators                 # record operator and conversion results into contracts/operators/
```

`lab fetch` checks every clone against its pinned commit and every archive against its SHA-256 before
moving it into place, and refuses to overwrite a checkout that is at a different commit.

The application is published on localhost only. Credentials in `compose.yaml` are lab-only.

## Profiles

One database holds two installations, each on its own port:

| Profile | Address | Installation |
|---|---|---|
| `single` | <http://127.0.0.1:8080/> | one site |
| `network` | <http://127.0.0.1:8081/> | a subdirectory network: the main site and a second site at `/second/` |

Apache sets `LAB_PROFILE` per port (`app/sites.conf`) and `app/config.php` selects the table prefix and,
for the network, the multisite settings. The installer runs in three phases: the single site; the
network's main site, converted into a network while multisite is still off (as the platform's own
network setup does); and, with multisite on, the second site and a network-wide plugin. Scenarios pick a
profile with `profile = "network"`; the default is `single`. Health checks hit an endpoint Apache answers
itself, so the application never sees a request that is not part of a scenario.

## Captures

`lab capture` dumps the database, sends one GET request (no redirects followed) tagged with a capture id,
dumps the database again and writes `captures/<name>.json` containing:

- the response status, headers, body and body SHA-256;
- the ordered hook trace (action or filter, name, argument count, nesting depth), from the first hook
  of the boot through the shutdown hooks;
- every source file loaded, in order, with its position in the hook trace;
- every SQL query run during the request;
- outgoing mail, intercepted and never delivered;
- scheduled cron events, without timestamps;
- the rows added and removed per table.

Runs are deterministic: every process in the app container sees a wall-clock time that starts at a
fixed instant and advances one microsecond per clock read, never with real time (`FAKETIME` in
`compose.yaml`, provided by the pinned `libfaketime` package), the secret keys in the
generated config are fixed lab-only values, and cron never runs on its own. Two captures of the same
request from the same database state are therefore identical.

The trace comes from our own instrumentation (`app/instrument.php`), required from the generated site
config through `app/config.php`. It pre-registers its callbacks in the `$wp_filter` global, which the
platform turns into hooks as soon as its plugin API loads, so the trace starts at the first hook and no
plugin of ours appears in the recorded lifecycle. It records only requests carrying the capture header;
the shutdown writer re-registers itself so the platform's own shutdown hooks are part of the trace. The
lab config enables query logging and disables request-triggered cron so captures are repeatable. Captures are git-ignored; reviewed fixtures derived from them belong in `contracts/`.

## Fixtures

The installer activates our own behavior-free fixture plugins (`app/fixtures/`): one must-use plugin, one
single-file plugin and one directory plugin, activated in reverse name order. In the network, the
directory plugin is active on the main site only and the single-file plugin is active network-wide. They only exist so traces
show where each kind of plugin loads and in which order.

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

## Lifecycle

`lab lifecycle` records, for each profile, the order in which a request loads source files and fires
actions, from the first boot hook through shutdown, and writes one JSON Lines file per request to
`contracts/lifecycle/`:

| File | Request |
|---|---|
| `single-front-page.jsonl` | front page of the single site |
| `network-main-site.jsonl` | front page of the network's main site |
| `network-second-site.jsonl` | front page of the network's second site (`/second/`) |

The first line is a header (`schema`, `platform_version`, `id`, `profile`, `path`); every other line is
either `{"event":"load","path":...}` or `{"event":"action","name":...,"args":...,"depth":...}`. Each
request is recorded in its steady state, after one warm-up visit, so one-off work done by the first visit
after an install is excluded. Filters are left out because scenarios already compare the full hook trace.
The lifecycle shows, for example, network resolution before any plugin, must-use plugins before
network-wide plugins before site plugins, and the theme's functions file between `setup_theme` and
`after_setup_theme`.

The whole recording runs twice from fresh installs and nothing is written unless both runs match. A test
rejects committed files that are missing, out of date with the pinned version, or not canonical.

## Serialized format

`lab serialize` runs our own script (`app/serialize-oracle.php`) in the running lab; no reset is needed
because it touches no site state. The script builds a deterministic corpus: hand-written seed inputs,
every truncation of each, byte substitutions and seeded random edits. It reads each input with
`unserialize()` while no class may be instantiated, then writes two JSON Lines files to
`contracts/serialize/`, each starting with a header line (`{"schema", "php_version", "section"}`):

| File | One line per |
|---|---|
| `decode.jsonl` | input (with its `max_depth` option, if any): whether it was refused, every message emitted, and what the value serializes back to |
| `floats.jsonl` | float bit pattern (special values, powers of ten and their neighbors, tie-prone values, random bits) and its serialized text |

Byte strings hold one character per byte, with the byte's value as its code point, so the files stay
readable and every byte round-trips. A second run produces byte-identical files. `crates/php-serialize`
replays both files in its tests; the rules they prove are in `contracts/runtime/serialized-format.md`.

## Value model

`lab value` runs our own script (`app/value-oracle.php`) in the running lab, again without a reset. Each
scenario is a list of operations on the variables `a`, `b` and `c`: assigning a value (scalar, empty
array, new object or a copy of another path), binding one path to another by reference, unsetting a
path, and dumping a variable. Paths mix integer and string keys, appends and properties. About two dozen
scenarios are hand-written for specific rules; the rest are generated from a fixed seed. The script turns
each scenario into PHP code and runs it in a fresh PHP process, so object ids are comparable, recording
every thrown error, warning and deprecation, and a dump of every variable at the end. A dump shows each
value with its type, array elements in order, which elements share a reference (numbered per dump),
object ids and properties, and marks recursion.

The result is written to `contracts/value/scenarios.jsonl` (a header line, then one scenario per line,
byte strings encoded as above). A second run produces a byte-identical file. `crates/php-value` replays
every scenario on its value model and compares all events; the rules they prove, and the one scenario
recorded as a known difference, are in `contracts/runtime/value-model.md`.

## Lexer

`lab lexer` runs our own script (`app/lexer-oracle.php`) in the running lab. The cases are about 170
hand-written sources for specific rules plus 2500 sources joined from random fragments (fixed seed).
Each case runs with short open tags on, and those containing `<?` again in a child process with them
off. For every case the script records each token's name, length and line, the warnings the scanner
raised, the parse error compiling the source reports (if any), and the decoded value of every constant
string literal or the error decoding it. Warnings that bypass error handlers are read from the output.

The result is written to `contracts/lexer/cases.jsonl` (a header line, then one case per line, byte
strings encoded as above). A second run produces a byte-identical file. `crates/php-lexer` replays every
case; the rules they prove are in `contracts/runtime/lexer.md`.

## Lexer corpus

`lab lexer-corpus` needs the checkouts from `lab fetch`: `compose.yaml` mounts `upstream/` read-only at
`/opt/upstream` (recreate the stack with `lab up` after fetching). Our script (`app/lexer-corpus.php`)
tokenizes every `.php` file of the platform checkout and the FILE section of every `.phpt` test of the
PHP checkout and prints, per file, its SHA-256, token count, a digest of the token stream and its
warnings. xtask lexes the same files with `crates/php-lexer` (a file whose bytes on the host differ
from the lab's counts as a difference) and writes every difference to `captures/lexer-corpus.txt`; the
command fails when any file differs. Nothing from
the corpus is committed.

## Parser

`lab parser` runs our own script (`app/parser-oracle.php`) in the running lab. The cases are
hand-written sources, the lexer oracle's cases, 3000 random expressions and 2500 one-token edits of
valid hand-written cases (fixed seed), and hand-written cases grouped by grammar area, listed last so
adding some leaves the seeded ones unchanged. For every case the script records the error parsing it
raises (class, message, line), the warnings raised on the way, and the engine's rendering of the code:
the message of a failed `assert()` on a closure whose body is the case. Renderings come from a worker
process that is restarted when compiling a case ends it.

The result is written to `contracts/parser/cases.jsonl` (a header line, then one case per line, byte
strings encoded as above). `crates/php-parser` replays every case; the rules they prove are in
`contracts/runtime/parser.md`.

## Parser corpus

`lab parser-corpus` parses the same upstream files as `lab lexer-corpus` with our script
(`app/parser-corpus.php`), recording per file its SHA-256, the error parsing it raises and its
warnings. xtask parses the same files with `crates/php-parser`, writes every difference to
`captures/parser-corpus.txt` and fails when any file differs. Nothing from the corpus is committed.

## Compiler

`lab compiler` runs our own script (`app/compiler-oracle.php`) in the running lab. It writes every case
to a file of its own and compiles the files in lint mode (`php -l`), many per process; lint mode compiles
without running anything and carries no classes or functions from one file into the next. For every case
it records the diagnostics compiling it reported, in order: severity (`Fatal error`, `Warning`,
`Deprecated`, …), message and line. A file stops at its first fatal error, and a message naming the
case's own file shows `%file%` in its place. The cases are hand-written sources grouped by the compiler
area they probe, followed by every parser oracle case that parses.

The result is written to `contracts/compiler/cases.jsonl` (a header line recording the PHP version,
`short_open_tag` and `zend.assertions`, then one case per line, byte strings encoded as above). A second
run produces a byte-identical file.

## Operators

`lab operators` runs our own script (`app/operator-oracle.php`) in the running lab, with
`serialize_precision` -1 and without strict types. It applies every unary operation (negation, unary
plus, bitwise not, logical not, the five casts, pre-increment and pre-decrement) to a fixed grid of
operands, and every binary operator (arithmetic, bitwise, shifts, concatenation and the comparisons) to
every pair of them. For each operation it records the result, the warnings, notices and deprecations
raised, in order, and the class and message of any error thrown.

The result is written to `contracts/operators/cases.jsonl`: a header line with the PHP version, the
operands, and the tables of messages and errors the results refer to by index, then one line per unary
operation and one per binary operator and left operand. Values are tagged (`["int",7]`); a float is
its IEEE 754 bit pattern in hex, so NAN payloads and negative zero survive, and byte strings are
encoded as above. A second run produces a byte-identical file. `crates/php-value` replays it in
`tests/operators.rs`.

## Pinning

Versions, image digests, the release archive URL and its SHA-256 come from
`contracts/reference-manifest.toml`; `cargo xtask` passes them to Docker, so the compose file and Dockerfile
contain no versions. The image build downloads the archive itself and fails if its hash differs.

## Clean-room rules

Upstream sources and archives are fetched into git-ignored directories (`upstream/`, `packages/`) and are
never committed. See `.cursor/rules/clean-room-copyright.mdc`.
