#![cfg_attr(not(feature = "std"), no_std)]

use core::fmt::{self, Display};
use core::time::Duration;

use log::debug;

use domain::{
    base::{
        iana::{Class, Opcode, Rcode},
        message::ShortMessage,
        message_builder::PushError,
        record::Ttl,
        wire::ParseError,
        Record, Rtype,
    },
    dep::octseq::ShortBuf,
    rdata::A,
};

pub mod io;

#[derive(Debug)]
pub enum DnsError {
    ShortBuf,
    InvalidMessage,
}

impl Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortBuf => write!(f, "ShortBuf"),
            Self::InvalidMessage => write!(f, "InvalidMessage"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DnsError {}

impl From<ShortBuf> for DnsError {
    fn from(_: ShortBuf) -> Self {
        Self::ShortBuf
    }
}

impl From<PushError> for DnsError {
    fn from(_: PushError) -> Self {
        Self::ShortBuf
    }
}

impl From<ShortMessage> for DnsError {
    fn from(_: ShortMessage) -> Self {
        Self::InvalidMessage
    }
}

impl From<ParseError> for DnsError {
    fn from(_: ParseError) -> Self {
        Self::InvalidMessage
    }
}

pub fn process_dns_request(
    request: &[u8],
    ip: &[u8; 4],
    ttl: Duration,
) -> Result<impl AsRef<[u8]>, DnsError> {
    let response = heapless07::Vec::<u8, 512>::new();

    let message = domain::base::Message::from_octets(request)?;
    debug!("Processing message with header: {:?}", message.header());

    let mut responseb = domain::base::MessageBuilder::from_target(response)?;

    let response = if matches!(message.header().opcode(), Opcode::Query) {
        debug!("Message is of type Query, processing all questions");

        let mut answerb = responseb.start_answer(&message, Rcode::NoError)?;

        for question in message.question() {
            let question = question?;

            if matches!(question.qtype(), Rtype::A) && matches!(question.qclass(), Class::In) {
                log::info!(
                    "Question {:?} is of type A, answering with IP {:?}, TTL {:?}",
                    question,
                    ip,
                    ttl
                );

                let record = Record::new(
                    question.qname(),
                    Class::In,
                    Ttl::from_duration_lossy(ttl),
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
