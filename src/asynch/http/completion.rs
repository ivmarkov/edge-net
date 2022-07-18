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
        asynch::http::{Body, BodyType, Error, SendBody},
        close::Close,
    };

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

        pub fn complete_read(&mut self, complete: bool) {
            self.read = if complete {
                CompletionState::Complete
            } else {
                CompletionState::Started
            };
        }

        pub fn complete_write(&mut self, complete: bool) {
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

    pub struct BodyCompletionTracker<'b, T>(Body<'b, CompletionTracker<T>>)
    where
        T: Close;

    impl<'b, T> BodyCompletionTracker<'b, T>
    where
        T: Read + Close,
    {
        pub fn new(
            body_type: BodyType,
            buf: &'b mut [u8],
            read_len: usize,
            input: CompletionTracker<T>,
        ) -> Self {
            Self::wrap(Body::new(body_type, buf, read_len, input))
        }

        pub fn wrap(body: Body<'b, CompletionTracker<T>>) -> Self {
            let mut this = Self(body);

            this.update_completion();
            this
        }

        pub fn release(self) -> Body<'b, CompletionTracker<T>> {
            self.0
        }

        pub fn body(&mut self) -> &mut Body<'b, CompletionTracker<T>> {
            &mut self.0
        }

        fn update_completion(&mut self) {
            let complete = self.body().is_complete();
            self.body().as_raw_reader().complete_read(complete);
        }
    }

    impl<'b, T> Io for BodyCompletionTracker<'b, T>
    where
        T: Io + Close,
    {
        type Error = Error<T::Error>;
    }

    impl<'b, T> Read for BodyCompletionTracker<'b, T>
    where
        T: Read + Close,
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

                self.update_completion();
                Ok(size)
            }
        }
    }

    pub struct SendBodyCompletionTracker<T>(SendBody<CompletionTracker<T>>)
    where
        T: Close;

    impl<T> SendBodyCompletionTracker<T>
    where
        T: Write + Close,
    {
        pub fn new(body_type: BodyType, output: CompletionTracker<T>) -> Self {
            Self::wrap(SendBody::new(body_type, output))
        }

        pub fn wrap(body: SendBody<CompletionTracker<T>>) -> Self {
            let mut this = Self(body);

            this.update_completion();
            this
        }

        pub fn release(self) -> SendBody<CompletionTracker<T>> {
            self.0
        }

        pub fn body(&mut self) -> &mut SendBody<CompletionTracker<T>> {
            &mut self.0
        }

        fn update_completion(&mut self) {
            let complete = self.body().is_complete();
            self.body().as_raw_writer().complete_write(complete);
        }
    }

    impl<T> Io for SendBodyCompletionTracker<T>
    where
        T: Io + Close,
    {
        type Error = Error<T::Error>;
    }

    impl<T> Write for SendBodyCompletionTracker<T>
    where
        T: Write + Close,
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

                self.update_completion();
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
                })?;

                self.update_completion();

                Ok(())
            }
        }
    }
}
