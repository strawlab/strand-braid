# Braid Configuration and Launching

## How to launch Braid

The central runtime of Braid, the `braid-run` executable, is launched from the
command line like so:

```ignore
braid run braid-config.toml
```

The `braid-config.toml` is the path of a Braid TOML configuration file.

## Braid TOML configuration files

The Braid configuration file, in the [TOML format](https://toml.io/), specifies
how Braid and multiple Strand Camera instances are launched. Any options not
specified result in default values being used. The defaults should be reasonable
and secure, allowing minimal configurations describing only specific aspects of
a particular setup.

The reference documentation for the `BraidConfig` type, which is automatically
deserialized from a `.toml` file:
[`braid_config_data::BraidConfig`](https://strawlab.org/strand-braid-api-docs/latest/braid_config_data/struct.BraidConfig.html).

Here is a minimal configuration for a 3 camera Braid setup:

```toml
{{#include ../../../braid/simple.toml}}
```
