use std::collections::{BTreeMap, VecDeque};
use withkey::WithKey;

// TODO: better error handling. Do not wrap Result types, but handle Results automatically.

/// A type which reads ahead by `bufsize` elements and sorts within.
pub struct BufferedSortIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    single_iter: I,
    sorted_buf: BTreeMap<K, VecDeque<T>>,
    done_reading: bool,
    highest_key: Option<K>,
}

impl<K: std::cmp::Ord + Clone, I, T, E: std::fmt::Debug> BufferedSortIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    pub fn new(single_iter: I, bufsize: usize) -> Result<Self, E> {
        let sorted_buf = BTreeMap::new();
        let mut result = Self {
            single_iter,
            sorted_buf,
            done_reading: false,
            highest_key: None,
        };

        let mut count = 0;
        while count < bufsize {
            count += 1;
            if !result.read_next()? {
                break;
            }
        }
        Ok(result)
    }

    pub fn get_ref(&self) -> &I {
        &self.single_iter
    }

    pub fn into_inner(self) -> I {
        self.single_iter
    }

    // read a single item from the iterator. returns true if should read again.
    #[inline]
    fn read_next(&mut self) -> Result<bool, E> {
        if self.done_reading {
            return Ok(false);
        }
        match self.single_iter.next() {
            None => {
                self.done_reading = true;
                Ok(false)
            }
            Some(result_el) => {
                let el = result_el?;
                let key = el.key();
                let rows_entry = &mut self.sorted_buf.entry(key).or_default();
                rows_entry.push_back(el);
                Ok(true)
            }
        }
    }

    // get the first item, return whether the key must be removed and the item
    #[inline]
    fn empty_first(&mut self) -> Option<std::result::Result<T, E>> {
        let mut remove_key = None;
        let result: T = {
            let mut first = self.sorted_buf.iter_mut().next();
            match first {
                None => return None,
                Some((this_key, ref mut this_el_vec)) => {
                    if let Some(ref hk) = self.highest_key {
                        assert!(this_key >= hk, "failed to sort data (bufsize too small?)");
                    }
                    self.highest_key = Some(this_key.clone());

                    let first_el = this_el_vec.pop_front();
                    if this_el_vec.is_empty() {
                        remove_key = Some(this_key.clone());
                    }
                    first_el.unwrap()
                }
            }
        };

        if let Some(rk) = remove_key {
            self.sorted_buf.remove(&rk);
        }

        Some(Ok(result))
    }
}

impl<
        K: std::cmp::Ord + std::fmt::Debug + std::cmp::PartialEq + std::cmp::PartialOrd + Clone,
        I,
        T,
        E: std::fmt::Debug,
    > Iterator for BufferedSortIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    type Item = std::result::Result<T, E>;
    fn next(&mut self) -> std::option::Option<<Self as Iterator>::Item> {
        match self.read_next() {
            Ok(_) => {}
            Err(e) => {
                // The error may be desynchronized somewhat from element in iterator
                // from which it came, but at least it gets
                // propagated up without panic.
                return Some(Err(e));
            }
        };
        self.empty_first()
    }
}

/// A type takes a sorted iterator and returns grouped results.
///
/// Panics if the input iterator is not sorted.
pub struct AscendingGroupIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    single_iter: I,
    peek: Option<std::result::Result<T, E>>,
    key_type: std::marker::PhantomData<K>,
}

