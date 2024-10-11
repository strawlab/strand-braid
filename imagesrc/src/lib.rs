extern crate log;
extern crate machine_vision_formats as formats;

use futures::stream::Stream;

pub trait Sample {}

pub trait ImageSource<S: Sample> {
    fn frames(
        &mut self,
    ) -> Result<Box<dyn Stream<Item = S> + Send + Unpin>, Box<dyn std::error::Error>>;
}
