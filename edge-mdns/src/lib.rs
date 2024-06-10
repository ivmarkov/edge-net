#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::large_futures)]

use core::{fmt::{self, Display, Write}, iter::once, ops::RangeBounds};

use ::domain::base::{message_builder::QuestionBuilder, ParsedRecord};
use domain::{
    base::{
        header::Flags, iana::Class, message::ShortMessage, message_builder::{AnswerBuilder, PushError}, name::{FlattenInto, FromStrError}, wire::{Composer, ParseError}, Dname, DnameBuilder, Message, MessageBuilder, ParsedDname, Record, Rtype, ToDname
    },
    dep::octseq::{OctetsBuilder, ShortBuf},
    rdata::{Aaaa, AllRecordData, Cname, Ptr, Srv, Txt, A},
};
use log::trace;
use octseq::{EmptyBuilder, FreezeBuilder, FromBuilder, Octets, Truncate};

// #[cfg(feature = "io")]
// pub mod io;

/// Re-export the domain lib if the user would like to directly 
/// assemble / parse mDNS messages.
pub mod domain {
    pub use domain::*;
}

#[derive(Debug)]
pub enum MdnsError {
    ShortBuf,
    InvalidMessage,
}

impl Display for MdnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortBuf => write!(f, "ShortBuf"),
            Self::InvalidMessage => write!(f, "InvalidMessage"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for MdnsError {}

impl From<ShortBuf> for MdnsError {
    fn from(_: ShortBuf) -> Self {
        Self::ShortBuf
    }
}

impl From<PushError> for MdnsError {
    fn from(_: PushError) -> Self {
        Self::ShortBuf
    }
}

impl From<FromStrError> for MdnsError {
    fn from(_: FromStrError) -> Self {
        Self::InvalidMessage
    }
}

impl From<ShortMessage> for MdnsError {
    fn from(_: ShortMessage) -> Self {
        Self::InvalidMessage
    }
}

impl From<ParseError> for MdnsError {
    fn from(_: ParseError) -> Self {
        Self::InvalidMessage
    }
}

#[derive(Debug, Clone)]
pub struct Host<'a> {
    pub id: u16,
    pub hostname: &'a str,
    pub ip: [u8; 4],
    pub ipv6: Option<[u8; 16]>,
}

