pub trait WithKey<T> {
    fn key(&self) -> T;
}
