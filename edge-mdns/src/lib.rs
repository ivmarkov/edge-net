#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::large_futures)]

use core::fmt::{self, Display};
use core::net::{Ipv4Addr, Ipv6Addr};
use core::ops::RangeBounds;

//use bitflags::bitflags;

use ::domain::base::message::Section;
use ::domain::base::{Message, MessageBuilder, ParsedRecord, RecordSection};
use ::domain::dep::octseq::{FreezeBuilder, FromBuilder, OctetsBuilder, Truncate};
use domain::base::iana::Class;
use domain::base::message::ShortMessage;
use domain::base::message_builder::PushError;
use domain::base::name::{FromStrError, Label, ToLabelIter};
use domain::base::record::ComposeRecord;
use domain::base::wire::{Composer, ParseError};
use domain::base::ToName;
use domain::base::{ParsedName, Record, Ttl};
use domain::dep::octseq::Octets;
use domain::dep::octseq::ShortBuf;
use domain::rdata::{Aaaa, AllRecordData, Ptr, Srv, A};

// #[cfg(feature = "io")]
// pub mod io;

/// Re-export the domain lib if the user would like to directly
/// assemble / parse mDNS messages.
pub mod domain {
    pub use domain::*;
}

pub mod host;

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

#[derive(Clone)]
pub struct NameLabels<'a>(&'a [&'a str]);

impl<'a> NameLabels<'a> {
    pub const fn new(labels: &'a [&'a str]) -> Self {
        Self(labels)
    }
}

impl<'a> ToName for NameLabels<'a> {}

#[derive(Clone)]
pub struct NameLabelsIter<'a> {
    name: &'a NameLabels<'a>,
    index: usize,
}

impl<'a> Iterator for NameLabelsIter<'a> {
    type Item = &'a Label;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.name.0.len() {
            let label = Label::from_slice(self.name.0[self.index].as_bytes()).unwrap();
            self.index += 1;
            Some(label)
        } else {
            None
        }
    }
}

impl<'a> DoubleEndedIterator for NameLabelsIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index > 0 {
            self.index -= 1;
            let label = Label::from_slice(self.name.0[self.index].as_bytes()).unwrap();
            Some(label)
        } else {
            None
        }
    }
}

impl<'a> ToLabelIter for NameLabels<'a> {
    type LabelIter<'t> = NameLabelsIter<'t> where Self: 't;

    fn iter_labels(&self) -> Self::LabelIter<'_> {
        NameLabelsIter {
            name: self,
            index: 0,
        }
    }
}

pub enum Answer<N, T> {
    A {
        owner: N,
        ip: Ipv4Addr,
    },
    AAAA {
        owner: N,
        ip: Ipv6Addr,
    },
    Ptr {
        owner: N,
        ptr: T,
    },
    Srv {
        owner: N,
        port: u16,
        target: T,
    },
    Txt {
        owner: N,
        //txt: &'a [(&'a str, &'a str)],
    },
}
impl<N, T> Answer<N, T>
where
    N: ToName,
    T: ToName,
{
    pub fn owner(&self) -> &N {
        match self {
            Self::A { owner, .. } => owner,
            Self::AAAA { owner, .. } => owner,
            Self::Ptr { owner, .. } => owner,
            Self::Srv { owner, .. } => owner,
            Self::Txt { owner, .. } => owner,
        }
    }
}

impl<'a, O, N> TryFrom<Record<ParsedName<O>, AllRecordData<O, N>>> for Answer<ParsedName<O>, N>
where
    O: Octets,
    N: ToName,
{
    type Error = MdnsError;

    fn try_from(record: Record<ParsedName<O>, AllRecordData<O, N>>) -> Result<Self, Self::Error> {
        Answer::parse(record)
    }
}

impl<'a, O, N> ComposeRecord for Answer<O, N>
where
    O: ToName,
    N: ToName,
{
    fn compose_record<Target: Composer + ?Sized>(
        &self,
        tgt: &mut Target,
    ) -> Result<(), Target::AppendError> {
        let ttl = Ttl::from_secs(60);

        match self {
            Self::A { owner, ip } => Record::new(
                owner,
                Class::IN,
                ttl,
                A::new(domain::base::net::Ipv4Addr::from(ip.octets())),
            )
            .compose(tgt),
            Self::AAAA { owner, ip } => Record::new(
                owner,
                Class::IN,
                ttl,
                Aaaa::new(domain::base::net::Ipv6Addr::from(ip.octets())),
            )
            .compose(tgt),
            Self::Ptr { owner, ptr } => {
                Record::new(owner, Class::IN, ttl, Ptr::new(ptr)).compose(tgt)
            }
            Self::Srv {
                owner,
                port,
                target,
            } => Record::new(owner, Class::IN, ttl, Srv::new(0, 0, *port, target)).compose(tgt),
            _ => todo!(),
            // Self::Txt { owner } => {
            //     Record::new(owner, Class::IN, Ttl::from_secs(60), AllRecordData::Txt(&[]))
            //         .compose(target)
            // }
        }
    }
}

