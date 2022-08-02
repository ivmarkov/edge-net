#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]

use core::future::{pending, Future};

use edge_net::asynch::{
    http::{
        server::{Handler, HandlerError, Server, ServerConnection},
        Method,
    },
    stdnal::StdTcpBinder,
    tcp::{TcpAcceptor, TcpBinder},
};
use edge_net::std_mutex::StdRawMutex;
use embassy_util::blocking_mutex::raw::RawMutex;
use embedded_io::asynch::{Read, Write};

fn main() {
    simple_logger::SimpleLogger::new().env().init().unwrap();

    smol::block_on(accept());
}

pub async fn accept() {
    let binder = StdTcpBinder::new();

    run::<StdRawMutex, _>(binder.bind("0.0.0.0:8080".parse().unwrap()).await.unwrap()).await;
}

pub async fn run<R, A>(acceptor: A)
where
    R: RawMutex,
    A: TcpAcceptor,
{
    let mut server = Server::<128, 2048, _, _>::new(acceptor, SimpleHandler);

    server.process::<4, 4, R, _>(pending()).await.unwrap();
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
                    connection.initiate_response(200, None, &[]).await?;

                    connection.write_all("Hello!".as_bytes()).await?;
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
