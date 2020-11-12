# mvg multi view geometry

Run tests with:

    cargo test

## TODO

- Make intrinsic parameters a trait so that different models can exist.
- Make 2D and 3D coordinate types unified and use PhantomData on an enum to
distinguish world and camera (3D) or distorted and undistorted (2D)
- Vectorize all operations (allows e.g. fearless_simd to be used)
- Pixel -> 3D transformations should return ncollide rays (rather than points)
- Make extrinsic a trait so +z forward and -z forward cameras can co-exist
- Use `mint` for external api (rather than nalgebra)?
- Remove all "water" handling into separate crate. Especially important since I
  switched the signature of `project_3d_to_pixel()` to return `T` instead of
  `Result<T>`, which requires potentially `panic!`ing now (in `expect()`). See
  tag "laksdfjasl".
