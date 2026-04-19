import sympy as sp
from sympy.matrices.dense import eye
from types import SimpleNamespace

from my_printing import print_header


def open_codegen_outputs(stem, model_name):
    fd_cam = open(f'scratch/{stem}_cam.rs', 'w')
    fd_pt = open(f'scratch/{stem}_pt.rs', 'w')
    print_header(file=fd_cam, model_name=model_name)
    print_header(file=fd_pt, model_name=model_name)
    return fd_cam, fd_pt


def build_rodrigues_extrinsics():
    rx, ry, rz = sp.symbols('r_x, r_y, r_z')

    rvec = sp.Matrix([rx, ry, rz])
    theta = sp.sqrt(rvec.dot(rvec))

    kx = rx / theta
    ky = ry / theta
    kz = rz / theta

    k = sp.Matrix([[0, -kz, ky], [kz, 0, -kx], [-ky, kx, 0]])
    rotation = eye(3) + sp.sin(theta) * k + (1 - sp.cos(theta)) * k * k

    wx, wy, wz, px, py, pz = sp.symbols('w_x, w_y, w_z, p_x, p_y, p_z')
    cam = sp.Matrix([wx, wy, wz])
    pt = sp.Matrix([px, py, pz])

    full_x = rotation * (pt - cam)
    return SimpleNamespace(
        rotation=rotation,
        rx=rx,
        ry=ry,
        rz=rz,
        wx=wx,
        wy=wy,
        wz=wz,
        px=px,
        py=py,
        pz=pz,
        cam=cam,
        pt=pt,
        X=full_x[0, 0],
        Y=full_x[1, 0],
        Z=full_x[2, 0],
    )


def build_normalized_coordinates(x_coord, y_coord, z_coord):
    xp = x_coord / z_coord
    yp = y_coord / z_coord
    return SimpleNamespace(xp=xp, yp=yp, r2=sp.Matrix([xp, yp]).dot(sp.Matrix([xp, yp])))


def brown_conrady_distortion(xp, yp, k1, k2, p1, p2, k3=None):
    r2 = sp.Matrix([xp, yp]).dot(sp.Matrix([xp, yp]))
    radial = 1 + k1 * r2 + k2 * r2 * r2
    if k3 is not None:
        radial += k3 * r2 * r2 * r2

    xpp = xp * radial + 2 * p1 * xp * yp + p2 * (r2 + 2 * xp * xp)
    ypp = yp * radial + p1 * (r2 + 2 * yp * yp) + 2 * p2 * xp * yp
    return SimpleNamespace(xpp=xpp, ypp=ypp, r2=r2)


def project_single_focal(f, cx, cy, xp, yp):
    return sp.Matrix([f * xp + cx, f * yp + cy])


def project_dual_focal(fx, fy, cx, cy, xp, yp):
    return sp.Matrix([fx * xp + cx, fy * yp + cy])


def residuals_from_projection(u, v, pixel_coord):
    return sp.Matrix([u - pixel_coord[0], v - pixel_coord[1]])


def build_extrinsics_only_symbolic_model():
    extrinsics = build_rodrigues_extrinsics()
    u, v, fx, fy, cx, cy = sp.symbols('u, v, fx, fy, c_x, c_y')
    k1, k2, p1, p2, k3 = sp.symbols('k1, k2, p1, p2, k3')

    normalized = build_normalized_coordinates(extrinsics.X, extrinsics.Y, extrinsics.Z)
    distorted = brown_conrady_distortion(
        normalized.xp,
        normalized.yp,
        k1,
        k2,
        p1,
        p2,
        k3=k3,
    )
    pixel_coord = project_dual_focal(fx, fy, cx, cy, distorted.xpp, distorted.ypp)
    residuals = residuals_from_projection(u, v, pixel_coord)

    return SimpleNamespace(
        extrinsics=extrinsics,
        u=u,
        v=v,
        fx=fx,
        fy=fy,
        cx=cx,
        cy=cy,
        k1=k1,
        k2=k2,
        p1=p1,
        p2=p2,
        k3=k3,
        normalized=normalized,
        distorted=distorted,
        pixel_coord=pixel_coord,
        residuals=residuals,
        param_syms=sp.Matrix([
            extrinsics.rx,
            extrinsics.ry,
            extrinsics.rz,
            extrinsics.wx,
            extrinsics.wy,
            extrinsics.wz,
        ]),
        all_syms=[
            fx,
            fy,
            cx,
            cy,
            k1,
            k2,
            p1,
            p2,
            k3,
            extrinsics.rx,
            extrinsics.ry,
            extrinsics.rz,
            extrinsics.wx,
            extrinsics.wy,
            extrinsics.wz,
            extrinsics.px,
            extrinsics.py,
            extrinsics.pz,
            u,
            v,
        ],
    )