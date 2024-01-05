use core::future::pending;

use async_io::block_on;
use log::LevelFilter;

use edge_net::asynch::{
    http::{
        server::{Handler, HandlerError, Server, ServerConnection},
        Method,
    },
    stdnal::StdTcpListen,
    tcp::{TcpAccept, TcpListen},
};
use edge_net::std_mutex::StdRawMutex;
use embassy_sync::blocking_mutex::raw::RawMutex;
use embedded_io_async::{Read, Write};

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    block_on(accept());
}

pub async fn accept() {
    let binder = StdTcpListen::new();

    run::<StdRawMutex, _>(
        binder
            .listen("0.0.0.0:8181".parse().unwrap())
            .await
            .unwrap(),
    )
    .await;
}

pub async fn run<R, A>(acceptor: A)
where
    R: RawMutex,
    A: TcpAccept + 'static,
{
    let mut server = Server::<128, 2048, _, _>::new(acceptor, SimpleHandler);

    server.process::<16, 16, R, _>(pending()).await.unwrap();
}

pub struct SimpleHandler;

impl SimpleHandler {
    async fn handle<'a, 'b, const N: usize, T>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Result<(), HandlerError>
    where
        'b: 'a,
        T: Read + Write + 'a,
    {
        if path == "/" {
            if method == Method::Get {
                connection
                    .initiate_response(
                        200,
                        Some("OK"),
                        &[("Content-Length", "10"), ("Content-Type", "text/plain")],
                    )
                    .await?;

                connection.write_all("Hello!\r\n\r\n".as_bytes()).await?;
            } else {
                connection.initiate_response(405, None, &[]).await?;
            }
        } else {
            connection.initiate_response(404, None, &[]).await?;
        }

        Ok(())
    }
}

impl<'b, const N: usize, T> Handler<'b, N, T> for SimpleHandler
where
    T: Read + Write,
{
    async fn handle<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Result<(), HandlerError> {
        SimpleHandler::handle(self, path, method, connection).await
    }
}
