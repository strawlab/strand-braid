pub(crate) trait Argmin<T> {
    fn argmin(&mut self) -> Option<usize>;
}

impl<T: std::cmp::PartialOrd> Argmin<T> for std::slice::Iter<'_, T> {
    fn argmin(&mut self) -> Option<usize> {
        if let Some(mut current_min) = self.next() {
            let mut idx = 0usize;
            let mut best_idx = 0usize;
            for value in self {
                idx += 1;
                if value < current_min {
                    best_idx = idx;
                    current_min = value;
                }
            }
            Some(best_idx)
        } else {
            // no data in iterator
            None
        }
    }
}

#[test]
fn test_argmin() {
    assert_eq!(Vec::<i64>::new().iter().argmin(), None);
    assert_eq!(vec![1, 2, 3].iter().argmin(), Some(0));
    assert_eq!(vec![3, 2, 1].iter().argmin(), Some(2));
    assert_eq!(vec![3, -2, 1].iter().argmin(), Some(1));
}
