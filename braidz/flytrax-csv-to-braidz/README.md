# flytrax-csv-to-braidz

Convert 2D csv files from strand cam into tracks in .braidz file by tracking

## Installation

```text
# install rust from https://rustup.rs/
git clone https://github.com/strawlab/strand-braid
cd strand-braid/flytrax-csv-to-braidz/
cargo install --path .
```

## Running

```text
cargo run -- --cal .\tests\data\cal1.toml --csv .\tests\data\flytrax20191122_103500.csv
```

## Plotting

You can view .braidz files with the Python scripts in
https://github.com/strawlab/strand-braid/tree/main/strand-braid-user/analysis.

For example:

```text
python braid-analysis-plot3d.py flytrax20200609_161115.braidz
```

## Testing

```text
cargo test
```
