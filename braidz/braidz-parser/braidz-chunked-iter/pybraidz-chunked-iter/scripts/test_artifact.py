#!/usr/bin/env python3
"""Install and smoke-test one built pybraidz artifact."""

import glob
import importlib
import subprocess
import sys


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} ARTIFACT_GLOB")

    artifacts = glob.glob(sys.argv[1])
    if len(artifacts) != 1:
        raise RuntimeError(f"expected one artifact, found: {artifacts}")

    artifact = artifacts[0]
    if artifact.endswith(".whl") and "-cp38-abi3-" not in artifact:
        raise RuntimeError(
            f"wheel does not target the Python 3.8 stable ABI: {artifact}"
        )

    subprocess.check_call(
        [sys.executable, "-m", "pip", "install", "numpy", artifact]
    )
    module = importlib.import_module("pybraidz_chunked_iter")

    try:
        module.chunk_on_duration("/definitely/missing.braidz", 1.0)
    except ValueError:
        pass
    else:
        raise AssertionError("expected ValueError for a missing archive")


if __name__ == "__main__":
    main()
