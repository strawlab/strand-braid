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
[strand-braid-user directory](strand-braid-user) which contains user
documentation and scripts for interacting with the software and performing data
analysis.

## Documentation

* [User's Guide](https://strawlab.github.io/strand-braid/)

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

## Building

### Prerequisites

[Install rust](https://rustup.rs/).

[Install wasm-pack](https://rustwasm.github.io/wasm-pack/installer/).

Install your camera drivers. Currently Basler Pylon and Allied Vision Vimba are
supported.

First checkout the git repository into a location which will below be called
`/path/to/strand-braid`:

```
cd /path/to # <---- change this to a suitable filesystem directory
git clone https://github.com/strawlab/strand-braid
cd strand-braid # now in /path/to/strand-braid
```

### Strand Camera

First, build the browser user interface (BUI) for Strand Camera. This will build
files in `strand-cam/yew_frontend/pkg` which get included in the Strand Cam
executable:

```
cd /path/to/strand-braid/strand-cam/yew_frontend
wasm-pack build --target web
```

Then, build the Strand Cam executable for Basler cameras using the Pylon
drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-pylon
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-pylon
```

Alternatively or additionally, build the Strand Cam executable for Allied Vision
cameras using the Vimba drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-vimba
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-vimba
```

Many compile-time options exist to adjust the exact features used, but the
instructions above should build a working copy of Strand Camera albeit with
potentially reduced features and performance.

### Braid

We will build `braid-run` which is the main runtime application we call "Braid".

First, build the browser user interface (BUI) for Braid. This will build files
in `braid/braid-run/braid_frontend/pkg` which get included in the `braid-run`
executable:

```
cd /path/to/strand-braid/braid/braid-run/braid_frontend
wasm-pack build --target web
```

Then, build the `braid-run` executable:

```
cd /path/to/strand-braid/braid/braid-run
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/braid-run
```


## License

This software is developed by Prof. Dr. Andrew Straw at the University of
Freiburg, Germany.

This open-source software is distributable under the terms of the Affero General
Public License v1.0 only. See [COPYRIGHT](COPYRIGHT) and
[LICENSE.txt](LICENSE.txt) for more details.

## Future license plans

We have a goal to release many of the generally useful crates under licenses
such as the MIT license, the Apache License (Version 2.0), and BSD-like
licenses. Please get in touch if there are specific pieces of code where this
would be helpful so we can judge interest and prioritize this.

## Contributions

Any kinds of contributions are welcome as a pull request.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this software by you, as defined in the Apache-2.0 license,
shall be dual licensed under the terms of the

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

without any additional terms or conditions. (This helps us realize the future
license plans as described above.)

## Code of conduct

Anyone who interacts with this software in any space, including but not limited
to this GitHub repository, must follow our [code of
conduct](code_of_conduct.md).
