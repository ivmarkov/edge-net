use std::error::Error;

use embedded_svc::http::client::asynch::BlockingConnection;
use embedded_svc::http::client::{Client, Connection};
use embedded_svc::http::Method;
use embedded_svc::io::Read;
use embedded_svc::mutex::StdRawCondvar;
use embedded_svc::utils::asynch::executor::embedded::{CondvarWait, EmbeddedBlocker};

use embedded_svc_impl::asynch::http::client::ClientConnection;
use embedded_svc_impl::asynch::stdnal::StdTcpClientSocket;

fn main() {
    simple_logger::SimpleLogger::new().env().init().unwrap();

    read().unwrap();
}

fn read() -> anyhow::Result<()> {
    println!("About to open an HTTP connection to httpbin.org port 80");

    let wait = CondvarWait::<StdRawCondvar>::new();

    let blocker = EmbeddedBlocker::new(wait.notify_factory(), wait);

    let socket = StdTcpClientSocket::new();
    let mut buf = [0_u8; 8192];

    let connection = BlockingConnection::new(
        blocker,
        ClientConnection::<1024, _>::new(
            &mut buf,
            socket,
            "34.227.213.82:80".parse().unwrap(), /*httpbin.org*/
        ),
    );

    let mut client = Client::wrap(connection);

    for uri in ["/ip", "/headers"] {
        request(&mut client, uri)?;
    }

    Ok(())
}

fn request<C>(client: &mut Client<C>, uri: &str) -> anyhow::Result<()>
where
    C: Connection,
    C::Error: Error + Send + Sync + 'static,
{
    let mut response = client
        .request(Method::Get, uri, &[("Host", "34.227.213.82")])?
        .submit()?;

    let mut result = Vec::new();

    let mut buf = [0_u8; 1024];

    loop {
        let len = response.read(&mut buf)?;

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