impl<'a> Host<'a> {
    pub fn broadcast<'s, T>(
        &self,
        services: T,
        buf: &mut [u8],
        ttl_sec: u32,
    ) -> Result<usize, MdnsError>
    where
        T: IntoIterator<Item = Service<'s>> + Clone,
    {
        let buf = Buf(buf, 0);

        let message = MessageBuilder::from_target(buf)?;

        let mut answer = message.answer();

        self.set_broadcast(services, &mut answer, ttl_sec)?;

        let buf = answer.finish();

        Ok(buf.1)
    }

    pub fn respond<'s, T>(
        &self,
        services: T,
        data: &[u8],
        buf: &mut [u8],
        ttl_sec: u32,
    ) -> Result<usize, MdnsError>
    where
        T: IntoIterator<Item = Service<'s>> + Clone,
    {
        let buf = Buf(buf, 0);

        let message = MessageBuilder::from_target(buf)?;

        let mut answer = message.answer();

        if self.set_response(data, services, &mut answer, ttl_sec)? {
            let buf = answer.finish();

            Ok(buf.1)
        } else {
            Ok(0)
        }
    }

    fn set_broadcast<'s, T, F>(
        &self,
        services: F,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), MdnsError>
    where
        T: Composer,
        F: IntoIterator<Item = Service<'s>> + Clone,
    {
        self.set_header(answer);

        self.add_ipv4(answer, ttl_sec)?;
        self.add_ipv6(answer, ttl_sec)?;

        for service in services.clone() {
            service.add_service(answer, self.hostname, ttl_sec)?;
            service.add_service_type(answer, ttl_sec)?;
            service.add_service_subtypes(answer, ttl_sec)?;
            service.add_txt(answer, ttl_sec)?;
        }

        Ok(())
    }

    fn set_response<'s, T, F>(
        &self,
        data: &[u8],
        services: F,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<bool, MdnsError>
    where
        T: Composer,
        F: IntoIterator<Item = Service<'s>> + Clone,
    {
        self.set_header(answer);

        let message = Message::from_octets(data)?;

        let mut replied = false;

        for question in message.question() {
            trace!("Handling question {:?}", question);

            let question = question?;

            match question.qtype() {
                Rtype::A
                    if question
                        .qname()
                        .name_eq(&Host::host_fqdn(self.hostname, true)?) =>
                {
                    self.add_ipv4(answer, ttl_sec)?;
                    replied = true;
                }
                Rtype::Aaaa
                    if question
                        .qname()
                        .name_eq(&Host::host_fqdn(self.hostname, true)?) =>
                {
                    self.add_ipv6(answer, ttl_sec)?;
                    replied = true;
                }
                Rtype::Srv => {
                    for service in services.clone() {
                        if question.qname().name_eq(&service.service_fqdn(true)?) {
                            self.add_ipv4(answer, ttl_sec)?;
                            self.add_ipv6(answer, ttl_sec)?;
                            service.add_service(answer, self.hostname, ttl_sec)?;
                            replied = true;
                        }
                    }
                }
                Rtype::Ptr => {
                    for service in services.clone() {
                        if question.qname().name_eq(&Service::dns_sd_fqdn(true)?) {
                            service.add_service_type(answer, ttl_sec)?;
                            replied = true;
                        } else if question.qname().name_eq(&service.service_type_fqdn(true)?) {
                            // TODO
                            self.add_ipv4(answer, ttl_sec)?;
                            self.add_ipv6(answer, ttl_sec)?;
                            service.add_service(answer, self.hostname, ttl_sec)?;
                            service.add_service_type(answer, ttl_sec)?;
                            service.add_service_subtypes(answer, ttl_sec)?;
                            service.add_txt(answer, ttl_sec)?;
                            replied = true;
                        }
                    }
                }
                Rtype::Txt => {
                    for service in services.clone() {
                        if question.qname().name_eq(&service.service_fqdn(true)?) {
                            service.add_txt(answer, ttl_sec)?;
                            replied = true;
                        }
                    }
                }
                Rtype::Any => {
                    // A / AAAA
                    if question
                        .qname()
                        .name_eq(&Host::host_fqdn(self.hostname, true)?)
                    {
                        self.add_ipv4(answer, ttl_sec)?;
                        self.add_ipv6(answer, ttl_sec)?;
                        replied = true;
                    }

                    // PTR
                    for service in services.clone() {
                        if question.qname().name_eq(&Service::dns_sd_fqdn(true)?) {
                            service.add_service_type(answer, ttl_sec)?;
                            replied = true;
                        } else if question.qname().name_eq(&service.service_type_fqdn(true)?) {
                            // TODO
                            self.add_ipv4(answer, ttl_sec)?;
                            self.add_ipv6(answer, ttl_sec)?;
                            service.add_service(answer, self.hostname, ttl_sec)?;
                            service.add_service_type(answer, ttl_sec)?;
                            service.add_service_subtypes(answer, ttl_sec)?;
                            service.add_txt(answer, ttl_sec)?;
                            replied = true;
                        }
                    }

                    // SRV
                    for service in services.clone() {
                        if question.qname().name_eq(&service.service_fqdn(true)?) {
                            self.add_ipv4(answer, ttl_sec)?;
                            self.add_ipv6(answer, ttl_sec)?;
                            service.add_service(answer, self.hostname, ttl_sec)?;
                            replied = true;
                        }
                    }
                }
                _ => (),
            }
        }

        Ok(replied)
    }

    fn set_header<T: Composer>(&self, answer: &mut AnswerBuilder<T>) {
        let header = answer.header_mut();
        header.set_id(self.id);
        header.set_opcode(domain::base::iana::Opcode::Query);
        header.set_rcode(domain::base::iana::Rcode::NoError);

        let mut flags = Flags::new();
        flags.qr = true;
        flags.aa = true;
        header.set_flags(flags);
    }

    fn add_ipv4<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        answer.push((
            Self::host_fqdn(self.hostname, false).unwrap(),
            Class::In,
            ttl_sec,
            A::from_octets(self.ip[0], self.ip[1], self.ip[2], self.ip[3]),
        ))
    }

    fn add_ipv6<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        if let Some(ip) = &self.ipv6 {
            answer.push((
                Self::host_fqdn(self.hostname, false).unwrap(),
                Class::In,
                ttl_sec,
                Aaaa::new((*ip).into()),
            ))
        } else {
            Ok(())
        }
    }

    fn host_fqdn(hostname: &str, suffix: bool) -> Result<impl ToDname, FromStrError> {
        let suffix = if suffix { "." } else { "" };

        let mut host_fqdn = heapless07::String::<60>::new();
        write!(host_fqdn, "{}.local{}", hostname, suffix,).unwrap();

        Dname::<heapless07::Vec<u8, 64>>::from_chars(host_fqdn.chars())
    }
}

