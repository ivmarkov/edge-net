use core::net::{Ipv4Addr, Ipv6Addr};

use domain::base::{Message, MessageBuilder, ToName};

use crate::domain::base::{iana::Class, Record, Ttl};
use crate::domain::rdata::{Aaaa, AllRecordData, Ptr, Srv, A};
use crate::{Buf, MdnsError, NameLabels};

const DNS_SD_OWNER: NameLabels = NameLabels(&["_services", "_dns-sd", "_udp", "local"]);

pub type HostAnswer<'a> = Record<NameLabels<'a>, AllRecordData<&'a [u8], NameLabels<'a>>>;

pub trait HostAnswers {
    fn visit<F, E>(&self, ttl: Ttl, f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>;
}

impl<T> HostAnswers for &T
where
    T: HostAnswers,
{
    fn visit<F, E>(&self, ttl: Ttl, f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        (*self).visit(ttl, f)
    }
}

impl<T> HostAnswers for &mut T
where
    T: HostAnswers,
{
    fn visit<F, E>(&self, ttl: Ttl, f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        (**self).visit(ttl, f)
    }
}

pub struct NoHostAnswers;

impl NoHostAnswers {
    pub fn chain<T>(answers: T) -> ChainedHostAnswers<T, Self> {
        ChainedHostAnswers::new(answers, Self)
    }
}

impl HostAnswers for NoHostAnswers {
    fn visit<F, E>(&self, _ttl: Ttl, _f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
    {
        Ok(())
    }
}

pub struct ChainedHostAnswers<T, U> {
    first: T,
    second: U,
}

impl<T, U> ChainedHostAnswers<T, U> {
    pub const fn new(first: T, second: U) -> Self {
        Self { first, second }
    }

    pub fn chain<V>(self, answers: V) -> ChainedHostAnswers<Self, V> {
        ChainedHostAnswers::new(self, answers)
    }
}

impl<T, U> HostAnswers for ChainedHostAnswers<T, U>
where
    T: HostAnswers,
    U: HostAnswers,
{
    fn visit<F, E>(&self, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.first.visit(ttl, &mut f)?;
        self.second.visit(ttl, f)
    }
}

#[derive(Debug, Clone)]
pub struct Host<'a> {
    pub id: u16,
    pub hostname: &'a str,
    pub ip: Ipv4Addr,
    pub ipv6: Option<Ipv6Addr>,
}

impl<'a> Host<'a> {
    pub fn visit_answers<F, E>(&self, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let owner = &[self.hostname, "local"];

        f(Record::new(
            NameLabels(owner),
            Class::IN,
            ttl,
            AllRecordData::A(A::new(domain::base::net::Ipv4Addr::from(self.ip.octets()))),
        ))?;

        if let Some(ipv6) = self.ipv6 {
            f(Record::new(
                NameLabels(owner),
                Class::IN,
                ttl,
                AllRecordData::Aaaa(Aaaa::new(domain::base::net::Ipv6Addr::from(ipv6.octets()))),
            ))?;
        }

        Ok(())
    }

    // pub fn broadcast<'s, T>(
    //     &self,
    //     services: T,
    //     buf: &mut [u8],
    //     ttl_sec: u32,
    // ) -> Result<usize, MdnsError>
    // where
    //     T: IntoIterator<Item = Service<'s>> + Clone,
    // {
    //     let buf = Buf(buf, 0);

    //     let message = MessageBuilder::from_target(buf)?;

    //     let mut answer = message.answer();

    //     self.set_broadcast(services, &mut answer, ttl_sec)?;

    //     let buf = answer.finish();

    //     Ok(buf.1)
    // }

    // pub fn respond<'s, T>(
    //     &self,
    //     services: T,
    //     data: &[u8],
    //     buf: &mut [u8],
    //     ttl_sec: u32,
    // ) -> Result<usize, MdnsError>
    // where
    //     T: IntoIterator<Item = Service<'s>> + Clone,
    // {
    //     let buf = Buf(buf, 0);

    //     let message = MessageBuilder::from_target(buf)?;

    //     let mut answer = message.answer();

    //     if self.set_response(data, services, &mut answer, ttl_sec)? {
    //         let buf = answer.finish();

    //         Ok(buf.1)
    //     } else {
    //         Ok(0)
    //     }
    // }

