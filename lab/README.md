# Reference lab

The lab runs the pinned upstream reference platform on PHP and MySQL in Docker. It is the **oracle** that
Ferropress output is compared against, and it exists only for development and CI. Nothing here is part of
Ferropress or its production builds.

## Usage

```bash
cargo xtask lab up      # build and start the stack, wait until healthy
cargo xtask lab down    # stop it, keeping the database
cargo xtask lab reset   # stop it, delete the database, start from scratch
cargo xtask lab fetch   # clone pinned upstream sources into upstream/, download archives into packages/
```

`lab fetch` checks every clone against its pinned commit and every archive against its SHA-256 before
moving it into place, and refuses to overwrite a checkout that is at a different commit.

The application is published on <http://127.0.0.1:8080/> only. Credentials in `compose.yaml` are lab-only.

## Pinning

Versions, image digests, the release archive URL and its SHA-256 come from
`contracts/reference-manifest.toml`; `cargo xtask` passes them to Docker, so the compose file and Dockerfile
contain no versions. The image build downloads the archive itself and fails if its hash differs.

## Clean-room rules

Upstream sources and archives are fetched into git-ignored directories (`upstream/`, `packages/`) and are
never committed. See `.cursor/rules/clean-room-copyright.mdc`.