impl<O, N> Answer<ParsedName<O>, N> {
    pub fn parse(record: Record<ParsedName<O>, AllRecordData<O, N>>) -> Result<Self, MdnsError>
    where
        O: Octets,
        N: ToName,
    {
        let (owner, data) = record.into_owner_and_data();

        let answer = match data {
            AllRecordData::A(a) => Self::A {
                owner,
                ip: a.addr().octets().into(),
            },
            AllRecordData::Aaaa(aaaa) => Self::AAAA {
                owner,
                ip: aaaa.addr().octets().into(),
            },
            AllRecordData::Ptr(ptr) => Self::Ptr {
                owner,
                ptr: ptr.into_ptrdname(),
            },
            AllRecordData::Srv(srv) => Self::Srv {
                owner,
                port: srv.port(),
                target: srv.into_target(),
            },
            // Rtype::Txt => {
            // }
            _ => todo!(),
        };

        Ok(answer)
    }
}

pub trait Answers {
    type Owner: ToName;
    type Target: ToName;

    fn for_each<F, E>(&self, f: F) -> Result<(), E>
    where
        F: FnMut(&Answer<Self::Owner, Self::Target>) -> Result<(), E>,
        E: From<MdnsError>;

    fn serialize(&self, buf: &mut [u8], ttl_sec: u32) -> Result<usize, MdnsError> {
        let buf = Buf(buf, 0);

        let message = MessageBuilder::from_target(buf)?;

        let mut ab = message.answer();

        //self.set_answer_header(&mut answer);

        self.for_each(|answer| {
            ab.push(answer)?;

            Ok::<_, MdnsError>(())
        })?;

        let buf = ab.finish();

        Ok(buf.1)
    }
}

impl<'a, O: Octets> IntoIterator for Pr<&'a Message<O>> {
    type Item = Answer<ParsedName<O::Range<'a>>, ParsedName<O::Range<'a>>>;
    type IntoIter = Pr<RecordSection<'a, O>>;

    fn into_iter(self) -> Self::IntoIter {
        let answers = self.0.answer().unwrap();

        Pr(answers)
    }
}

pub struct Pr<T>(T);

impl<'a, O: Octets> Iterator for Pr<RecordSection<'a, O>> {
    type Item = Answer<ParsedName<O::Range<'a>>, ParsedName<O::Range<'a>>>;

    fn next(&mut self) -> Option<Self::Item> {
        let answer = self.0.next();

        if let Some(answer) = answer {
            let answer = answer.unwrap();

            let record = answer
                .into_record::<AllRecordData<_, _>>()
                .unwrap()
                .ok_or(MdnsError::InvalidMessage)
                .unwrap();

            let answer = Answer::try_from(record).unwrap();

            Some(answer)
        } else {
            None
        }
    }
}

impl<'a> Answers for &'a [u8] {
    type Owner = ParsedName<&'a [u8]> where Self: 'a;
    type Target = ParsedName<&'a [u8]> where Self: 'a;

    fn for_each<F, E>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(&Answer<Self::Owner, Self::Target>) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let message = Message::from_octets(self).unwrap();

        for answer in message.answer().unwrap() {
            let answer = answer.unwrap();

            let record = answer
                .into_record::<AllRecordData<_, _>>()
                .unwrap()
                .ok_or(MdnsError::InvalidMessage)?;

            let answer = Answer::try_from(record)?;

            f(&answer)?;
        }

        Ok(())
    }
}

//impl<T, F>

// impl<N> Answer<N>
// where
//     N: ToName,
// {
// }

// pub enum Question<'a> {
//     A(&'a str),
//     AAAA(&'a str),
//     Ptr(&'a str),
//     Srv(&'a str),
//     Txt(&'a str),
// }

// // pub enum SimpleQueryDetails<'a> {
// //     Host(&'a str),
// //     Service(&'a str),
// //     ServiceType(&'a str),
// // }

// // pub enum IteratorWrapper<H, S, T> {
// //     Host(H),
// //     Service(S),
// //     ServiceType(T),
// // }

// // impl<'a, H, S, T> Iterator for IteratorWrapper<H, S, T>
// // where
// //     H: Iterator<Item = Question<'a>>,
// //     S: Iterator<Item = Question<'a>>,
// //     T: Iterator<Item = Question<'a>>,
// // {
// //     type Item = Question<'a>;

