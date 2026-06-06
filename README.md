# Strand-Braid

[![User's Guide](https://img.shields.io/badge/docs-User's%20Guide-blue.svg?logo=Gitbook)](https://strawlab.github.io/strand-braid/)

## Description

[Strand Camera](https://strawlab.org/strand-cam/) is low-latency camera
acquisition and tracking software. It is useful for 2D tracking of animals,
robots, or other moving objects. It also serves as the basis for 3D tracking
using Braid.

[Braid](https://strawlab.org/braid/) is multi-camera acquisition and tracking
software. It is useful for 3D tracking of animals, robots, or other moving
objects. It operates with low latency and is suitable for closed-loop
experimental systems such as [virtual reality for freely moving
animals](https://strawlab.org/freemovr/).

This repository is a mono repository that houses the source code for both pieces
of software as well as many other related pieces, mostly written as Rust crates.

Users, as opposed to developers, of this software should refer to the
[docs/user-docs directory](docs/user-docs) which contains user
documentation and scripts for interacting with the software and performing data
analysis.

## Documentation

| Category | Type | Path | Description |
| :--- | :--- | :--- | :--- |
| User | User Guide | [docs/user-docs/users-guide/src/*.md](docs/user-docs/users-guide/src) | Installation, calibration, and troubleshooting |
| User | General | [docs/user-docs/README.md](docs/user-docs/README.md) | Introduction to user package |
| User | Main | [README.md](README.md) | Repository entry point |
| Dev | Architecture | [docs/developer-docs/repository-organization.md](docs/developer-docs/repository-organization.md) | Monorepo structure and components |
| Dev | Component | Various `README.md` | Library APIs and build instructions |
| Dev | Notes | [scratch/*.md](scratch/) | Technical investigations and brainstorming |
| Dev | History | [CHANGELOG.md](CHANGELOG.md) | Versioning and change history |
| Dev | Schema | [braid/braid-types/braidz-schema.md](braid/braid-types/braidz-schema.md) | `braidz` data format specification |
| Dev | Legal | [COPYRIGHT](COPYRIGHT), [LICENSE-APACHE](LICENSE-APACHE), [LICENSE-MIT](LICENSE-MIT), [code_of_conduct.md](code_of_conduct.md) | Licensing and contribution guidelines |


## Discussion

* [Google Group: multi-camera software from the Straw Lab](https://groups.google.com/g/multicams)

## Citation

While a new publication specifically about Braid should be written, in the
meantime, please cite the following paper about the predecessor to Braid:

* Straw AD, Branson K, Neumann TR, Dickinson MH. Multicamera Realtime 3D
  Tracking of Multiple Flying Animals. *Journal of The Royal Society Interface
  8*(11), 395-409 (2011)
  [doi:10.1098/rsif.2010.0230](https://dx.doi.org/10.1098/rsif.2010.0230)

If you additionally make use of 3D tracking of objects under water with cameras
above water (i.e. perform fish tracking), please additionally cite this:

* Stowers JR*, Hofbauer M*, Bastien R, Griessner J⁑, Higgins P⁑, Farooqui S⁑,
  Fischer RM, Nowikovsky K, Haubensak W, Couzin ID,    Tessmar-Raible K✎, Straw
  AD✎. Virtual Reality for Freely Moving Animals. *Nature Methods 14*, 995–1002
  (2017) [doi:10.1038/nmeth.4399](https://dx.doi.org/10.1038/nmeth.4399)

## Installing

Please see the [Installation section of our User
Guide](https://strawlab.github.io/strand-braid/installation.html).

## Building for Development

Please see [docs/developer-docs/building-for-development.md](docs/developer-docs/building-for-development.md) for detailed instructions.

## License

Development of this software is led by Prof. Dr. Andrew Straw at the
University of Freiburg, Germany.

Except where noted otherwise (in individual files and crates), this
open-source software is dual licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option. See [COPYRIGHT](COPYRIGHT) for details. A small number of
crates and files are derived from third-party code and carry different
licenses (for example BSD-2-Clause or BSD-3-Clause), as noted in their
file headers and `Cargo.toml` `license` fields.

## Contributions

Any kinds of contributions are welcome as a pull request.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this software by you, as defined in the Apache-2.0 license,
shall be dual licensed under the terms of the

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

without any additional terms or conditions.

## Code of conduct

Anyone who interacts with this software in any space, including but not limited
to this GitHub repository, must follow our [code of
conduct](code_of_conduct.md).
