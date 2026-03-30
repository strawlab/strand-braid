#[derive(PartialEq, Eq)]
pub(crate) struct Peek2<I: std::iter::Iterator> {
    inner: I,
    slot1: Option<I::Item>,
    slot2: Option<I::Item>,
}

impl<I: std::iter::Iterator> Peek2<I> {
    pub(crate) fn new(mut inner: I) -> Self {
        let slot1 = inner.next();
        if slot1.is_some() {
            let slot2 = inner.next();
            Self {
                inner,
                slot1,
                slot2,
            }
        } else {
            Self {
                inner,
                slot1,
                slot2: None,
            }
        }
    }
    // pub(crate) fn as_ref(&self) -> &I {
    //     &self.inner
    // }
    pub(crate) fn peek1(&self) -> Option<&I::Item> {
        self.slot1.as_ref()
    }
    pub(crate) fn peek2(&self) -> Option<&I::Item> {
        self.slot2.as_ref()
    }
    pub(crate) fn next(&mut self) -> Option<I::Item> {
        let cur_slot2 = self.slot2.take();
        let next_slot2 = if cur_slot2.is_some() {
            self.inner.next()
        } else {
            None
        };
        let result = self.slot1.take();
        self.slot1 = cur_slot2;
        self.slot2 = next_slot2;
        result
    }
}

#[test]
fn test_peek2() {
    let a = [1, 2, 3];
    let mut iter = Peek2::new(a.iter());
    assert_eq!(iter.peek1(), Some(&&1));
    assert_eq!(iter.peek2(), Some(&&2));

    assert_eq!(iter.next(), Some(&1));
    assert_eq!(iter.peek1(), Some(&&2));
    assert_eq!(iter.peek2(), Some(&&3));

    assert_eq!(iter.next(), Some(&2));
    assert_eq!(iter.peek1(), Some(&&3));
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), Some(&3));
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), None);
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    // ---
    let a = [1, 2];
    let mut iter = Peek2::new(a.iter());
    assert_eq!(iter.peek1(), Some(&&1));
    assert_eq!(iter.peek2(), Some(&&2));

    assert_eq!(iter.next(), Some(&1));
    assert_eq!(iter.peek1(), Some(&&2));
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), Some(&2));
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), None);
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    // ---
    let a = [1];
    let mut iter = Peek2::new(a.iter());
    assert_eq!(iter.peek1(), Some(&&1));
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), Some(&1));
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), None);
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    // ---
    let a: Vec<i32> = vec![];
    let mut iter = Peek2::new(a.iter());
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);

    assert_eq!(iter.next(), None);
    assert_eq!(iter.peek1(), None);
    assert_eq!(iter.peek2(), None);
}
