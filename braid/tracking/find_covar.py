from __future__ import division
import sympy
from sympy import Symbol, Matrix, sqrt, latex, lambdify, simplify, pprint
from sympy.utilities.lambdify import lambdastr
from sympy.solvers import solve
import numpy as np

sympy.init_printing(use_unicode=True)

dt = Symbol('dt')
sigma = Symbol('sigma')

def A(dt):
    return Matrix([[1, dt],
                [0, 1]])

# the trick is to find Q such that P1_2 and P2_2 are equal
def Q(dt):
    t3 = dt**3/3
    t2 = dt**2/2
    return sigma*Matrix([[t3,t2],[t2,dt]])

# initial covariance
p00 = Symbol('p00')
p01 = Symbol('p01')
p10 = Symbol('p10')
p11 = Symbol('p11')
#P0 = Matrix([[p00,0],[0,p11]])
P0 = Matrix([[p00,p01],[p10,p11]])

P1_1 = ((A(dt)*P0)*A(dt).T) + Q(dt)

P1_2 = ((A(dt)*P1_1)*A(dt).T) + Q(dt)
print 'P1_2'
pprint (simplify(P1_2))

P2_2 = ((A(2*dt)*P0)*A(2*dt).T) + Q(2*dt)
print 'P2_2'
pprint(simplify(P2_2))

for i in range(2):
    for j in range(2):
        assert(simplify(P1_2[i,j]-P2_2[i,j])==0)
