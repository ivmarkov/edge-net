use core::fmt::{Debug, Display};

use embedded_io_async::{Read, Write};

use log::warn;

use crate::{io::Error, Method};

use super::{Connection, Handler};

/// A chain of handlers that can be used to route requests to different handlers based on the path and method.
pub struct ChainHandler<H, N> {
    /// The path that this handler should handle.
    pub path: &'static str,
    /// The method that this handler should handle.
    pub method: Method,
    /// The handler that should be called if the path and method match.
    pub handler: H,
    /// The next handler in the chain.
    pub next: N,
}

impl<H, N> ChainHandler<H, N> {
    /// Create a new chain handler for the provided path and for GET requests on that path
    pub fn get<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainHandler<H, N>> {
        self.request(path, Method::Get, handler)
    }

    /// Create a new chain handler for the provided path and for POST requests on that path
    pub fn post<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainHandler<H, N>> {
        self.request(path, Method::Post, handler)
    }

    /// Create a new chain handler for the provided path and for PUT requests on that path
    pub fn put<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainHandler<H, N>> {
        self.request(path, Method::Put, handler)
    }

    /// Create a new chain handler for the provided path and for DELETE requests on that path
    pub fn delete<H2>(
        self,
        path: &'static str,
        handler: H2,
    ) -> ChainHandler<H2, ChainHandler<H, N>> {
        self.request(path, Method::Delete, handler)
    }

    /// Create a new chain handler for the provided path and method
    pub fn request<H2>(
        self,
        path: &'static str,
        method: Method,
        handler: H2,
    ) -> ChainHandler<H2, ChainHandler<H, N>> {
        ChainHandler {
            path,
            method,
            handler,
            next: self,
        }
    }
}

/// The root of a chain of handlers.
///
/// Returns a 404 response for all requests.
pub struct ChainRoot;

impl ChainRoot {
    /// Create a new chain handler for the provided path and for GET requests on that path
    pub fn get<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainRoot> {
        self.request(path, Method::Get, handler)
    }

    /// Create a new chain handler for the provided path and for POST requests on that path
    pub fn post<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainRoot> {
        self.request(path, Method::Post, handler)
    }

    /// Create a new chain handler for the provided path and for PUT requests on that path
    pub fn put<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainRoot> {
        self.request(path, Method::Put, handler)
    }

    /// Create a new chain handler for the provided path and for DELETE requests on that path
    pub fn delete<H2>(self, path: &'static str, handler: H2) -> ChainHandler<H2, ChainRoot> {
        self.request(path, Method::Delete, handler)
    }

    /// Create a new chain handler for the provided path and method
    pub fn request<H2>(
        self,
        path: &'static str,
        method: Method,
        handler: H2,
    ) -> ChainHandler<H2, ChainRoot> {
        ChainHandler {
            path,
            method,
            handler,
            next: ChainRoot,
        }
    }
}

impl Default for ChainRoot {
    fn default() -> Self {
        ChainRoot
    }
}

impl Handler for ChainRoot {
    type Error<E>
        = Error<E>
    where
        E: Debug;

    async fn handle<T, const N: usize>(
        &self,
        task_id: impl Display + Copy,
        connection: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write,
    {
        let headers = connection.headers().ok();

        if let Some(headers) = headers {
            warn!(
                "[Task {task_id}]: No handler found for path: {} and method: {}",
                headers.path, headers.method
            );
        }

        connection.initiate_response(404, None, &[]).await
    }
}

#[derive(Debug)]
pub enum ChainHandlerError<E1, E2> {
    First(E1),
    Second(E2),
}

impl<H, Q> Handler for ChainHandler<H, Q>
where
    H: Handler,
    Q: Handler,
{
    type Error<T>
        = ChainHandlerError<H::Error<T>, Q::Error<T>>
    where
        T: Debug;

    async fn handle<T, const N: usize>(
        &self,
        task_id: impl Display + Copy,
        connection: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write,
    {
        let headers = connection.headers().ok();

        if let Some(headers) = headers {
            if headers.path == self.path && headers.method == self.method {
                return self
                    .handler
                    .handle(task_id, connection)
                    .await
                    .map_err(ChainHandlerError::First);
            }
        }

        self.next
            .handle(task_id, connection)
            .await
            .map_err(ChainHandlerError::Second)
    }
}
