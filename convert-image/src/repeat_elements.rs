use std::iter::Iterator;

pub struct RepeatElements<'a, ITER, T>
where
    ITER: Iterator<Item = &'a T>,
    T: 'a,
{
    inner: ITER,
    current_element: T,
    count: usize,
    num_repeats: usize,
}

impl<'a, ITER, T> std::iter::Iterator for RepeatElements<'a, ITER, T>
where
    ITER: Iterator<Item = &'a T>,
    T: 'a + Copy,
{
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.count += 1;
        if self.count >= self.num_repeats {
            let next_element = self.inner.next();
            match next_element {
                Some(el) => {
                    self.current_element = *el;
                    self.count = 0;
                    Some(self.current_element)
                }
                None => None,
            }
        } else {
            Some(self.current_element)
        }
    }
}

pub trait RepeatElement<'a, ITER, T>
where
    ITER: Iterator<Item = &'a T>,
    T: 'a + Copy,
{
    fn repeat_elements(self, num_repeats: usize) -> RepeatElements<'a, ITER, T>;
}

impl<'a, ITER, T> RepeatElement<'a, ITER, T> for ITER
where
    ITER: Iterator<Item = &'a T>,
    T: 'a + Copy + Default,
{
    #[inline]
    fn repeat_elements(self, num_repeats: usize) -> RepeatElements<'a, ITER, T> {
        RepeatElements {
            inner: self,
            current_element: T::default(), // dummy value
            count: num_repeats,            // force recompute on first .next() call
            num_repeats,
        }
    }
}

#[test]
fn test_repeat_element() {
    let orig: Vec<u8> = vec![1, 2, 3];
    let repeated: Vec<u8> = orig.iter().repeat_elements(3).collect();
    assert_eq!(repeated.as_slice(), &[1, 1, 1, 2, 2, 2, 3, 3, 3]);
}

#[test]
fn test_repeat_element_empty() {
    let orig: Vec<u8> = vec![];
    assert_eq!(orig.iter().repeat_elements(3).next(), None);
}
