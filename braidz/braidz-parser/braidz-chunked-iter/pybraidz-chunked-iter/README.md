# pybraidz-chunked-iter - Chunked iteration over tables in `.braidz` files.

## Installation

This package is available through PyPI and can be installed with pip:

    pip install pybraidz_chunked_iter

Python 3.10 or newer is required. Wheels use Python's stable ABI with Python
3.10 as the minimum, so one wheel serves every later Python 3 release. Python
3.10 is the default on Ubuntu 22.04, and the oldest version the wheels can
still be tested with on GitHub's current Ubuntu runners.

## Example usage

See example usage in the "Chunked iteration of `kalman_estimates`" section of
the [docs](https://strawlab.github.io/strand-braid/braidz-files.html).

## Develop

This will iterate over chunks of the file `20201104_174158.braidz`, which can be
downloaded [here](https://strawlab-cdn.com/assets/20201104_174158.braidz):

    maturin develop && python examples/simple.py 20201104_174158.braidz

## Build a Python wheel

    maturin build

CI builds each wheel on a native runner for its target architecture, then uses
`scripts/test_artifact.py` to install it and exercise the compiled extension.
The same smoke test can be run on a locally built artifact:

    python scripts/test_artifact.py "target/wheels/*.whl"
