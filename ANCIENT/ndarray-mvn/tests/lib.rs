use ndarray::prelude::*;
use ndarray_mvn::{rand_mvn, covar};

#[test]
fn test_mvn1() {
    let mu = arr1(&[1.0, 2.0, 3.0, 4.0]);
    let sigma = arr2(&[[2.0, 0.1, 0.0, 0.0],
                        [0.1, 0.2, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0]]);
    let n = 1000;
    let y = rand_mvn(mu.view(),sigma.view(),n);
    assert!(y.shape()==&[n,4]);
    let mu2 = y.mean_axis(Axis(0));
    let eps = 0.2;
    assert!(mu.all_close(&mu2,eps)); // expect occasional failures here

    let sigma2 = covar(y.view());
    let eps = 0.2;
    assert!(sigma.all_close(&sigma2,eps)); // expect occasional failures here
}
