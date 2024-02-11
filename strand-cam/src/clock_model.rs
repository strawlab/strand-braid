use nalgebra as na;

use crate::Result;

pub(crate) fn fit_time_model(past_data: &[(f64, f64)]) -> Result<(f64, f64, f64)> {
    use na::{OMatrix, OVector, U2};

    let mut a: Vec<f64> = Vec::with_capacity(past_data.len() * 2);
    let mut b: Vec<f64> = Vec::with_capacity(past_data.len());

    for row in past_data.iter() {
        a.push(row.0);
        a.push(1.0);
        b.push(row.1);
    }
    let a = OMatrix::<f64, na::Dyn, U2>::from_row_slice(&a);
    let b = OVector::<f64, na::Dyn>::from_row_slice(&b);

    let epsilon = 1e-10;
    let results = lstsq::lstsq(&a, &b, epsilon)
        .map_err(|msg| crate::StrandCamError::ClockModelFitError(msg.into()))?;

    let gain = results.solution[0];
    let offset = results.solution[1];
    let residuals = results.residuals;

    Ok((gain, offset, residuals))
}

#[test]
fn test_fit_time_model() {
    let epsilon = 1e-12;

    let data = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
    let (gain, offset, _residuals) = fit_time_model(&data).unwrap();
    assert!((gain - 1.0).abs() < epsilon);
    assert!((offset - 0.0).abs() < epsilon);

    let data = vec![(0.0, 12.0), (1.0, 22.0), (2.0, 32.0), (3.0, 42.0)];
    let (gain, offset, _residuals) = fit_time_model(&data).unwrap();
    assert!((gain - 10.0).abs() < epsilon);
    assert!((offset - 12.0).abs() < epsilon);
}