    // fn set_broadcast<'s, T, F>(
    //     &self,
    //     services: F,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), MdnsError>
    // where
    //     T: Composer,
    //     F: IntoIterator<Item = Service<'s>> + Clone,
    // {
    //     self.set_header(answer);

    //     self.add_ipv4(answer, ttl_sec)?;
    //     self.add_ipv6(answer, ttl_sec)?;

    //     for service in services.clone() {
    //         service.add_service(answer, self.hostname, ttl_sec)?;
    //         service.add_service_type(answer, ttl_sec)?;
    //         service.add_service_subtypes(answer, ttl_sec)?;
    //         service.add_txt(answer, ttl_sec)?;
    //     }

    //     Ok(())
    // }

    // fn set_response<'s, T, F>(
    //     &self,
    //     data: &[u8],
    //     services: F,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<bool, MdnsError>
    // where
    //     T: Composer,
    //     F: IntoIterator<Item = Service<'s>> + Clone,
    // {
    //     self.set_header(answer);

    //     let message = Message::from_octets(data)?;

    //     let mut replied = false;

    //     for question in message.question() {
    //         trace!("Handling question {:?}", question);

    //         let question = question?;

    //         match question.qtype() {
    //             Rtype::A
    //                 if question
    //                     .qname()
    //                     .name_eq(&Host::host_fqdn(self.hostname, true)?) =>
    //             {
    //                 self.add_ipv4(answer, ttl_sec)?;
    //                 replied = true;
    //             }
    //             Rtype::Aaaa
    //                 if question
    //                     .qname()
    //                     .name_eq(&Host::host_fqdn(self.hostname, true)?) =>
    //             {
    //                 self.add_ipv6(answer, ttl_sec)?;
    //                 replied = true;
    //             }
    //             Rtype::Srv => {
    //                 for service in services.clone() {
    //                     if question.qname().name_eq(&service.service_fqdn(true)?) {
    //                         self.add_ipv4(answer, ttl_sec)?;
    //                         self.add_ipv6(answer, ttl_sec)?;
    //                         service.add_service(answer, self.hostname, ttl_sec)?;
    //                         replied = true;
    //                     }
    //                 }
    //             }
    //             Rtype::Ptr => {
    //                 for service in services.clone() {
    //                     if question.qname().name_eq(&Service::dns_sd_fqdn(true)?) {
    //                         service.add_service_type(answer, ttl_sec)?;
    //                         replied = true;
    //                     } else if question.qname().name_eq(&service.service_type_fqdn(true)?) {
    //                         // TODO
    //                         self.add_ipv4(answer, ttl_sec)?;
    //                         self.add_ipv6(answer, ttl_sec)?;
    //                         service.add_service(answer, self.hostname, ttl_sec)?;
    //                         service.add_service_type(answer, ttl_sec)?;
    //                         service.add_service_subtypes(answer, ttl_sec)?;
    //                         service.add_txt(answer, ttl_sec)?;
    //                         replied = true;
    //                     }
    //                 }
    //             }
    //             Rtype::Txt => {
    //                 for service in services.clone() {
    //                     if question.qname().name_eq(&service.service_fqdn(true)?) {
    //                         service.add_txt(answer, ttl_sec)?;
    //                         replied = true;
    //                     }
    //                 }
    //             }
    //             Rtype::Any => {
    //                 // A / AAAA
    //                 if question
    //                     .qname()
    //                     .name_eq(&Host::host_fqdn(self.hostname, true)?)
    //                 {
    //                     self.add_ipv4(answer, ttl_sec)?;
    //                     self.add_ipv6(answer, ttl_sec)?;
    //                     replied = true;
    //                 }

    //                 // PTR
    //                 for service in services.clone() {
    //                     if question.qname().name_eq(&Service::dns_sd_fqdn(true)?) {
    //                         service.add_service_type(answer, ttl_sec)?;
    //                         replied = true;
    //                     } else if question.qname().name_eq(&service.service_type_fqdn(true)?) {
    //                         // TODO
    //                         self.add_ipv4(answer, ttl_sec)?;
    //                         self.add_ipv6(answer, ttl_sec)?;
    //                         service.add_service(answer, self.hostname, ttl_sec)?;
    //                         service.add_service_type(answer, ttl_sec)?;
    //                         service.add_service_subtypes(answer, ttl_sec)?;
    //                         service.add_txt(answer, ttl_sec)?;
    //                         replied = true;
    //                     }
    //                 }

