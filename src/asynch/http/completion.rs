#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::future::Future;

    use log::info;

    use embedded_io::{
        asynch::{Read, Write},
        Io,
    };

    use crate::{
        asynch::http::{Body, BodyType, Error, PartiallyRead, SendBody},
        close::Close,
    };

    pub trait Complete {
        fn complete_read(&mut self, complete: bool);
        fn complete_write(&mut self, complete: bool);
    }

    impl<C> Complete for &mut C
    where
        C: Complete,
    {
        fn complete_read(&mut self, complete: bool) {
            (*self).complete_read(complete);
        }

        fn complete_write(&mut self, complete: bool) {
            (*self).complete_write(complete);
        }
    }

    #[derive(Copy, Clone, Eq, PartialEq, Debug)]
    pub enum CompletionState {
        NotStarted,
        Started,
        Complete,
    }

    pub struct CompletionTracker<T>
    where
        T: Close,
    {
        io: T,
        read: CompletionState,
        write: CompletionState,
    }

    impl<T> CompletionTracker<T>
    where
        T: Close,
    {
        pub const fn new(io: T) -> Self {
            Self {
                io,
                read: CompletionState::NotStarted,
                write: CompletionState::NotStarted,
            }
        }

        pub fn reset(&mut self) {
            self.read = CompletionState::NotStarted;
            self.write = CompletionState::NotStarted;
        }

        pub fn as_raw(&mut self) -> &mut T {
            &mut self.io
        }

        pub fn completion(&self) -> (CompletionState, CompletionState) {
            (self.read, self.write)
        }
    }

    impl<T> Complete for CompletionTracker<T>
    where
        T: Close,
    {
        fn complete_read(&mut self, complete: bool) {
            self.read = if complete {
                CompletionState::Complete
            } else {
                CompletionState::Started
            };
        }

        fn complete_write(&mut self, complete: bool) {
            self.write = if complete {
                CompletionState::Complete
            } else {
                CompletionState::Started
            };
        }
    }

    impl<T> Drop for CompletionTracker<T>
    where
        T: Close,
    {
        fn drop(&mut self) {
            if self.read != self.write
                || self.read != CompletionState::NotStarted
                    && self.read != CompletionState::Complete
            {
                self.close();
            }
        }
    }

    impl<T> Close for CompletionTracker<T>
    where
        T: Close,
    {
        fn close(&mut self) {
            info!("Socket closed");

            self.io.close();
        }
    }

    impl<T> Io for CompletionTracker<T>
    where
        T: Io + Close,
    {
        type Error = T::Error;
    }

    impl<T> Read for CompletionTracker<T>
    where
        T: Read + Close,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = T::ReadFuture<'a>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            self.io.read(buf)
        }
    }

    impl<T> Write for CompletionTracker<T>
    where
        T: Write + Close,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = T::WriteFuture<'a>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            self.io.write(buf)
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = T::FlushFuture<'a>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            self.io.flush()
        }
    }

    pub struct BodyCompletionTracker<'b, T>(Body<'b, PartiallyRead<'b, T>>);

    impl<'b, T> BodyCompletionTracker<'b, T>
    where
        T: Read,
    {
        pub fn new<const N: usize>(
            body_type: BodyType,
            buf: &'b mut [u8],
            read_len: usize,
            input: T,
        ) -> Self {
            Self(Body::new(body_type, buf, read_len, input))
        }

        pub const fn wrap(body: Body<'b, PartiallyRead<'b, T>>) -> Self {
            Self(body)
        }

        pub fn release(self) -> Body<'b, PartiallyRead<'b, T>> {
            self.0
        }
    }

    impl<'b, T> Io for BodyCompletionTracker<'b, T>
    where
        T: Io,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, T> Read for BodyCompletionTracker<'b, T>
    where
        T: Read + Close + Complete,
    {
        type ReadFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Self::ReadFuture<'a> {
            async move {
                let size = self.0.read(buf).await.map_err(|e| {
                    self.0.close();
                    e
                })?;

                let complete = self.0.is_complete();
                self.0
                    .as_raw_reader()
                    .as_raw_reader()
                    .complete_read(complete);

                Ok(size)
            }
        }
    }

    pub struct SendBodyCompletionTracker<T>(SendBody<T>);

    impl<T> SendBodyCompletionTracker<T>
    where
        T: Write,
    {
        pub fn new(body_type: BodyType, output: T) -> Self {
            Self(SendBody::new(body_type, output))
        }

        pub const fn wrap(body: SendBody<T>) -> Self {
            let mut this = Self(body);

            let complete = this.0.is_complete();
            this.as_raw_reader().as_raw_reader().set_complete(complete);

            this
        }

        pub fn release(self) -> SendBody<T> {
            self.0
        }
    }

    impl<T> Io for SendBodyCompletionTracker<T>
    where
        T: Io,
    {
        type Error = Error<T::Error>;
    }

    impl<T> Write for SendBodyCompletionTracker<T>
    where
        T: Write + Close + Complete,
    {
        type WriteFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<usize, Self::Error>>;

        fn write<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteFuture<'a> {
            async move {
                let size = self.0.write(buf).await.map_err(|e| {
                    self.0.close();
                    e
                })?;

                let complete = self.0.is_complete();
                self.0.as_raw_writer().complete_write(complete);

                Ok(size)
            }
        }

        type FlushFuture<'a>
        where
            Self: 'a,
        = impl Future<Output = Result<(), Self::Error>>;

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            async move {
                self.0.flush().await.map_err(|e| {
                    self.0.close();
                    e
                })
            }
        }
    }
}
