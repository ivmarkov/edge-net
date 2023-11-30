#![cfg_attr(not(feature = "std"), no_std)]

use core::fmt;
use core::time::Duration;

use log::debug;

use domain::{
    base::{
        iana::{Class, Opcode, Rcode},
        octets::*,
        Record, Rtype,
    },
    rdata::A,
};

#[cfg(feature = "std")]
mod server;

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
    debug!("Processing message with header: {:?}", message.header());

    let mut responseb = domain::base::MessageBuilder::from_target(response)?;

    let response = if matches!(message.header().opcode(), Opcode::Query) {
        debug!("Message is of type Query, processing all questions");

        let mut answerb = responseb.start_answer(&message, Rcode::NoError)?;

        for question in message.question() {
            let question = question?;

            if matches!(question.qtype(), Rtype::A) {
                debug!(
                    "Question {:?} is of type A, answering with IP {:?}, TTL {:?}",
                    question, ip, ttl
                );

                let record = Record::new(
                    question.qname(),
                    Class::In,
                    ttl.as_secs() as u32,
                    A::from_octets(ip[0], ip[1], ip[2], ip[3]),
                );
                debug!("Answering question {:?} with {:?}", question, record);
                answerb.push(record)?;
            } else {
                debug!("Question {:?} is not of type A, not answering", question);
            }
        }

        answerb.finish()
    } else {
        debug!("Message is not of type Query, replying with NotImp");

        let headerb = responseb.header_mut();

        headerb.set_id(message.header().id());
        headerb.set_opcode(message.header().opcode());
        headerb.set_rd(message.header().rd());
        headerb.set_rcode(domain::base::iana::Rcode::NotImp);

        responseb.finish()
    };
    Ok(response)
}
