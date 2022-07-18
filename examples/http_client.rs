use embedded_io::asynch::Read;

use embedded_svc::http::client::asynch::{Client as _, RequestWrite};
use embedded_svc::http::Method;

use embedded_svc_impl::asynch::http::client::Client;
use embedded_svc_impl::asynch::stdnal::StdTcpClientSocket;
use embedded_svc_impl::asynch::tcp::TcpClientSocket;

fn main() {
    simple_logger::SimpleLogger::new().env().init().unwrap();

    smol::block_on(read()).unwrap();
}

async fn read() -> anyhow::Result<()> {
    println!("About to open an HTTP connection to httpbin.org port 80");

    let socket = StdTcpClientSocket::new();
    let mut buf = [0_u8; 8192];

    let mut client = Client::<1024, _>::new(
        &mut buf,
        socket,
        "34.227.213.82:80".parse().unwrap(), /*httpbin.org*/
    );

    for uri in ["/ip", "/headers"] {
        request(&mut client, uri).await?;
    }

    Ok(())
}

async fn request<'a, const N: usize, T>(
    client: &mut Client<'a, N, T>,
    uri: &str,
) -> anyhow::Result<()>
where
    T: TcpClientSocket,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    let mut response = client
        .request(Method::Get, uri, &[("Host", "34.227.213.82")])
        .await?
        .submit()
        .await?;

    let mut result = Vec::new();

    let mut buf = [0_u8; 1024];

    loop {
        let len = response.read(&mut buf).await?;

        if len > 0 {
            result.extend_from_slice(&buf[0..len]);
        } else {
            break;
        }
    }

    println!(
        "Request to httpbin.org, URI \"{}\" returned:\nHeader:\n{}\n\nBody:\n=================\n{}\n=================\n\n\n\n",
        uri,
        response,
        std::str::from_utf8(&result)?);

    Ok(())
}