#[derive(Debug, Clone)]
pub struct Service<'a> {
    pub name: &'a str,
    pub service: &'a str,
    pub protocol: &'a str,
    pub port: u16,
    pub service_subtypes: &'a [&'a str],
    pub txt_kvs: &'a [(&'a str, &'a str)],
}

impl<'a> Service<'a> {
    fn add_service<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        hostname: &str,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        answer.push((
            self.service_fqdn(false).unwrap(),
            Class::In,
            ttl_sec,
            Srv::new(0, 0, self.port, Host::host_fqdn(hostname, false).unwrap()),
        ))
    }

    fn add_service_type<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        answer.push((
            Self::dns_sd_fqdn(false).unwrap(),
            Class::In,
            ttl_sec,
            Ptr::new(self.service_type_fqdn(false).unwrap()),
        ))?;

        answer.push((
            self.service_type_fqdn(false).unwrap(),
            Class::In,
            ttl_sec,
            Ptr::new(self.service_fqdn(false).unwrap()),
        ))
    }

    fn add_service_subtypes<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        for service_subtype in self.service_subtypes {
            self.add_service_subtype(answer, service_subtype, ttl_sec)?;
        }

        Ok(())
    }

    fn add_service_subtype<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        service_subtype: &str,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        answer.push((
            Self::dns_sd_fqdn(false).unwrap(),
            Class::In,
            ttl_sec,
            Ptr::new(self.service_subtype_fqdn(service_subtype, false).unwrap()),
        ))?;

        answer.push((
            self.service_subtype_fqdn(service_subtype, false).unwrap(),
            Class::In,
            ttl_sec,
            Ptr::new(self.service_fqdn(false).unwrap()),
        ))
    }

    fn add_txt<T: Composer>(
        &self,
        answer: &mut AnswerBuilder<T>,
        ttl_sec: u32,
    ) -> Result<(), PushError> {
        // only way I found to create multiple parts in a Txt
        // each slice is the length and then the data
        let mut octets = heapless07::Vec::<_, 256>::new();
        //octets.append_slice(&[1u8, b'X'])?;
        //octets.append_slice(&[2u8, b'A', b'B'])?;
        //octets.append_slice(&[0u8])?;
        for (k, v) in self.txt_kvs {
            octets.append_slice(&[(k.len() + v.len() + 1) as u8])?;
            octets.append_slice(k.as_bytes())?;
            octets.append_slice(&[b'='])?;
            octets.append_slice(v.as_bytes())?;
        }

        let txt = Txt::from_octets(&mut octets).unwrap();

        answer.push((self.service_fqdn(false).unwrap(), Class::In, ttl_sec, txt))
    }

    fn service_fqdn(&self, suffix: bool) -> Result<impl ToDname, FromStrError> {
        let suffix = if suffix { "." } else { "" };

        let mut service_fqdn = heapless07::String::<60>::new();
        write!(
            service_fqdn,
            "{}.{}.{}.local{}",
            self.name, self.service, self.protocol, suffix,
        )
        .unwrap();

        Dname::<heapless07::Vec<u8, 64>>::from_chars(service_fqdn.chars())
    }

    fn service_type_fqdn(&self, suffix: bool) -> Result<impl ToDname, FromStrError> {
        let suffix = if suffix { "." } else { "" };

        let mut service_type_fqdn = heapless07::String::<60>::new();
        write!(
            service_type_fqdn,
            "{}.{}.local{}",
            self.service, self.protocol, suffix,
        )
        .unwrap();

        Dname::<heapless07::Vec<u8, 64>>::from_chars(service_type_fqdn.chars())
    }

    fn service_subtype_fqdn(
        &self,
        service_subtype: &str,
        suffix: bool,
    ) -> Result<impl ToDname, FromStrError> {
        let suffix = if suffix { "." } else { "" };

        let mut service_subtype_fqdn = heapless07::String::<40>::new();
        write!(
            service_subtype_fqdn,
            "{}._sub.{}.{}.local{}",
            service_subtype, self.service, self.protocol, suffix,
        )
        .unwrap();

        Dname::<heapless07::Vec<u8, 64>>::from_chars(service_subtype_fqdn.chars())
    }

    fn dns_sd_fqdn(suffix: bool) -> Result<impl ToDname, FromStrError> {
        Dname::<heapless07::Vec<u8, 64>>::from_chars(
            if suffix {
                "_services._dns-sd._udp.local."
            } else {
                "_services._dns-sd._udp.local"
            }
            .chars(),
        )
    }
}

