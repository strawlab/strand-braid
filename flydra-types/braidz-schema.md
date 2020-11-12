# Braidz schema

This document describes the changes to the braidz on-disk schema.

## 2

In v2, we introduced the files `reconstruct_latency_usec.hlog` and `reprojection_distance_100x_pixels.hlog` which are in the hdrHistogram format. It is otherwise exactly identical.

## 1

This is the initial release after porting from the .h5 format with the Python API. It is as close as possible to a one-to-one conversion.
