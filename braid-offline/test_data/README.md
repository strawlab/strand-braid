# Workflow2 for retracking:

# For 3D multi-camera data

See `retrack-demo.sh`.

# For 2D single-camera data

```
cargo run --no-default-features --features "flat-3d" --bin braid-offline-retrack --  -d test_data/20180330_113743.short -o /tmp/k2d.braidz
python test_data/plot_csv_dir.py /tmp/k2d
```

**Also, see tests in `flydra2/tests`.**