pub enum Answer<'a> {
    A {
        name: &'a str,
        ip: [u8; 4],
    },
    AAAA {
        name: &'a str,
        ip: [u8; 16],
    },
    Ptr {
        name: &'a str,
        ptr: &'a str,
    },
    Srv {
        name: &'a str,
        port: u16,
        target: &'a str,
    },
    Txt {
        name: &'a str,
        txt: &'a [(&'a str, &'a str)],
    },
}

impl<'a> Answer<'a> {
    pub fn from<O: Octets>(record: ParsedRecord<O>, buf: &'a mut Buf) -> Result<Self, MdnsError> {
        //let answer = answer?;

        let record = record.into_record::<AllRecordData<_, _>>()?.ok_or(MdnsError::InvalidMessage)?;

        write!(buf, "{}", record.owner())?;
        let name = core::str::from_utf8(buf.as_ref()).map_err(|_| MdnsError::InvalidMessage)?;

        let answer = match record.data() {
            AllRecordData::A(a) => Self::A { name, ip: a.addr().octets() },
            AllRecordData::Aaaa(aaaa) => Self::AAAA { name, ip: aaaa.addr().octets() },
            // }
            // AllRecordData::Ptr(ptr) => {
            //     //let hostname: Dname<Buf> = ptr.ptrdname();
            // }

            // Rtype::Srv => {
            // }
            // Rtype::Ptr => {
            //     let ipv6: Record<_, ParsedDname<_>> = answer.into_record()?.ok_or(MdnsError::InvalidMessage)?;

            // }
            // Rtype::Txt => {
            // }
            // AllRecordData::Srv(srv) => {
            //     host.id = srv.port();
            //     //host.hostname = srv.target().to_string();
            // }
            _ => todo!(),
        };

        Ok(answer)
    }
}

