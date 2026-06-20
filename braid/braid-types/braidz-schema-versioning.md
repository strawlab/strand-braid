# Braidz schema versioning

This document describes the versioning policy for the braidz on-disk schema
and the history of changes to it. For the schema itself, the current
implementation in code is ground truth.

## Versioning policy

The schema version (`BRAID_SCHEMA`) is versioned like semver major versions: it
is bumped only on a **backward-incompatible** change to the on-disk format — one
that would prevent an existing reader from correctly parsing a newly written
file. Removing or renaming a file, removing a CSV column or struct field, or
changing the type or meaning of an existing one all require a bump.

Purely **additive** changes — a new file, a new optional CSV column, or a new
`#[serde(default)]` struct field — are backward compatible and do **not** bump
the schema, because existing readers continue to work unchanged.

## Version history

### 3

Note: both changes below were purely additive, so under the current
[versioning policy](#versioning-policy) they would not warrant a schema bump.
The version was bumped to 3 before that policy was adopted; the number is
retained for historical accuracy.

In v3, two changes were made:

- `data2d_distorted.csv` gained two columns: `device_timestamp` and `block_id`,
  the timestamp and frame number reported directly by the camera. Both are
  optional (empty when the camera does not provide them).
- The `BraidMetadata` struct (stored in `braid_metadata.yml`) gained a
  `saving_program_name` field recording the name of the program that wrote the
  file. When loading older files that lack this field, it defaults to the empty
  string (`""`).

It is otherwise identical to v2.

### 2

In v2, we introduced the files `reconstruct_latency_usec.hlog` and `reprojection_distance_100x_pixels.hlog` which are in the hdrHistogram format. It is otherwise exactly identical.

### 1

This is the initial release after porting from the .h5 format with the Python API. It is as close as possible to a one-to-one conversion.
