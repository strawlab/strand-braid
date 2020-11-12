use rand::distributions::{Normal, IndependentSample};
use ndarray::prelude::*;

// modified from http://rosettacode.org/wiki/Cholesky_decomposition#Rust
fn cholesky<T>(mat: Vec<T>, n: usize) -> Vec<T> where T: num_traits::Float {
    let zero: T = num_traits::cast(0.0).unwrap();
    let one: T = num_traits::cast(1.0).unwrap();

    let mut res: Vec<T> = vec![ zero; mat.len()];
    for i in 0..n {
        for j in 0..(i+1){
            let mut s: T = zero;
            for k in 0..j {
                s = s + res[i * n + k] * res[j * n + k];
            }
            res[i * n + j] = if i == j { (mat[i * n + i] - s).sqrt() } else { (one / res[j * n + j] * (mat[i * n + j] - s)) };
        }
    }
    res
}

fn cholesky_arr<T>(arr: ArrayView2<T>) -> Array2<T> where T: num_traits::Float {
    let n = arr.dim().0;
    assert!(arr.dim().1==n);
    let in_data = arr.iter().map(|x| *x).collect(); // make a C order vec
    let out_data = cholesky(in_data, n);
    Array::from_shape_vec((n,n), out_data).unwrap()
}

pub fn covar<T>(arr: ArrayView2<T>) -> Array2<T> where T: 'static + num_traits::Float + ndarray::ScalarOperand {
    let n = arr.dim().0;

    let mu = arr.mean_axis(Axis(0));
    let y = arr.to_owned()-&mu; // TODO why do I need .to_owned() here to make copy?
    let sigma1 = y.t().dot( &y );
    let scale = 1.0/(n as f64 - 1.0);
    let scale_t: T = num_traits::cast(scale).unwrap();
    sigma1 * scale_t
}

pub fn standard_normal<T>(shape: (usize, usize)) -> Array2<T> where T: num_traits::Float {
    let normal = Normal::new(0.0, 1.0);
    let size = shape.0*shape.1;
    let mut rng = rand::thread_rng();
    let data = (0..size).map(|_| num_traits::cast(normal.ind_sample(&mut rng)).unwrap()).collect();
    Array::from_shape_vec(shape, data).unwrap()
}

pub fn rand_mvn<T>(mu: ArrayView1<T>, sigma: ArrayView2<T>, n: usize) -> Array2<T> where T: 'static + num_traits::Float + std::fmt::Debug {
    let m = mu.len();
    assert!(sigma.dim()==(m,m));
    let norm_data = standard_normal((m,n));
    let sigma_chol = cholesky_arr(sigma);
    let result = sigma_chol.dot(&norm_data).reversed_axes() + mu;
    result
}