pub enum Question<'a> {
    A(&'a str),
    AAAA(&'a str),
    Ptr(&'a str),
    Srv(&'a str),
    Txt(&'a str),
}

// pub enum SimpleQueryDetails<'a> {
//     Host(&'a str),
//     Service(&'a str),
//     ServiceType(&'a str),
// }

// pub enum IteratorWrapper<H, S, T> {
//     Host(H),
//     Service(S),
//     ServiceType(T),
// }

// impl<'a, H, S, T> Iterator for IteratorWrapper<H, S, T>
// where
//     H: Iterator<Item = Question<'a>>,
//     S: Iterator<Item = Question<'a>>,
//     T: Iterator<Item = Question<'a>>,
// {
//     type Item = Question<'a>;

//     fn next(&mut self) -> Option<Question<'a>> {
//         match self {
//             Self::Host(iter) => iter.next(),
//             Self::Service(iter) => iter.next(),
//             Self::ServiceType(iter) => iter.next(),
//         }
//     }
// }

// impl<'a> SimpleQueryDetails<'a> {
//     pub fn questions(&self) -> impl Iterator<Item = Question<'_>> {
//         match self {
//             SimpleQueryDetails::Host(fqdn) => IteratorWrapper::Host(once(Question::A(fqdn)).chain(once(Question::AAAA(fqdn)))),
//             SimpleQueryDetails::Service(fqdn) => IteratorWrapper::Service(once(Question::Ptr(fqdn))),
//             SimpleQueryDetails::ServiceType(fqdn) => IteratorWrapper::ServiceType(once(Question::Ptr(fqdn))),
//         }
//     }
// }

// pub enum QueryDetails<'a> {
//     Simple(SimpleQueryDetails<'a>),
//     Complex(&'a [Question<'a>]),
// }

// pub struct Query<'a> {
//     pub domain: &'a str,
//     pub query_details: QueryDetails<'a>,
// }

impl<'a> Question<'a> {
    // pub fn ask<'s>(questions: &'a [Self], buf: &mut [u8]) -> Result<usize, MdnsError> {
    //     let buf = Buf(buf, 0);

    //     let message = MessageBuilder::from_target(buf)?;

    //     let mut qb = message.question();

    //     for question in questions {
    //         self.push(&mut qb)?;
    //     }

    //     let buf = qb.finish();

    //     Ok(buf.1)
    // }

    // fn push(&self, qb: &mut QuestionBuilder<Buf>) -> Result<(), MdnsError> {
    //     match self {
    //         Self::A(fqdn) => qb.push(((*fqdn).try_into().unwrap(), Rtype::A, Class::In))?,
    //         Self::AAAA(fqdn) => qb.push((fqdn.into(), Rtype::Aaaa, Class::In))?,
    //         Self::Ptr(fqdn) => qb.push((fqdn.into(), Rtype::Ptr, Class::In))?,
    //         Self::Srv(fqdn) => qb.push((fqdn.into(), Rtype::Srv, Class::In))?,
    //         Self::Txt(fqdn) => qb.push((fqdn.into(), Rtype::Txt, Class::In))?,
    //     }

    //     Ok(())
    // }
    
    // pub fn decode_reply<'s>(&self, data: &[u8], buf: &'s mut [u8]) -> Result<impl Iterator<Item = Answer<'s>>, MdnsError> {
    //     // if self.set_response(data, services, &mut answer, ttl_sec)? {
    //     //     let buf = answer.finish();

    //     //     Ok(buf.1)
    //     // } else {
    //     //     Ok(0)
    //     // }

    //     // Ok(buf.1)

    //     let message = Message::from_octets(data)?;

    //     let mut replied = false;

    //     let mut host = Host {
    //         id: 0,
    //         hostname: "",
    //         ip: [0, 0, 0, 0],
    //         ipv6: None,
    //     };

    //     let buf = Buf(buf, 0);

    //     for answer in message.answer()? {
    //         let answer = answer?;

    //         //trace!("Handling answer {:?}", answer);

    //         //let answer = answer?;

    //         let record = answer.into_record::<AllRecordData<_, _>>()?.ok_or(MdnsError::InvalidMessage)?;

    //         match record.into_data() {
    //             AllRecordData::A(a) => {
    //                 host.ip = a.addr().octets();
    //             }
    //             AllRecordData::Aaaa(aaaa) => {
    //                 host.ipv6 = Some(aaaa.addr().octets());
    //             }
    //             AllRecordData::Ptr(ptr) => {
    //                 //let hostname: Dname<Buf> = ptr.ptrdname();
    //             }

    //             // Rtype::Srv => {
    //             // }
    //             // Rtype::Ptr => {
    //             //     let ipv6: Record<_, ParsedDname<_>> = answer.into_record()?.ok_or(MdnsError::InvalidMessage)?;

    //             // }
    //             // Rtype::Txt => {
    //             // }
    //             AllRecordData::Srv(srv) => {
    //                 host.id = srv.port();
    //                 //host.hostname = srv.target().to_string();
    //             }
    //             _ => (),
    //         }
    //     }

    //     //Ok(host)
    //     Ok(core::iter::empty())
    // }
}

pub struct Buf<'a>(pub &'a mut [u8], pub usize);

impl<'a> FreezeBuilder for Buf<'a> {
    type Octets = Self;

    fn freeze(self) -> Self {
        self
    }
}

impl<'a> Octets for Buf<'a> {
    type Range<'r> = &'r [u8] where Self: 'r;

    fn range(&self, range: impl RangeBounds<usize>) -> Self::Range<'_> {
        self.0[..self.1].range(range)
    }
}

impl<'a> FromBuilder for Buf<'a> {
    type Builder = Buf<'a>;

    fn from_builder(builder: Self::Builder) -> Self {
        Buf(&mut builder.0[builder.1..], 0)
    }
}

impl<'a> Composer for Buf<'a> {}

impl<'a> OctetsBuilder for Buf<'a> {
    type AppendError = ShortBuf;

    fn append_slice(&mut self, slice: &[u8]) -> Result<(), Self::AppendError> {
        if self.1 + slice.len() <= self.0.len() {
            let end = self.1 + slice.len();
            self.0[self.1..end].copy_from_slice(slice);
            self.1 = end;

            Ok(())
        } else {
            Err(ShortBuf)
        }
    }
}

impl<'a> Truncate for Buf<'a> {
    fn truncate(&mut self, len: usize) {
        self.1 = len;
    }
}

impl<'a> AsMut<[u8]> for Buf<'a> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0[..self.1]
    }
}

impl<'a> AsRef<[u8]> for Buf<'a> {
    fn as_ref(&self) -> &[u8] {
        &self.0[..self.1]
    }
}