    //                 // SRV
    //                 for service in services.clone() {
    //                     if question.qname().name_eq(&service.service_fqdn(true)?) {
    //                         self.add_ipv4(answer, ttl_sec)?;
    //                         self.add_ipv6(answer, ttl_sec)?;
    //                         service.add_service(answer, self.hostname, ttl_sec)?;
    //                         replied = true;
    //                     }
    //                 }
    //             }
    //             _ => (),
    //         }
    //     }

    //     Ok(replied)
    // }

    // fn set_header<T: Composer>(&self, answer: &mut AnswerBuilder<T>) {
    //     let header = answer.header_mut();
    //     header.set_id(self.id);
    //     header.set_opcode(domain::base::iana::Opcode::Query);
    //     header.set_rcode(domain::base::iana::Rcode::NoError);

    //     let mut flags = Flags::new();
    //     flags.qr = true;
    //     flags.aa = true;
    //     header.set_flags(flags);
    // }

    // fn add_ipv4<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     answer.push((
    //         Self::host_fqdn(self.hostname, false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         A::from_octets(self.ip[0], self.ip[1], self.ip[2], self.ip[3]),
    //     ))
    // }

    // fn add_ipv6<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     if let Some(ip) = &self.ipv6 {
    //         answer.push((
    //             Self::host_fqdn(self.hostname, false).unwrap(),
    //             Class::In,
    //             ttl_sec,
    //             Aaaa::new((*ip).into()),
    //         ))
    //     } else {
    //         Ok(())
    //     }
    // }

    // fn host_fqdn(hostname: &str, suffix: bool) -> Result<impl ToDname, FromStrError> {
    //     let suffix = if suffix { "." } else { "" };

    //     let mut host_fqdn = heapless07::String::<60>::new();
    //     write!(host_fqdn, "{}.local{}", hostname, suffix,).unwrap();

