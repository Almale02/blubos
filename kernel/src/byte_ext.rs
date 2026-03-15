pub const trait ByteExt {
    fn kb(&self) -> usize;
    fn mb(&self) -> usize;
    fn gb(&self) -> usize;
    fn tb(&self) -> usize;
}
impl const ByteExt for usize {
    fn kb(&self) -> usize {
        (*self) * 1024_usize.pow(1)
    }

    fn mb(&self) -> usize {
        (*self) * 1024_usize.pow(2)
    }

    fn gb(&self) -> usize {
        (*self) * 1024_usize.pow(3)
    }

    fn tb(&self) -> usize {
        (*self) * 1024_usize.pow(4)
    }
}
