import sympy as sp
from camera_codegen import (
    brown_conrady_distortion,
    build_normalized_coordinates,
    build_rodrigues_extrinsics,
    open_codegen_outputs,
    project_dual_focal,
    residuals_from_projection,
)
from my_printing import compute_and_print_jacobian

fd_cam, fd_pt = open_codegen_outputs('extrinsics_only', 'ExtrinsicsOnly')

extrinsics = build_rodrigues_extrinsics()

## Parameters for no-distortion case (u, v are pixel coordinates in image)
u, v, fx, fy, cx, cy = sp.symbols('u, v, fx, fy, c_x, c_y')

## Parameters for Brown-Conrady distortion (we leave out k3 for back-compat with MCSC/Flydra/Braid)
k1, k2, p1, p2, k3 = sp.symbols('k1, k2, p1, p2, k3')

# projected but undistorted pixel coordinate
normalized = build_normalized_coordinates(extrinsics.X, extrinsics.Y, extrinsics.Z)
xp = normalized.xp
yp = normalized.yp

distorted = brown_conrady_distortion(xp, yp, k1, k2, p1, p2, k3=k3)
xpp = distorted.xpp
ypp = distorted.ypp

## residual difference between observed image point and projection through our model.
residuals = residuals_from_projection(u, v, project_dual_focal(fx, fy, cx, cy, xpp, ypp))

# Finally compute and print components of Jacobian

### 6 camera parameters (3 rotation, 3 translation)
parameter_vector_bc = sp.Matrix([
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