    //     Dname::<heapless07::Vec<u8, 64>>::from_chars(host_fqdn.chars())
    // }
}

impl<'a> HostAnswers for Host<'a> {
    fn visit<F, E>(&self, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.visit_answers(ttl, &mut f)
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
    pub fn visit_answers<F, E>(&self, hostname: &'a str, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let owner = &[self.name, self.service, self.protocol, "local"];
        let target = &[hostname, "local"];

        f(Record::new(
            NameLabels(owner),
            Class::IN,
            ttl,
            AllRecordData::Srv(Srv::new(0, 0, self.port, NameLabels(target))),
        ))?;

        f(Record::new(
            DNS_SD_OWNER,
            Class::IN,
            ttl,
            AllRecordData::Ptr(Ptr::new(NameLabels(owner))),
        ))?;

        for subtype in self.service_subtypes {
            let subtype_owner = &[subtype, self.name, self.service, self.protocol, "local"];

            f(Record::new(
                NameLabels(subtype_owner),
                Class::IN,
                ttl,
                AllRecordData::Srv(Srv::new(0, 0, self.port, NameLabels(owner))),
            ))?;

            f(Record::new(
                DNS_SD_OWNER,
                Class::IN,
                ttl,
                AllRecordData::Ptr(Ptr::new(NameLabels(subtype_owner))),
            ))?;
        }

        // TODO: TXT

        Ok(())
    }

    // fn add_service<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     hostname: &str,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     answer.push((
    //         self.service_fqdn(false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         Srv::new(0, 0, self.port, Host::host_fqdn(hostname, false).unwrap()),
    //     ))
    // }

    // fn add_service_type<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     answer.push((
    //         Self::dns_sd_fqdn(false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         Ptr::new(self.service_type_fqdn(false).unwrap()),
    //     ))?;

    //     answer.push((
    //         self.service_type_fqdn(false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         Ptr::new(self.service_fqdn(false).unwrap()),
    //     ))
    // }

    // fn add_service_subtypes<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     for service_subtype in self.service_subtypes {
    //         self.add_service_subtype(answer, service_subtype, ttl_sec)?;
    //     }

    //     Ok(())
    // }

    // fn add_service_subtype<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     service_subtype: &str,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     answer.push((
    //         Self::dns_sd_fqdn(false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         Ptr::new(self.service_subtype_fqdn(service_subtype, false).unwrap()),
    //     ))?;

    //     answer.push((
    //         self.service_subtype_fqdn(service_subtype, false).unwrap(),
    //         Class::In,
    //         ttl_sec,
    //         Ptr::new(self.service_fqdn(false).unwrap()),
    //     ))
    // }

    // fn add_txt<T: Composer>(
    //     &self,
    //     answer: &mut AnswerBuilder<T>,
    //     ttl_sec: u32,
    // ) -> Result<(), PushError> {
    //     // only way I found to create multiple parts in a Txt
    //     // each slice is the length and then the data
    //     let mut octets = heapless07::Vec::<_, 256>::new();
    //     //octets.append_slice(&[1u8, b'X'])?;
    //     //octets.append_slice(&[2u8, b'A', b'B'])?;
    //     //octets.append_slice(&[0u8])?;
    //     for (k, v) in self.txt_kvs {
    //         octets.append_slice(&[(k.len() + v.len() + 1) as u8])?;
    //         octets.append_slice(k.as_bytes())?;
    //         octets.append_slice(&[b'='])?;
    //         octets.append_slice(v.as_bytes())?;
    //     }

    //     let txt = Txt::from_octets(&mut octets).unwrap();

    //     answer.push((self.service_fqdn(false).unwrap(), Class::In, ttl_sec, txt))
    // }

    // fn service_fqdn(&self, suffix: bool) -> Result<impl ToDname, FromStrError> {
    //     let suffix = if suffix { "." } else { "" };

    //     let mut service_fqdn = heapless07::String::<60>::new();
    //     write!(
    //         service_fqdn,
    //         "{}.{}.{}.local{}",
    //         self.name, self.service, self.protocol, suffix,
    //     )
    //     .unwrap();

    //     Dname::<heapless07::Vec<u8, 64>>::from_chars(service_fqdn.chars())
    // }

    // fn service_type_fqdn(&self, suffix: bool) -> Result<impl ToDname, FromStrError> {
    //     let suffix = if suffix { "." } else { "" };

    //     let mut service_type_fqdn = heapless07::String::<60>::new();
    //     write!(
    //         service_type_fqdn,
    //         "{}.{}.local{}",
    //         self.service, self.protocol, suffix,
    //     )
    //     .unwrap();

    //     Dname::<heapless07::Vec<u8, 64>>::from_chars(service_type_fqdn.chars())
    // }

    // fn service_subtype_fqdn(
    //     &self,
    //     service_subtype: &str,
    //     suffix: bool,
    // ) -> Result<impl ToDname, FromStrError> {
    //     let suffix = if suffix { "." } else { "" };

    //     let mut service_subtype_fqdn = heapless07::String::<40>::new();
    //     write!(
    //         service_subtype_fqdn,
    //         "{}._sub.{}.{}.local{}",
    //         service_subtype, self.service, self.protocol, suffix,
    //     )
    //     .unwrap();

    //     Dname::<heapless07::Vec<u8, 64>>::from_chars(service_subtype_fqdn.chars())
    // }

    // fn dns_sd_fqdn(suffix: bool) -> Result<impl ToDname, FromStrError> {
    //     Dname::<heapless07::Vec<u8, 64>>::from_chars(
    //         if suffix {
    //             "_services._dns-sd._udp.local."
    //         } else {
    //             "_services._dns-sd._udp.local"
    //         }
    //         .chars(),
    //     )
    // }
}

pub struct ServiceAnswers<'a> {
    hostname: &'a str,
    service: &'a Service<'a>,
}

impl<'a> HostAnswers for ServiceAnswers<'a> {
    fn visit<F, E>(&self, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.service.visit_answers(self.hostname, ttl, &mut f)
    }
}

pub trait Services {
    fn visit_services<F, E>(&self, f: F) -> Result<(), E>
    where
        F: FnMut(&Service) -> Result<(), E>,
        E: From<MdnsError>;

