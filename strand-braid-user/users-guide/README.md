# Braid and Strand Camera User's Guide

This is the source of the *Braid and Strand Camera User's Guide*, which is
visible [here](https://strawlab.github.io/strand-braid/).

## Building

This user's guide can be built with
[mdBook](https://rust-lang.github.io/mdBook). (If you have `cargo` installed,
you can install this with `cargo install mdbook`.)

Build the User's Guide website with the command:

    # run from strand-braid-user/users-guide/
    mdbook build

## Development

You can develop the users guide with the command:

    # run from strand-braid-user/users-guide/
    mdbook serve --open

This will then run a program which watches for changes, rebuilds the website when
something changes, and hosts it. By default this is at http://localhost:3000/ .
