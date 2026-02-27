use simba::scalar::RealField;

#[derive(Debug, Clone)]
pub struct Interval<T> {
    a: T,
    b: T,
}

impl<T: RealField + Copy> Interval<T> {
    pub fn new(a: T, b: T) -> Option<Self> {
        if b >= a {
            Some(Self { a, b })
        } else {
            None
        }
    }
}

impl<T> Interval<T> {
    pub fn a(&self) -> &T {
        &self.a
    }
    pub fn b(&self) -> &T {
        &self.b
    }
}

impl<T: RealField + Copy> Interval<T> {
    pub fn new_from_range<RB: std::ops::RangeBounds<T>>(range: RB) -> Option<Self> {
        if let std::ops::Bound::Included(start) = range.start_bound()
            && let std::ops::Bound::Included(end) = range.end_bound() {
                return Interval::new(*start, *end);
            }
        None
    }
}

impl<T: RealField + Copy> Interval<T> {
    pub fn size(&self) -> T {
        self.b - self.a
    }
}

#[derive(Clone)]
pub struct BisectionSearch<T, F>
where
    F: Fn(&T) -> T,
{
    pub interval: Interval<T>,
    fa: T,
    fb: T,
    f: F,
}

impl<T, F> BisectionSearch<T, F>
where
    F: Fn(&T) -> T,
{
    pub fn new(interval: Interval<T>, f: F) -> Self {
        let fa = f(&interval.a);
        let fb = f(&interval.b);
        Self {
            interval,
            fa,
            fb,
            f,
        }
    }
}

impl<T, F> BisectionSearch<T, F>
where
    T: RealField + Copy,
    F: Fn(&T) -> T,
{
    pub fn step(mut self) -> Self {
        let two = T::one() + T::one();
        let c = (self.interval.a + self.interval.b) / two;
        let fc = (self.f)(&c);
        if fc == T::zero() {
            return BisectionSearch {
                interval: Interval { a: c, b: c },
                fa: fc,
                fb: fc,
                f: self.f,
            };
        }

        if self.fa.is_sign_positive() != fc.is_sign_positive() {
            self.interval.b = c;
            self.fb = fc;
            return self;
        }

        self.interval.a = c;
        self.fa = fc;
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn wikipedia_example() {
        // example at https://en.wikipedia.org/wiki/Bisection_method
        let mut bisect =
            BisectionSearch::new(Interval::new(1.0, 2.0).unwrap(), |x| x * x * x - x - 2.0);

        for _ in 0..15 {
            dbg!((&bisect.interval.a(), &bisect.interval.b()));
            bisect = bisect.step();
        }

        assert!(f64::abs(bisect.interval.a - 1.521) < 0.001);
    }
}