// //     fn next(&mut self) -> Option<Question<'a>> {
// //         match self {
// //             Self::Host(iter) => iter.next(),
// //             Self::Service(iter) => iter.next(),
// //             Self::ServiceType(iter) => iter.next(),
// //         }
// //     }
// // }

// // impl<'a> SimpleQueryDetails<'a> {
// //     pub fn questions(&self) -> impl Iterator<Item = Question<'_>> {
// //         match self {
// //             SimpleQueryDetails::Host(fqdn) => IteratorWrapper::Host(once(Question::A(fqdn)).chain(once(Question::AAAA(fqdn)))),
// //             SimpleQueryDetails::Service(fqdn) => IteratorWrapper::Service(once(Question::Ptr(fqdn))),
// //             SimpleQueryDetails::ServiceType(fqdn) => IteratorWrapper::ServiceType(once(Question::Ptr(fqdn))),
// //         }
// //     }
// // }

// // pub enum QueryDetails<'a> {
// //     Simple(SimpleQueryDetails<'a>),
// //     Complex(&'a [Question<'a>]),
// // }

// // pub struct Query<'a> {
// //     pub domain: &'a str,
// //     pub query_details: QueryDetails<'a>,
// // }

// impl<'a> Question<'a> {
//     // pub fn ask<'s>(questions: &'a [Self], buf: &mut [u8]) -> Result<usize, MdnsError> {
//     //     let buf = Buf(buf, 0);

//     //     let message = MessageBuilder::from_target(buf)?;

//     //     let mut qb = message.question();

//     //     for question in questions {
//     //         self.push(&mut qb)?;
//     //     }

//     //     let buf = qb.finish();

//     //     Ok(buf.1)
//     // }

//     // fn push(&self, qb: &mut QuestionBuilder<Buf>) -> Result<(), MdnsError> {
//     //     match self {
//     //         Self::A(fqdn) => qb.push(((*fqdn).try_into().unwrap(), Rtype::A, Class::In))?,
//     //         Self::AAAA(fqdn) => qb.push((fqdn.into(), Rtype::Aaaa, Class::In))?,
//     //         Self::Ptr(fqdn) => qb.push((fqdn.into(), Rtype::Ptr, Class::In))?,
//     //         Self::Srv(fqdn) => qb.push((fqdn.into(), Rtype::Srv, Class::In))?,
//     //         Self::Txt(fqdn) => qb.push((fqdn.into(), Rtype::Txt, Class::In))?,
//     //     }

//     //     Ok(())
//     // }

//     // pub fn decode_reply<'s>(&self, data: &[u8], buf: &'s mut [u8]) -> Result<impl Iterator<Item = Answer<'s>>, MdnsError> {
//     //     // if self.set_response(data, services, &mut answer, ttl_sec)? {
//     //     //     let buf = answer.finish();

//     //     //     Ok(buf.1)
//     //     // } else {
//     //     //     Ok(0)
//     //     // }

//     //     // Ok(buf.1)

//     //     let message = Message::from_octets(data)?;

//     //     let mut replied = false;

//     //     let mut host = Host {
//     //         id: 0,
//     //         hostname: "",
//     //         ip: [0, 0, 0, 0],
//     //         ipv6: None,
//     //     };

//     //     let buf = Buf(buf, 0);

//     //     for answer in message.answer()? {
//     //         let answer = answer?;

//     //         //trace!("Handling answer {:?}", answer);

//     //         //let answer = answer?;

//     //         let record = answer.into_record::<AllRecordData<_, _>>()?.ok_or(MdnsError::InvalidMessage)?;

//     //         match record.into_data() {
//     //             AllRecordData::A(a) => {
//     //                 host.ip = a.addr().octets();
//     //             }
//     //             AllRecordData::Aaaa(aaaa) => {
//     //                 host.ipv6 = Some(aaaa.addr().octets());
//     //             }
//     //             AllRecordData::Ptr(ptr) => {
//     //                 //let hostname: Dname<Buf> = ptr.ptrdname();
//     //             }

//     //             // Rtype::Srv => {
//     //             // }
//     //             // Rtype::Ptr => {
//     //             //     let ipv6: Record<_, ParsedDname<_>> = answer.into_record()?.ok_or(MdnsError::InvalidMessage)?;

//     //             // }
//     //             // Rtype::Txt => {
//     //             // }
//     //             AllRecordData::Srv(srv) => {
//     //                 host.id = srv.port();
//     //                 //host.hostname = srv.target().to_string();
//     //             }
//     //             _ => (),
//     //         }
//     //     }

//     //     //Ok(host)
//     //     Ok(core::iter::empty())
//     // }
// }

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
