#!/bin/bash
# Verify that the strand-braid THIRD-PARTY-LICENSES crate list is up to date.
#
# Two checks, both run on every CI pipeline:
#
#   1. Every crate path listed by gen-strand-braid-third-party-licenses.sh
#      exists in the workspace (catches deleted/renamed crates).
#   2. Every binary shipped by strand-braid.install is produced by a crate that
#      is on the license list (catches a newly shipped binary whose licenses
#      would otherwise be silently omitted).
#
# Requires `cargo` and `python3` on $PATH.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
GEN="$HERE/gen-strand-braid-third-party-licenses.sh"
INSTALL_FILE="$REPO_ROOT/_packaging/strand-braid/debian/strand-braid.install"

cd "$REPO_ROOT"

# Check 1: every listed crate path exists.
status=0
while IFS= read -r crate; do
  if [[ ! -f "$REPO_ROOT/$crate/Cargo.toml" ]]; then
    echo "ERROR: license list crate '$crate' has no Cargo.toml" >&2
    status=1
  fi
done < <("$GEN" --list)
if [[ "$status" -ne 0 ]]; then
  exit 1
fi

# Check 2: every shipped binary's crate is on the license list.
GEN="$GEN" INSTALL_FILE="$INSTALL_FILE" python3 - <<'PY'
import json, os, subprocess, sys

# Crate paths on the license list (gen ... --list is the single source of truth).
license_crates = {
    line.strip()
    for line in subprocess.check_output([os.environ["GEN"], "--list"], text=True).splitlines()
    if line.strip()
}

# Binaries installed to usr/bin by the Debian package.
shipped = []
with open(os.environ["INSTALL_FILE"]) as f:
    for line in f:
        line = line.strip()
        if line and not line.startswith("#") and line.endswith("usr/bin"):
            shipped.append(line.split()[0])

# Map every bin/example target name -> set of producing crate dirs.
md = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"]))
root = md["workspace_root"]
target_to_crate = {}
for pkg in md["packages"]:
    cdir = os.path.relpath(os.path.dirname(pkg["manifest_path"]), root)
    for t in pkg["targets"]:
        if {"bin", "example"} & set(t["kind"]):
            target_to_crate.setdefault(t["name"], set()).add(cdir)

# Binaries that are renamed at packaging time and therefore are not a target
# name in the workspace. Map them to their producing crate explicitly.
ALIASES = {
    # Built from the `strand-cam` crate with the `flydratrax` feature.
    "strand-cam-flydratrax": "strand-cam",
}

errors = []
for binname in shipped:
    if binname in ALIASES:
        crates = {ALIASES[binname]}
    else:
        crates = target_to_crate.get(binname)
    if not crates:
        errors.append(
            f"shipped binary '{binname}' maps to no workspace bin/example target "
            f"(add an entry to ALIASES in check-strand-braid-third-party-licenses.sh)")
        continue
    if not (crates & license_crates):
        errors.append(
            f"shipped binary '{binname}' is built by {sorted(crates)}, "
            f"none of which is on the license list")

if errors:
    print("THIRD-PARTY-LICENSES list is out of date:", file=sys.stderr)
    for e in errors:
        print(f"  - {e}", file=sys.stderr)
    print("Update _packaging/gen-strand-braid-third-party-licenses.sh.", file=sys.stderr)
    sys.exit(1)

print(f"OK: all {len(shipped)} shipped binaries are covered by the license list.")
PY
