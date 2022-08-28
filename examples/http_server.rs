#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

use core::future::{pending, Future};

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
use embedded_io::asynch::{Read, Write};

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .env()
        .init()
        .unwrap();

    smol::block_on(accept());
}

pub async fn accept() {
    let binder = StdTcpListen::new();

    run::<StdRawMutex, _>(
        binder
            .listen("0.0.0.0:8080".parse().unwrap())
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

impl<'b, const N: usize, T> Handler<'b, N, T> for SimpleHandler
where
    T: Read + Write,
{
    type HandleFuture<'a> = impl Future<Output = Result<(), HandlerError>>
    where Self: 'a, 'b: 'a, T: 'a;

    fn handle<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        connection: &'a mut ServerConnection<'b, N, T>,
    ) -> Self::HandleFuture<'a> {
        async move {
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
}
