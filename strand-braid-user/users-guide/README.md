# User's Guide

## Building

This user's guide can be built with
[mdBook](https://rust-lang.github.io/mdBook). (If you have `cargo` installed,
you can install this with `cargo install mdbook`.)

Build the User's Guide website with the command:

    mdbook build

## Development

You can develop the users guide with the command:

    mdbook serve users-guide --open

This will then run a program which watches for changes, rebuilds the website when
something changes, and hosts it. By default this is at http://localhost:3000/ .