    fn visit_answers<F, E>(&self, hostname: &str, ttl: Ttl, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.visit_services(|service| service.visit_answers(hostname, ttl, &mut f))
    }
}

impl<T> Services for &mut T
where
    T: Services,
{
    fn visit_services<F, E>(&self, f: F) -> Result<(), E>
    where
        F: FnMut(&Service) -> Result<(), E>,
        E: From<MdnsError>,
    {
        (**self).visit_services(f)
    }
}

impl<T> Services for &T
where
    T: Services,
{
    fn visit_services<F, E>(&self, f: F) -> Result<(), E>
    where
        F: FnMut(&Service) -> Result<(), E>,
        E: From<MdnsError>,
    {
        (*self).visit_services(f)
    }
}

pub struct ServicesAnswers<'a, T> {
    host: &'a str,
    services: T,
}

impl<'a, T> HostAnswers for ServicesAnswers<'a, T>
where
    T: Services,
{
    fn visit<F, E>(&self, ttl: Ttl, f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.services.visit_answers(self.host, ttl, f)
    }
}

/// Handles an incoming mDNS message by parsing it and potentially preparing a response.
///
/// If incoming is `None`, the handler should prepare a broadcast message with
/// all its data.
///
/// Returns the length of the response message.
/// If length is 0, the IO layer using the handler should not send a message.
pub trait MdnsHandler {
    fn handle(&self, incoming: Option<&[u8]>, buf: &mut [u8], ttl: Ttl)
        -> Result<usize, MdnsError>;
}

impl<T> MdnsHandler for &T
where
    T: MdnsHandler,
{
    fn handle(
        &self,
        incoming: Option<&[u8]>,
        buf: &mut [u8],
        ttl: Ttl,
    ) -> Result<usize, MdnsError> {
        (*self).handle(incoming, buf, ttl)
    }
}

impl<T> MdnsHandler for &mut T
where
    T: MdnsHandler,
{
    fn handle(
        &self,
        incoming: Option<&[u8]>,
        buf: &mut [u8],
        ttl: Ttl,
    ) -> Result<usize, MdnsError> {
        (**self).handle(incoming, buf, ttl)
    }
}

pub struct NoHandler;

impl NoHandler {
    pub fn chain<T>(handler: T) -> ChainedHandler<T, Self> {
        ChainedHandler::new(handler, Self)
    }
}

impl MdnsHandler for NoHandler {
    fn handle(
        &self,
        _incoming: Option<&[u8]>,
        _buf: &mut [u8],
        _ttl: Ttl,
    ) -> Result<usize, MdnsError> {
        Ok(0)
    }
}

pub struct ChainedHandler<T, U> {
    first: T,
    second: U,
}

impl<T, U> ChainedHandler<T, U> {
    pub const fn new(first: T, second: U) -> Self {
        Self { first, second }
    }

    pub fn chain<V>(self, handler: V) -> ChainedHandler<Self, V> {
        ChainedHandler::new(self, handler)
    }
}

impl<T, U> MdnsHandler for ChainedHandler<T, U>
where
    T: MdnsHandler,
    U: MdnsHandler,
{
    fn handle(
        &self,
        incoming: Option<&[u8]>,
        buf: &mut [u8],
        ttl: Ttl,
    ) -> Result<usize, MdnsError> {
        let len = self.first.handle(incoming, buf, ttl)?;

        if len == 0 {
            self.second.handle(incoming, buf, ttl)
        } else {
            Ok(len)
        }
    }
}

pub struct HostAnswersMdnsHandler<T> {
    answers: T,
}

impl<T> MdnsHandler for HostAnswersMdnsHandler<T>
where
    T: HostAnswers,
{
    fn handle(
        &self,
        incoming: Option<&[u8]>,
        buf: &mut [u8],
        ttl: Ttl,
    ) -> Result<usize, MdnsError> {
        let buf = Buf(buf, 0);

        let mb = MessageBuilder::from_target(buf)?;
        let mut ab = mb.answer();

        let mut pushed = false;

        if let Some(incoming) = incoming {
            let message = Message::from_octets(incoming)?;

            for question in message.question() {
                let question = question?;

                self.answers.visit(ttl, |answer| {
                    if question.qname().name_eq(answer.owner()) {
                        ab.push(answer)?;

                        pushed = true;
                    }

                    Ok::<_, MdnsError>(())
                })?;
            }
        } else {
            self.answers.visit(ttl, |answer| {
                ab.push(answer)?;

                pushed = true;

                Ok::<_, MdnsError>(())
            })?;
        }

        let buf = ab.finish();

        if pushed {
            Ok(buf.1)
        } else {
            Ok(0)
        }
    }
}
