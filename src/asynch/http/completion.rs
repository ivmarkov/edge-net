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
        asynch::http::{Body, Error, PartiallyRead, SendBody},
        close::Close,
    };

    pub struct Completion<T>
    where
        T: Close,
    {
        io: T,
        read_started: bool,
        read_complete: bool,
        write_started: bool,
        write_complete: bool,
    }

    impl<T> Completion<T>
    where
        T: Close,
    {
        const fn new(io: T) -> Self {
            Self {
                io,
                read_started: false,
                read_complete: false,
                write_started: false,
                write_complete: false,
            }
        }

        fn read_complete(&mut self, complete: bool) {
            self.read_complete = complete;
        }

        fn write_complete(&mut self, complete: bool) {
            self.write_complete = complete;
        }
    }

    impl<T> Drop for Completion<T>
    where
        T: Close,
    {
        fn drop(&mut self) {
            if self.read_started && !self.read_complete
                || self.write_started && !self.write_complete
            {
                self.close();
            }
        }
    }

    impl<T> Close for Completion<T>
    where
        T: Close,
    {
        fn close(&mut self) {
            info!("Socket closed");

            self.io.close();
        }
    }

    impl<T> Io for Completion<T>
    where
        T: Io + Close,
    {
        type Error = T::Error;
    }

    impl<T> Read for Completion<T>
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

    impl<T> Write for Completion<T>
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

    pub struct BodyCompletionTracker<'b, T>(Body<'b, PartiallyRead<'b, Completion<T>>>)
    where
        T: Close;

    impl<'b, T> BodyCompletionTracker<'b, T>
    where
        T: Read + Close,
    {
        pub fn new<const N: usize>(
            headers: &crate::asynch::http::Headers<'b, N>,
            buf: &'b mut [u8],
            read_len: usize,
            completion: Completion<T>,
        ) -> Self {
            Self(Body::new(headers, buf, read_len, completion))
        }

        pub fn release(self) -> Completion<T> {
            self.0.release().release()
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

                let complete = self.0.is_complete();
                self.0
                    .as_raw_reader()
                    .as_raw_reader()
                    .read_complete(complete);

                Ok(size)
            }
        }
    }

    pub struct SendBodyCompletionTracker<T>(SendBody<Completion<T>>)
    where
        T: Close;

    impl<T> SendBodyCompletionTracker<T>
    where
        T: Write + Close,
    {
        pub fn new<'b>(
            headers: &crate::asynch::http::SendHeaders<'b>,
            completion: Completion<T>,
        ) -> Self {
            Self(SendBody::new(headers, completion))
        }

        pub fn release(self) -> Completion<T> {
            self.0.release()
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

                let complete = self.0.is_complete();
                self.0.as_raw_writer().write_complete(complete);

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
