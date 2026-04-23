import sympy as sp
from camera_codegen import (
    brown_conrady_distortion,
    build_normalized_coordinates,
    build_rodrigues_extrinsics,
    open_codegen_outputs,
    project_single_focal,
    residuals_from_projection,
)
from my_printing import compute_and_print_jacobian

fd_cam, fd_pt = open_codegen_outputs('opencv4', 'OpenCV4')

extrinsics = build_rodrigues_extrinsics()

## Parameters for no-distortion case (u, v are pixel coordinates in image)
u, v, f, cx, cy = sp.symbols('u, v, f, c_x, c_y')

## Parameters for Brown-Conrady distortion (OpenCV4: only 4 distortion coeffs - no k3)
k1, k2, p1, p2 = sp.symbols('k1, k2, p1, p2')

## Distortion

# projected but undistorted pixel coordinate
normalized = build_normalized_coordinates(extrinsics.X, extrinsics.Y, extrinsics.Z)
xp = normalized.xp
yp = normalized.yp

distorted = brown_conrady_distortion(xp, yp, k1, k2, p1, p2)
xpp = distorted.xpp
ypp = distorted.ypp

## residual difference between observed image point and projection through our model.
residuals = residuals_from_projection(u, v, project_single_focal(f, cx, cy, xpp, ypp))

# Finally compute and print components of Jacobian

### 13 camera parameters (f, cx, cy, 4 distortions, 3 rotation, 3 translation) - no k3
parameter_vector_bc = sp.Matrix([
    f,
    cx,
    cy,
    k1,
    k2,
    p1,
    p2,
    extrinsics.rx,
    extrinsics.ry,
    extrinsics.rz,
    extrinsics.wx,
    extrinsics.wy,
    extrinsics.wz,
])
compute_and_print_jacobian(residuals, parameter_vector_bc, file=fd_cam, title="camera parameters")

# Now compute Jacobian with respect to world point
compute_and_print_jacobian(residuals, extrinsics.pt, file=fd_pt, title="points")
