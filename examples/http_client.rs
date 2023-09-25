use std::error::Error;

use embedded_io_async::Read;
use embedded_nal_async::TcpConnect;

use edge_net::asynch::http::client::ClientConnection;
use edge_net::asynch::http::Method;
use edge_net::asynch::stdnal::StdTcpConnect;

fn main() {
    simple_logger::SimpleLogger::new().env().init().unwrap();

    smol::block_on(read()).unwrap();
}

async fn read() -> anyhow::Result<()> {
    println!("About to open an HTTP connection to httpbin.org port 80");

    let connector = StdTcpConnect::new();
    let mut buf = [0_u8; 8192];

    let mut connection = ClientConnection::<1024, _>::new(
        &mut buf,
        &connector,
        "34.227.213.82:80".parse().unwrap(), /*httpbin.org*/
    );

    for uri in ["/ip", "/headers"] {
        request(&mut connection, uri).await?;
    }

    Ok(())
}

async fn request<'b, const N: usize, T>(
    connection: &mut ClientConnection<'b, N, T>,
    uri: &str,
) -> anyhow::Result<()>
where
    T: TcpConnect,
    T::Error: Error + Send + Sync + 'static,
{
    connection
        .initiate_request(Method::Get, uri, &[("Host", "34.227.213.82")])
        .await?;
    connection.initiate_response().await?;

    let mut result = Vec::new();

    let mut buf = [0_u8; 1024];

    loop {
        let len = connection.read(&mut buf).await?;

        if len > 0 {
            result.extend_from_slice(&buf[0..len]);
        } else {
            break;
        }
    }

    println!(
        "Request to httpbin.org, URI \"{}\" returned:\nBody:\n=================\n{}\n=================\n\n\n\n",
        uri,
        //response.headers(),
        std::str::from_utf8(&result)?);

    Ok(())
}
