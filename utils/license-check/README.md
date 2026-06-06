# license-check

CLI program that checks every tracked `*.rs` file in the workspace declares an
`SPDX-License-Identifier:` license header, and can add missing headers in place.

## Policy

Each `.rs` file must contain a line of the form

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
```

within its first lines. The in-house convention is the two-line header:

```rust
// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0
```

A small number of files originate from (or wrap / are rewritten from)
third-party code under a different license. These are listed in
[`allowlist.toml`](allowlist.toml), which maps a repo-relative path prefix to
the SPDX expression required there (e.g. `BSD-2-Clause`). The allowlist is
compiled into the binary, so the check is hermetic.

## Usage

Check all files (this is what CI runs). Exits non-zero if any file is
non-compliant:

```sh
cargo run -p license-check
```

Add the two-line in-house header to files that are missing one, in place.
Files carrying third-party copyright notices are reported as `MANUAL` and left
untouched for hand editing:

```sh
cargo run -p license-check -- --fix
```

By default the current directory is used as the workspace root; pass a path to
override it.
