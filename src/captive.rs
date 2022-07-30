use core::fmt;
use core::time::Duration;

use log::info;

use domain::{
    base::{
        iana::{Class, Opcode, Rcode},
        octets::*,
        Record, Rtype,
    },
    rdata::A,
};

#[cfg(feature = "std")]
pub use server::*;

#[derive(Debug)]
pub struct InnerError<T: fmt::Debug + fmt::Display>(T);

#[derive(Debug)]
pub enum DnsError {
    ShortBuf(InnerError<ShortBuf>),
    ParseError(InnerError<ParseError>),
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsError::ShortBuf(e) => e.0.fmt(f),
            DnsError::ParseError(e) => e.0.fmt(f),
        }
    }
}

impl From<ShortBuf> for DnsError {
    fn from(e: ShortBuf) -> Self {
        Self::ShortBuf(InnerError(e))
    }
}

impl From<ParseError> for DnsError {
    fn from(e: ParseError) -> Self {
        Self::ParseError(InnerError(e))
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DnsError {}

pub fn process_dns_request(
    request: impl AsRef<[u8]>,
    ip: &[u8; 4],
    ttl: Duration,
) -> Result<impl AsRef<[u8]>, DnsError> {
    let request = request.as_ref();
    let response = Octets512::new();

    let message = domain::base::Message::from_octets(request)?;
    info!("Processing message with header: {:?}", message.header());

    let mut responseb = domain::base::MessageBuilder::from_target(response)?;

    let response = if matches!(message.header().opcode(), Opcode::Query) {
        info!("Message is of type Query, processing all questions");

        let mut answerb = responseb.start_answer(&message, Rcode::NoError)?;

        for question in message.question() {
            let question = question?;

            if matches!(question.qtype(), Rtype::A) {
                info!(
                    "Question {:?} is of type A, answering with IP {:?}, TTL {:?}",
                    question, ip, ttl
                );

                let record = Record::new(
                    question.qname(),
                    Class::In,
                    ttl.as_secs() as u32,
                    A::from_octets(ip[0], ip[1], ip[2], ip[3]),
                );
                info!("Answering question {:?} with {:?}", question, record);

                answerb.push(record)?;
            } else {
                info!("Question {:?} is not of type A, not answering", question);
            }
        }

        answerb.finish()
    } else {
        info!("Message is not of type Query, replying with NotImp");

        let headerb = responseb.header_mut();

        headerb.set_id(message.header().id());
        headerb.set_opcode(message.header().opcode());
        headerb.set_rd(message.header().rd());
        headerb.set_rcode(domain::base::iana::Rcode::NotImp);

        responseb.finish()
    };

    Ok(response)
}

#[cfg(feature = "std")]
mod server {
    use std::{
        io, mem,
        net::{Ipv4Addr, SocketAddrV4, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, JoinHandle},
        time::Duration,
    };

    use log::*;

    #[derive(Clone, Debug)]
    pub struct DnsConf {
        pub bind_ip: Ipv4Addr,
        pub bind_port: u16,
        pub ip: Ipv4Addr,
        pub ttl: Duration,
    }

    impl DnsConf {
        pub fn new(ip: Ipv4Addr) -> Self {
            Self {
                bind_ip: Ipv4Addr::new(0, 0, 0, 0),
                bind_port: 53,
                ip,
                ttl: Duration::from_secs(60),
            }
        }
    }

    #[derive(Debug)]
    pub enum Status {
        Stopped,
        Started,
        Error(io::Error),
    }

    pub struct DnsServer {
        conf: DnsConf,
        status: Status,
        running: Arc<AtomicBool>,
        handle: Option<JoinHandle<Result<(), io::Error>>>,
    }

    impl DnsServer {
        pub fn new(conf: DnsConf) -> Self {
            Self {
                conf,
                status: Status::Stopped,
                running: Arc::new(AtomicBool::new(false)),
                handle: None,
            }
        }

        pub fn get_status(&mut self) -> &Status {
            self.cleanup();
            &self.status
        }

        pub fn start(&mut self) -> Result<(), io::Error> {
            if matches!(self.get_status(), Status::Started) {
                return Ok(());
            }

            let socket =
                UdpSocket::bind(SocketAddrV4::new(self.conf.bind_ip, self.conf.bind_port))?;

            socket.set_read_timeout(Some(Duration::from_secs(1)))?;

            let running = self.running.clone();
            let ip = self.conf.ip;
            let ttl = self.conf.ttl;

            self.running.store(true, Ordering::Relaxed);

            self.handle = Some(thread::spawn(move || {
                let result = Self::run(&*running, ip, ttl, socket);

                running.store(false, Ordering::Relaxed);

                result
            }));

            Ok(())
        }

        pub fn stop(&mut self) -> Result<(), io::Error> {
            if matches!(self.get_status(), Status::Stopped) {
                return Ok(());
            }

            self.running.store(false, Ordering::Relaxed);
            self.cleanup();

            let mut status = Status::Stopped;
            mem::swap(&mut self.status, &mut status);

            match status {
                Status::Error(e) => Err(e),
                _ => Ok(()),
            }
        }

        fn cleanup(&mut self) {
            if !self.running.load(Ordering::Relaxed) && self.handle.is_some() {
                self.status = match mem::take(&mut self.handle).unwrap().join().unwrap() {
                    Ok(_) => Status::Stopped,
                    Err(e) => Status::Error(e),
                };
            }
        }

        fn run(
            running: &AtomicBool,
            ip: Ipv4Addr,
            ttl: Duration,
            socket: UdpSocket,
        ) -> Result<(), io::Error> {
            while running.load(Ordering::Relaxed) {
                info!("Waiting for data");

                let mut request_arr = [0_u8; 512];

                let (request_len, source_addr) = match socket.recv_from(&mut request_arr) {
                    Ok(value) => value,
                    Err(err) => match err.kind() {
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => continue,
                        _ => return Err(err),
                    },
                };

                let request = &request_arr[..request_len];

                info!("Received {} bytes from {}", request.len(), source_addr);

                let response = super::process_dns_request(request, &ip.octets(), ttl)
                    .map_err(|_| io::ErrorKind::Other)?;

                socket.send_to(response.as_ref(), source_addr)?;

                info!("Sent {} bytes to {}", response.as_ref().len(), source_addr);
            }

            Ok(())
        }
    }
}
