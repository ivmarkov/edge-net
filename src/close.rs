use embedded_io::{
    blocking::{Read, Write},
    Io,
};

pub trait Close {
    fn close(&mut self);
}

impl<C> Close for &mut C
where
    C: Close,
{
    fn close(&mut self) {
        (*self).close()
    }
}

pub struct CloseFn<T, F>(T, F);

impl<T, F> CloseFn<T, F> {
    pub const fn new(io: T, f: F) -> Self {
        Self(io, f)
    }
}

impl<T> CloseFn<T, ()> {
    pub const fn noop(io: T) -> Self {
        Self(io, ())
    }
}

impl<T, F> AsRef<T> for CloseFn<T, F> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T, F> AsMut<T> for CloseFn<T, F> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Close for CloseFn<T, ()> {
    fn close(&mut self) {}
}

impl<T, F> Close for CloseFn<T, F>
where
    F: Fn(&mut T),
{
    fn close(&mut self) {
        (self.1)(&mut self.0)
    }
}

impl<T, F> Io for CloseFn<T, F>
where
    T: Io,
{
    type Error = T::Error;
}

impl<T, F> Read for CloseFn<T, F>
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf)
    }
}

impl<T, F> Write for CloseFn<T, F>
where
    T: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush()
    }
}

#[cfg(feature = "experimental")]
pub mod asynch {
    use embedded_io::asynch::{Read, Write};

    use super::CloseFn;

    impl<T, F> Read for CloseFn<T, F>
    where
        T: Read,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = T::ReadFuture<'a>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            self.0.read(buf)
        }
    }

    impl<T, F> Write for CloseFn<T, F>
    where
        T: Write,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = T::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.0.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = T::FlushFuture<'a>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            self.0.flush()
        }
    }
}
