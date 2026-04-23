#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if command -v uv >/dev/null 2>&1; then
    UV=uv
else
    echo "Error: uv command not found. Please install uv."
    exit 1
fi

mkdir -p scratch generated

echo "== Running extrinsics-only.py =="
"$UV" run --project . extrinsics-only.py
echo "== Running opencv5.py =="
"$UV" run --project . opencv5.py
echo "== Running opencv4.py =="
"$UV" run --project . opencv4.py
echo "== Running opencv0.py =="
"$UV" run --project . opencv0.py

echo "== Validating jacobian =="
"$UV" run --project . validate_jacobian.py

echo "== Building generated modules =="
"$UV" run --project . build_modules.py

if [ "${1:-}" = "--check" ]; then
    # Check mode for CI: compare with previous generation
    echo "== Checking for differences =="
    for f in generated/{opencv5,opencv4,opencv0,extrinsics_only}.rs; do
        # Keep scratch dir for debugging in check mode
        if git diff --quiet "$f"; then
            echo "$f is up to date"
        else
            echo "ERROR: $f has uncommitted changes. Run ./regenerate.sh to update."
            # Keep scratch dir for inspection
            exit 1
        fi
    done
else
    echo ""
    echo "Regeneration complete. Outputs in: geometry/bundle-adj/codegen/generated/"
fi
