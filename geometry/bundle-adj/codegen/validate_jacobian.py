"""Validate the shared extrinsics-only symbolic Jacobian against finite differences."""

import numpy as np
import sympy as sp

from camera_codegen import build_extrinsics_only_symbolic_model

model = build_extrinsics_only_symbolic_model()

# The parameter vector under test
param_syms = model.param_syms
param_names = ['rx', 'ry', 'rz', 'wx', 'wy', 'wz']

# ── Nominal parameter values ───────────────────────────────────────────────────
#  (choose values far from zero to avoid degenerate Rodrigues singularity)
nominal = {
    model.fx: 800.0,
    model.fy: 820.0,
    model.cx: 320.0,
    model.cy: 240.0,
    model.k1: 0.01,
    model.k2: 0.001,
    model.p1: -0.002,
    model.p2: 0.0005,
    model.k3: 0.0,
    model.extrinsics.rx: 0.1,
    model.extrinsics.ry: 0.4,
    model.extrinsics.rz: -0.2,
    model.extrinsics.wx: 1.0,
    model.extrinsics.wy: 0.5,
    model.extrinsics.wz: -2.0,
    model.extrinsics.px: 0.3,
    model.extrinsics.py: -0.7,
    model.extrinsics.pz: 5.0,
    model.u: 305.0,
    model.v: 198.0,
}

# ── Build fast numerical functions via lambdify ────────────────────────────────
all_syms = model.all_syms

resid_fn = sp.lambdify(all_syms, model.residuals, 'numpy')
J_sym = model.residuals.jacobian(param_syms)
J_fn = sp.lambdify(all_syms, J_sym, 'numpy')


def eval_at(subs: dict):
    vals = [float(subs[s]) for s in all_syms]
    r = resid_fn(*vals)
    return np.array(r, dtype=float).flatten()


def symbolic_jacobian_at(subs: dict):
    vals = [float(subs[s]) for s in all_syms]
    J = J_fn(*vals)
    return np.array(J, dtype=float)


def finite_diff_jacobian_at(subs: dict, h: float = 1e-6):
    r0 = eval_at(subs)
    J = np.zeros((2, len(param_syms)))
    for j, sym in enumerate(param_syms):
        subs_p = dict(subs)
        subs_p[sym] += h
        subs_m = dict(subs)
        subs_m[sym] -= h
        J[:, j] = (eval_at(subs_p) - eval_at(subs_m)) / (2 * h)
    return J


# ── Print projected pixel coordinates at nominal values ───────────────────────
proj_x_fn = sp.lambdify(all_syms, model.pixel_coord[0], 'numpy')
proj_y_fn = sp.lambdify(all_syms, model.pixel_coord[1], 'numpy')

nominal_vals = [float(nominal[s]) for s in all_syms]
print(f"Projected pixel coordinates at nominal values:")
print(f"  fx*xpp + cx = {float(proj_x_fn(*nominal_vals)):.10f}")
print(f"  fy*ypp + cy = {float(proj_y_fn(*nominal_vals)):.10f}")
print()

# ── Compare ───────────────────────────────────────────────────────────────────
print("Evaluating symbolic and finite-difference Jacobians at nominal values...\n")

J_sym_val = symbolic_jacobian_at(nominal)
J_fd_val = finite_diff_jacobian_at(nominal)

print(f"{'':8s} {'Symbolic':>14s}  {'FD estimate':>14s}  {'Abs error':>12s}  {'Rel error':>12s}  {'OK?':>5s}")
print("-" * 75)

all_ok = True
tol_rel = 1e-4   # 0.01 % relative tolerance
tol_abs = 1e-6   # floor for near-zero entries

for i in range(2):
    for j, name in enumerate(param_names):
        s = J_sym_val[i, j]
        f = J_fd_val[i, j]
        abs_err = abs(s - f)
        rel_err = abs_err / (abs(s) + 1e-15)
        ok = abs_err < tol_abs or rel_err < tol_rel
        if not ok:
            all_ok = False
        flag = "OK" if ok else "FAIL <<"
        print(
            f"J[{i},{j}] ({name:>4s}):  {s:14.6f}  {f:14.6f}  {abs_err:12.2e}  {rel_err:12.2e}  {flag}"
        )
    print()

print("=" * 75)
if all_ok:
    print("All entries match within tolerance.")
else:
    print("MISMATCHES detected — symbolic Jacobian does not agree with finite differences.")
