import sympy as sp
from camera_codegen import (
    build_normalized_coordinates,
    build_rodrigues_extrinsics,
    open_codegen_outputs,
    project_single_focal,
    residuals_from_projection,
)
from my_printing import compute_and_print_jacobian

fd_cam, fd_pt = open_codegen_outputs('opencv0', 'OpenCV0')

extrinsics = build_rodrigues_extrinsics()

## Parameters for no-distortion case (u, v are pixel coordinates in image)
u, v, f, cx, cy = sp.symbols('u, v, f, c_x, c_y')

## OpenCV0 model - no distortion at all
# projected pixel coordinate (no distortion)
normalized = build_normalized_coordinates(extrinsics.X, extrinsics.Y, extrinsics.Z)
xp = normalized.xp
yp = normalized.yp

## residual difference between observed image point and projection through our model.
residuals = residuals_from_projection(u, v, project_single_focal(f, cx, cy, xp, yp))

# Finally compute and print components of Jacobian

### 8 camera parameters (f, cx, cy, 3 rotation, 3 translation) - no distortion
parameter_vector_bc = sp.Matrix([
    f,
    cx,
    cy,
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
