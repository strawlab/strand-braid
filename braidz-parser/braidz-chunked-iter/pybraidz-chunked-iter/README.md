# pybraidz-chunked-iter - Chunked iteration over tables in `.braidz` files.

## Example usage

See example usage in the
[docs](https://strawlab.github.io/strand-braid/braidz-files.html).

## Develop

This will iterate over chunks of the file `20201104_174158.braidz`, which can be
downloaded [here](https://strawlab-cdn.com/assets/20201104_174158.braidz):

    maturin develop && python examples/simple.py 20201104_174158.braidz

## Build a Python wheel

    maturin build
