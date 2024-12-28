use bisection_search::{BisectionSearch, Interval};
use simba::scalar::RealField;

/// Define the parameters required to solve a refraction problem.
///
/// In the following drawing, we want to find `x` when we know `w`, `h`, `d`,
/// and the ratio of refractive indices `n` where `n = n2/n1`.
///
/// `a` and `b` are angles, `n1` and `n2` are refractive indices, `d`, `h` and
/// `w` are lengths.
///
/// ```text
///                                                      ---
///                                                 ----/ a|
///               n1                           ----/       |
///                                       ----/            |
///                                 -----/                 |
///                            ----/                       | w
///                       ----/                            |
///                  ----/                                 |
///           x   --/                    d-x               |
///      |--------|----------------------------------------|
///      |       /
///      | h     |
///      |      /
///      |     /
///      |    /                   n2
///      |    |
///      |   /
///      |  /
///      | /
///      |b|
///      |/
///
///    sin(a)   n2
///    ------ = -- = n
///    sin(b)   n1
/// ```
///
/// See [this](https:///math.stackexchange.com/questions/150769) for an
/// explanation of the problem and this solution. Ascii drawing done with
/// [textik.com](https://textik.com).
#[derive(Clone)]
pub struct RefractionEq<T> {
    /// Distance along the refractive boundary
    pub d: T,
    /// Height below the refractive boundary
    pub h: T,
    /// Height above the refractive boundary
    pub w: T,
    /// Ratio of refractive indices
    pub n: T,
}

impl<T: RealField + Copy> RefractionEq<T> {
    /// Evaluate the refraction equation at location `x`.
    pub fn f(&self, x: T) -> T {
        let RefractionEq { d, h, w, n } = self.clone();

        let d_minus_x = d - x;
        d_minus_x * (x * x + h * h).sqrt() / (x * (d_minus_x * d_minus_x + w * w).sqrt()) - n
    }
}

pub fn find_root<T>(a: T, b: T, eq: RefractionEq<T>, tolerance: T) -> Option<T>
where
    T: RealField + Copy,
{
    let interval = match Interval::new(a, b) {
        Some(i) => i,
        None => return None,
    };

    let mut bisect = BisectionSearch::new(interval, |x| eq.f(*x));
    loop {
        bisect = bisect.step();
        if bisect.interval.size() < tolerance {
            break;
        }
    }
    return Some(*bisect.interval.a());
}

#[cfg(test)]
mod tests {
    #[test]
    fn example1() {
        // https://math.stackexchange.com/questions/150769/refraction-equation-quartic-equation/151249#comment347929_151050
        // https://www.wolframalpha.com/input/?i=plot+%28%28d-x%29%2Fsqrt%28%28d-x%29%5E2%2Bw%5E2%29%29%2F%28x%2Fsqrt%28x%5E2%2Bh%5E2%29%29+-+1.33+where+w%3D1%2C+h%3D5%2C+d%3D30+for+x%3D0..30
        let f = crate::RefractionEq {
            d: 30.0,
            h: 5.0,
            w: 1.0,
            n: 1.33,
        };
        let eps = 0.01;
        assert!(f64::abs(f.f(5.7)) < eps);
    }

    #[test]
    fn test_derivative() {
        let refraction = crate::RefractionEq {
            d: 30.0,
            h: 5.0,
            w: 1.0,
            n: 1.33,
        };

        // Evaluate until a given tolerance.
        let val = crate::find_root(
            0.0, 30.0, // Initial guess
            refraction, 1e-10,
        )
        .unwrap();
        let eps = 0.01;
        assert!(f64::abs(val - 5.7) < eps);
    }
}