impl<K, I, T, E> AscendingGroupIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    pub fn new(mut single_iter: I) -> Self {
        let peek = single_iter.next();
        Self {
            single_iter,
            peek,
            key_type: std::marker::PhantomData,
        }
    }

    pub fn get_ref(&self) -> &I {
        &self.single_iter
    }

    pub fn into_inner(self) -> I {
        self.single_iter
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GroupedRows<K, T: WithKey<K>> {
    pub group_key: K,
    pub rows: Vec<T>,
}

impl<K: std::fmt::Debug + std::cmp::PartialEq + std::cmp::PartialOrd, I, T: WithKey<K>, E> Iterator
    for AscendingGroupIter<K, I, T, E>
where
    I: Iterator<Item = std::result::Result<T, E>>,
    T: WithKey<K>,
{
    type Item = std::result::Result<GroupedRows<K, T>, E>;
    fn next(&mut self) -> std::option::Option<<Self as Iterator>::Item> {
        match self.peek.take() {
            None => None, // no more rows, return None
            Some(row_result) => {
                match row_result {
                    Ok(next_seed) => {
                        // first row of new group_key
                        let mut item = GroupedRows {
                            group_key: next_seed.key(),
                            rows: vec![next_seed],
                        };
                        loop {
                            // more rows of new group_key until seed for next
                            match self.single_iter.next() {
                                None => break, // no more rows in file, finish this group_key
                                Some(result_val) => {
                                    match result_val {
                                        Ok(val) => {
                                            // got another row
                                            if val.key() == item.group_key {
                                                item.rows.push(val);
                                            } else {
                                                if val.key().partial_cmp(&item.group_key)
                                                    != Some(std::cmp::Ordering::Greater)
                                                {
                                                    panic!("key is not monotonically ascending ({:?} < {:?})",
                                                        val.key(), item.group_key);
                                                }
                                                self.peek = Some(Ok(val));
                                                break;
                                            }
                                        }
                                        Err(e) => return Some(Err(e)),
                                    }
                                }
                            }
                        }
                        Some(Ok(item))
                    }
                    Err(e) => Some(Err(e)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(PartialEq)]
    struct Foo {
        x: u8,
    }
    impl WithKey<u8> for Foo {
        fn key(&self) -> u8 {
            self.x
        }
    }

    #[test]
    fn groupby_empty() {
        let foos: Vec<Foo> = vec![];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut group_iter = AscendingGroupIter::new(foos_iter);
        assert!(group_iter.next().is_none());
    }

    #[test]
    fn groupby_monotonic() {
        let foos = vec![
            Foo { x: 1 },
            Foo { x: 1 },
            Foo { x: 2 },
            Foo { x: 2 },
            Foo { x: 3 },
        ];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut group_iter = AscendingGroupIter::new(foos_iter);
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 1,
                    rows: vec![Foo { x: 1 }, Foo { x: 1 }]
                }))
        );
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 2,
                    rows: vec![Foo { x: 2 }, Foo { x: 2 }]
                }))
        );
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 3,
                    rows: vec![Foo { x: 3 }]
                }))
        );
        assert!(group_iter.next().is_none());
    }

    #[test]
    #[should_panic]
    fn groupby_nonmonotonic() {
        let foos = vec![Foo { x: 1 }, Foo { x: 3 }, Foo { x: 2 }];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut group_iter = AscendingGroupIter::new(foos_iter);
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 1,
                    rows: vec![Foo { x: 1 }, Foo { x: 1 }]
                }))
        );
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 2,
                    rows: vec![Foo { x: 2 }, Foo { x: 2 }]
                }))
        );
        assert!(
            group_iter.next()
                == Some(Ok(GroupedRows {
                    group_key: 3,
                    rows: vec![Foo { x: 3 }]
                }))
        );
        assert!(group_iter.next().is_none());
    }

    #[test]
    fn buffered_sort_empty() {
        let foos = vec![];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut sorted_iter = BufferedSortIter::new(foos_iter, 100).unwrap();
        assert!(sorted_iter.next().is_none());
    }

    #[test]
    fn buffered_sort_results() {
        let foos = vec![
            Foo { x: 1 },
            Foo { x: 3 },
            Foo { x: 1 },
            Foo { x: 4 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 1 },
        ];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut sorted_iter = BufferedSortIter::new(foos_iter, 100).unwrap();
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 4 })));
        assert!(sorted_iter.next().is_none());
    }

    #[test]
    #[should_panic]
    fn buffered_sort_results_too_spread() {
        // the bufsize is only 2 here but the disordered items extend further apart, so this should fail.
        let foos = vec![
            Foo { x: 1 },
            Foo { x: 3 },
            Foo { x: 1 },
            Foo { x: 4 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 1 },
        ];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut sorted_iter = BufferedSortIter::new(foos_iter, 2).unwrap();
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 4 })));
        assert!(sorted_iter.next().is_none());
    }

    #[test]
    fn buffered_sort_results_partial() {
        // here bufsize is 4, which is enough
        let foos = vec![
            Foo { x: 1 },
            Foo { x: 3 },
            Foo { x: 4 },
            Foo { x: 1 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
            Foo { x: 3 },
        ];
        let foos_iter = foos.into_iter().map(|x| {
            let r: Result<Foo, u8> = Ok(x);
            r
        });
        let mut sorted_iter = BufferedSortIter::new(foos_iter, 4).unwrap();
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 1 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 3 })));
        assert!(sorted_iter.next() == Some(Ok(Foo { x: 4 })));
        assert!(sorted_iter.next().is_none());
    }
}
