use edge_http::io::server::{Connection, Handler, Server, ServerBuffers};
use edge_http::Method;

use edge_std_nal_async::StdTcpListen;
use embedded_nal_async_xtra::TcpListen;

use embedded_io_async::{Read, Write};

use log::info;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let mut buffers: ServerBuffers = ServerBuffers::new();

    futures_lite::future::block_on(run(&mut buffers)).unwrap();
}

pub async fn run<const P: usize, const B: usize>(
    buffers: &mut ServerBuffers<P, B>,
) -> Result<(), anyhow::Error> {
    let addr = "0.0.0.0:8881";

    info!("Running HTTP server on {addr}");

    let acceptor = StdTcpListen::new().listen(addr.parse().unwrap()).await?;

    let mut server: Server<_, _> = Server::new(acceptor, HttpHandler);

    server.process::<2, P, B>(buffers).await?;

    Ok(())
}

struct HttpHandler;

impl<'b, T, const N: usize> Handler<'b, T, N> for HttpHandler
where
    T: Read + Write,
    T::Error: Send + Sync + std::error::Error + 'static,
{
    type Error = anyhow::Error;

    async fn handle(&self, conn: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        let headers = conn.headers()?;

        if !matches!(headers.method, Some(Method::Get)) {
            conn.initiate_response(405, Some("Method Not Allowed"), &[])
                .await?;
        } else if !matches!(headers.path, Some("/")) {
            conn.initiate_response(404, Some("Not Found"), &[]).await?;
        } else {
            conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
                .await?;

            conn.write_all(b"Hello world!").await?;
        }

        Ok(())
    }
}
