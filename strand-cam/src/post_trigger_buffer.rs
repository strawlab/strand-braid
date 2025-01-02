use std::collections::VecDeque;

use ci2::DynamicFrameWithInfo;

pub(crate) struct PostTriggerBuffer {
    size: usize,
    inner: VecDeque<DynamicFrameWithInfo>,
}

impl PostTriggerBuffer {
    pub(crate) fn new() -> Self {
        Self {
            size: 0,
            inner: VecDeque::new(),
        }
    }

    fn trim(&mut self) {
        while self.inner.len() > self.size {
            self.inner.pop_front();
        }
    }

    pub(crate) fn set_size(&mut self, size: usize) {
        self.size = size;
        self.trim();
    }

    pub(crate) fn push(&mut self, frame: &DynamicFrameWithInfo) {
        if self.size > 0 {
            self.inner.push_back(frame.clone());
        }
        self.trim();
    }

    pub(crate) fn get_and_clear(&mut self) -> VecDeque<DynamicFrameWithInfo> {
        std::mem::take(&mut self.inner)
    }
}
