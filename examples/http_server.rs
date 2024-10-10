use edge_http::io::server::{Connection, DefaultServer, Handler};
use edge_http::io::Error;
use edge_http::Method;
use edge_nal::TcpBind;

use embedded_io_async::{Read, Write};

use log::info;

fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let mut server = DefaultServer::new();

    futures_lite::future::block_on(run(&mut server)).unwrap();
}

pub async fn run(server: &mut DefaultServer) -> Result<(), anyhow::Error> {
    let addr = "0.0.0.0:8881";

    info!("Running HTTP server on {addr}");

    let acceptor = edge_nal_std::Stack::new()
        .bind(addr.parse().unwrap())
        .await?;

    server.run(acceptor, HttpHandler, None, None).await?;

    Ok(())
}

struct HttpHandler;

impl<'b, T, const N: usize> Handler<'b, T, N> for HttpHandler
where
    T: Read + Write,
{
    type Error = Error<T::Error>;

    async fn handle(&self, conn: &mut Connection<'b, T, N>) -> Result<(), Self::Error> {
        let headers = conn.headers()?;

        if headers.method != Method::Get {
            conn.initiate_response(405, Some("Method Not Allowed"), &[])
                .await?;
        } else if headers.path != "/" {
            conn.initiate_response(404, Some("Not Found"), &[]).await?;
        } else {
            conn.initiate_response(200, Some("OK"), &[("Content-Type", "text/plain")])
                .await?;

            conn.write_all(b"Hello world!").await?;
        }

        Ok(())
    }
}
