# Making Releases

This document describes how to cut a release of `strand-braid`.

## What a release is

A `strand-braid` release is a set of **Debian (`.deb`) packages published to a
[GitHub Release](https://github.com/strawlab/strand-braid/releases)** — one
`.zip` per supported Ubuntu version, each containing the `strand-braid` `.deb`,
a `README.txt`, and the third-party license files.

We do **not** publish individual crates to [crates.io](https://crates.io). The
unit of release is the `strand-braid` Debian package, which bundles every
shipped binary (`strand-cam`, `braid`, `braid-run`, the `braidz` tools, the
calibration tools, the media utilities, etc.).

## Platform status

| Platform | Artifact | Release status |
| :--- | :--- | :--- |
| Ubuntu (20.04 / 22.04 / 24.04 / 26.04) | `.deb` inside a per-version `.zip` | **Published on tag** — fully automated. |
| Windows (x86_64) | portable `.zip` of `.exe` files | **Blocked** — builds on every push, but tag/release publishing is gated (see below). |
| macOS (Apple Silicon) | portable `.zip` of unsigned binaries | **Published on tag** — automated; unsigned (see below). |

### Windows: release publishing is gated on the Pylon shim

The Windows workflow
([`build-strand-braid-windows.yml`](../../.github/workflows/build-strand-braid-windows.yml))
builds a portable `.zip` on every push and PR, but **tag (release) builds are
deliberately blocked** until a Windows build of the public Pylon C-ABI shim DLL
exists.

The Basler/Pylon backend loads that shim at runtime. On Linux the `.deb` bundles
a precompiled `libpylon-cabi-*.so` (downloaded from `strawlab.org`); no
equivalent Windows `.dll` has been built yet. Rather than publish a Windows
release that silently lacks Basler support, the workflow's "Require Pylon shim
on release builds" step fails the build on tags whenever the
`PYLON_CABI_WIN_URL` repository variable is unset.

**To unblock Windows releases:**

1. Build the Windows `libpylon-cabi` DLL and host it alongside the Linux `.so`
   under `https://strawlab.org/assets/libpylon-cabi/precompiled/`.
2. Set the `PYLON_CABI_WIN_URL` repository variable (Settings → Secrets and
   variables → Actions → Variables) to that URL.
3. Remove the "Require Pylon shim on release builds" guard step (the "Bundle
   Pylon shim" and "Assemble release zip" steps already pick the DLL up).

Until then, Windows `.zip`s attached to a release would be incomplete, so no tag
should be expected to publish one.

### macOS: portable, unsigned .zip (Apple Silicon)

The macOS workflow
([`build-strand-braid-macos.yml`](../../.github/workflows/build-strand-braid-macos.yml))
builds Apple-Silicon (`aarch64-apple-darwin`) binaries on a `macos-14` runner
and, on tag pushes, publishes a portable `strand-braid-macos-aarch64-<version>.zip`
to the GitHub Release — mirroring the Windows workflow.

Unlike Windows, the macOS Pylon C-ABI shim already exists, so the Basler backend
is not a blocker: the build bundles
`libpylon-cabi-v1-macos-aarch64-pylon_7.3.1.9.dylib` (hosted next to the Linux
`.so` under `https://strawlab.org/assets/libpylon-cabi/precompiled/`)
unconditionally, exactly as the `.deb` bundles the Linux shim.

The binaries are shipped **unsigned / un-notarized**. macOS Gatekeeper
quarantines downloaded binaries, so the bundled
[`_packaging/macos-installer-zip-readme.txt`](../../_packaging/macos-installer-zip-readme.txt)
documents the one-time `xattr -dr com.apple.quarantine <dir>` workaround. Adding
Developer-ID signing + notarization later (an Apple Developer account plus
signing secrets) would remove that friction.

Only Apple Silicon is built; an Intel (`x86_64`) build would need a `macos-13`
runner and a separate `x86_64` Pylon shim.

> Note: the auto-generated **Downloads** table (below) currently lists only the
> Ubuntu `.zip`s. The Windows and macOS zips are attached to the same Release
> but are not yet added to that table.

## How automated is it?

The expensive part — building for every supported Ubuntu version and publishing
the GitHub Release — is **fully automated**. Pushing a git tag is all it takes.

The GitHub Actions workflow
[`.github/workflows/build-strand-braid-deb.yml`](../../.github/workflows/build-strand-braid-deb.yml)
runs on every push, but on a **tag push** the shared composite action
[`.github/actions/package-strand-braid-deb`](../../.github/actions/package-strand-braid-deb/action.yml)
additionally publishes the per-Ubuntu `.zip` to the GitHub Release (via
`softprops/action-gh-release`, gated on `github.ref_type == 'tag'`).

It builds for four Ubuntu versions:

| Ubuntu | Codename | How it builds |
| :--- | :--- | :--- |
| 22.04 | jammy | native GitHub-hosted runner |
| 24.04 | noble | native GitHub-hosted runner |
| 20.04 | focal | inside an `ubuntu:focal` container |
| 26.04 | resolute | inside an `ubuntu:resolute` container |

The remaining work — bumping the version, refreshing `Cargo.lock`, rolling the
changelog, and tagging — is manual but assisted by a small script. The whole
process is the checklist below.

## Versioning model

The release version lives in **one place**: the `[workspace.package]` table in
the root [`Cargo.toml`](../../Cargo.toml):

```toml
[workspace.package]
version = "1.0.0-rc.3"
```

Every crate that participates in a release inherits it:

```toml
[package]
name = "braid"
version.workspace = true
```

Crates that are *not* part of a release simply keep their own `version = "..."`
and do not opt in, so the set of crates carrying `version.workspace = true` is
exactly the set that ships in a release.

The Debian package version comes from `CARGO_PKG_VERSION` of the
[`write-debian-changelog`](../../utils/write-debian-changelog) crate (which
inherits the workspace version), so bumping the one workspace version is exactly
what sets the version of the published `.deb`.

The release **tag name must equal that version, with no prefix** — e.g. the tag
`1.0.0-rc.3` for version `1.0.0-rc.3`. The tag names the published release `.zip`
files; the workspace version names the `.deb` inside them. To stop these from
drifting, the packaging action verifies `tag == workspace version` on tag builds
and fails the build if they disagree.

(Older tags in this repo such as `strand-cam/0.10.1` predate this whole-project
release scheme and are per-crate; new releases use the bare-version tag.)

## Release checklist

The example below cuts `1.0.0-rc.3`. Substitute your version throughout.

### 1. Start from a clean, up-to-date `main`

```
git switch main
git pull
git status   # should be clean
```

Decide the version number ([semver](https://semver.org/); release candidates use
the `-rc.N` suffix).

### 2. Bump the version

Set the release version in the `[workspace.package]` table of the root
[`Cargo.toml`](../../Cargo.toml):

```toml
[workspace.package]
version = "1.0.0-rc.3"
```

Every crate with `version.workspace = true` picks up the new version
automatically. Then refresh the lockfile's workspace entries:

```
cargo update --workspace
```

Use `--workspace` (not a bare `cargo update`, which would also bump external
dependencies and pull unrelated churn into the release commit).

### 3. Update the changelog

Edit [`CHANGELOG.md`](../../CHANGELOG.md):

- Rename the top `## Unreleased` heading to `## 1.0.0-rc.3 - YYYY-MM-DD` (use the
  release date).
- Read through the entries and make sure they describe this release accurately.

### 4. Build locally to sanity-check (recommended)

A full local build is the same one CI runs. At minimum confirm the workspace
still builds and the lockfile is consistent:

```
cargo check --workspace
```

To exercise the actual packaging end-to-end (requires the build dependencies
from [`building-for-development.md`](building-for-development.md) plus `trunk`),
you can reproduce what CI does — build the binaries, collect them into `build/`,
then `make -C _packaging`. The authoritative recipe is the composite action
[`.github/actions/package-strand-braid-deb/action.yml`](../../.github/actions/package-strand-braid-deb/action.yml);
the GitHub Release is built from exactly those steps, so a local rebuild is
optional.

### 5. Commit

```
git add -A
git commit -m "Release 1.0.0-rc.3"
```

### 6. Tag and push

Push the commit first, then the tag. **Pushing the tag is what triggers the
build and publishes the public GitHub Release** — do this only when you are
ready.

```
git push origin main
git tag 1.0.0-rc.3
git push origin 1.0.0-rc.3
```

### 7. Watch the build

Open the
[Actions tab](https://github.com/strawlab/strand-braid/actions/workflows/build-strand-braid-deb.yml)
and confirm all four Ubuntu jobs pass. If the tag and workspace version disagree,
the "Verify tag matches workspace version" step fails early — fix the version (step 2),
delete and recreate the tag, and push again.

On success a GitHub Release for the tag holds four `.zip` assets:

```
strand-braid-ubuntu-2004-1.0.0-rc.3.zip
strand-braid-ubuntu-2204-1.0.0-rc.3.zip
strand-braid-ubuntu-2404-1.0.0-rc.3.zip
strand-braid-ubuntu-2604-1.0.0-rc.3.zip
```

### 8. Finalize the release notes

GitHub creates the Release as part of the upload. Once all four `.zip` assets
are published, the `update-release-notes` job prepends a **Downloads** table
(one row per `.zip`, linking each Ubuntu version's asset) to the top of the
release body, so users see the downloads without expanding the collapsed
"Assets" section. The table sits between `<!-- BEGIN DOWNLOAD TABLE -->` and
`<!-- END DOWNLOAD TABLE -->` markers; re-running the workflow replaces it in
place and leaves anything below the markers untouched.

Edit the release to add human-readable release notes *below* the table (the
relevant `CHANGELOG.md` section is a good source) — leave the marker block
intact — and mark it as a pre-release for `-rc.N` versions.

### 9. Update https://version-check.strawlab.org/

In a step done outside this repository, but the files served from
https://version-check.strawlab.org/ need to be updated to reflect the new
release.

### 10. Prepare for future development

Add a fresh empty `## Unreleased` section at the start of CHANGELOG.md for
future work.

## Fixing a mistake

A release is published by a tag, so an aborted or wrong release is undone by
removing the tag (and the GitHub Release, if one was created):

```
git push origin :refs/tags/1.0.0-rc.3   # delete the remote tag
git tag -d 1.0.0-rc.3                    # delete the local tag
```

Then delete the GitHub Release in the web UI, correct the problem, and start
over. Prefer a new version number over re-publishing the same one if anything was
already downloaded.

## Files involved

| File | Role |
| :--- | :--- |
| [`CHANGELOG.md`](../../CHANGELOG.md) | Human-facing change history. |
| [`.github/workflows/build-strand-braid-deb.yml`](../../.github/workflows/build-strand-braid-deb.yml) | Triggers the per-Ubuntu builds; publishes on tags. |
| [`.github/actions/package-strand-braid-deb/action.yml`](../../.github/actions/package-strand-braid-deb/action.yml) | The build/package/publish steps, plus the tag/version guard. |
| [`_packaging/Makefile`](../../_packaging/Makefile) | Drives `dpkg-buildpackage` for the `strand-braid` package. |
| [`utils/write-debian-changelog`](../../utils/write-debian-changelog) | Emits `debian/changelog`; its `CARGO_PKG_VERSION` is the `.deb` version. |
