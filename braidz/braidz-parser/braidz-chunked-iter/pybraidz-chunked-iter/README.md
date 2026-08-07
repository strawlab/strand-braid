# pybraidz-chunked-iter - Chunked iteration over tables in `.braidz` files.

## Installation

This package is available through PyPI and can be installed with pip:

    pip install pybraidz_chunked_iter

Python 3.8 or newer is required. Wheels use Python's stable ABI with Python 3.8
as the deliberate minimum, matching the oldest Python version supported by
PyO3 0.29 while retaining compatibility with newer Python 3 releases.

## Example usage

See example usage in the "Chunked iteration of `kalman_estimates`" section of
the [docs](https://strawlab.github.io/strand-braid/braidz-files.html).

## Develop

This will iterate over chunks of the file `20201104_174158.braidz`, which can be
downloaded [here](https://strawlab-cdn.com/assets/20201104_174158.braidz):

    maturin develop && python examples/simple.py 20201104_174158.braidz

## Build a Python wheel

    maturin build
