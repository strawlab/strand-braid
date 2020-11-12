/// A data structure for handling a queue of frames to be processed.
///
/// Circular buffer with a leading index ("available") and
/// a potentially lagging index ("pending"). Get an available item and then,
///
/// modeled after CNvQueue in nvidia Samples in Video Codec SDK
#[allow(non_snake_case)]
pub struct Queue<T> {
    m_pBuffer: Vec<T>,
    m_uPendingCount: usize,
    m_uAvailableIdx: usize,
    m_uPendingndex: usize,
}

#[allow(non_snake_case)]
impl<T> Queue<T> {
    #[allow(non_snake_case)]
    pub fn new(pBuffer: Vec<T>) -> Self {
        Self {
            m_pBuffer: pBuffer,
            m_uPendingCount: 0,
            m_uAvailableIdx: 0,
            m_uPendingndex: 0,
        }
    }

    pub fn get_available(&mut self) -> Option<&mut T> {
        let sz = self.m_pBuffer.len();
        if self.m_uPendingCount == sz {
            None
        } else {
            let pItem = &mut self.m_pBuffer[self.m_uAvailableIdx];
            self.m_uAvailableIdx = (self.m_uAvailableIdx + 1) % sz;
            self.m_uPendingCount += 1;
            Some(pItem)
        }
    }

    pub fn get_pending(&mut self) -> Option<&mut T> {
        let sz = self.m_pBuffer.len();
        if self.m_uPendingCount == 0 {
            None
        } else {
            let pItem = &mut self.m_pBuffer[self.m_uPendingndex];
            self.m_uPendingndex = (self.m_uPendingndex + 1) % sz;
            self.m_uPendingCount -= 1;
            Some(pItem)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::queue::Queue;

    #[test]
    fn test_queue() {
        let items = vec![
            "Buffer 1".to_string(),
            "Buffer 2".to_string(),
            "Buffer 3".to_string(),
        ];
        {
            let mut q = Queue::new(items);

            assert!(q.get_pending().is_none());

            let i1 = q.get_available().unwrap();
            println!("i1 {:?}", i1);

            *i1 = "Buffer 1 modified".to_string();

            let i2 = q.get_available().unwrap();
            println!("i2 {:?}", i2);

            let i3 = q.get_available().unwrap();
            println!("i3 {:?}", i3);

            assert!(q.get_available().is_none());

            let p1 = q.get_pending().unwrap();
            println!("p1 {:?}", p1);

            assert_eq!(p1.as_str(), "Buffer 1 modified");
        }
    }
}
