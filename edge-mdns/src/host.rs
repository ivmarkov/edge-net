use core::net::{Ipv4Addr, Ipv6Addr};

use crate::domain::base::{iana::Class, Record, Ttl};
use crate::domain::rdata::{Aaaa, AllRecordData, Ptr, Srv, A};

use crate::{HostAnswer, HostAnswers, MdnsError, NameLabels, RecordDataChain, Txt, DNS_SD_OWNER};

/// A simple representation of a host that can be used to generate mDNS answers.
///
/// This structure implements the `HostAnswers` trait, which allows it to be used
/// as a responder for mDNS queries coming from other network peers.
#[derive(Debug, Clone)]
pub struct Host<'a> {
    /// The name of the host. I.e. a name "foo" will be pingable as "foo.local"
    pub hostname: &'a str,
    /// The IPv4 address of the host.
    /// Leaving it as `Ipv4Addr::UNSPECIFIED` means that the host will not aswer it to A queries.
    pub ipv4: Ipv4Addr,
    /// The IPv6 address of the host.
    /// Leaving it as `Ipv6Addr::UNSPECIFIED` means that the host will not aswer it to AAAA queries.
    pub ipv6: Ipv6Addr,
    /// The time-to-live of the mDNS answers.
    pub ttl: Ttl,
}

impl<'a> Host<'a> {
    fn visit_answers<F, E>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let owner = &[self.hostname, "local", ""];

        if !self.ipv4.is_unspecified() {
            f(Record::new(
                NameLabels(owner),
                Class::IN,
                self.ttl,
                RecordDataChain::Next(AllRecordData::A(A::new(domain::base::net::Ipv4Addr::from(
                    self.ipv4.octets(),
                )))),
            ))?;
        }

        if !self.ipv6.is_unspecified() {
            f(Record::new(
                NameLabels(owner),
                Class::IN,
                self.ttl,
                RecordDataChain::Next(AllRecordData::Aaaa(Aaaa::new(
                    domain::base::net::Ipv6Addr::from(self.ipv6.octets()),
                ))),
            ))?;
        }

        Ok(())
    }
}

impl<'a> HostAnswers for Host<'a> {
    fn visit<F, E>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.visit_answers(&mut f)
    }
}

/// A simple representation of a DNS-SD service that can be used to generate mDNS answers.
///
/// This structure (indirectly - via the `ServiceAnswers` wraper which also provides the hostname)
/// implements the `HostAnswers` trait, which allows it to be used as a responder for mDNS queries
/// coming from other network peers.
#[derive(Debug, Clone)]
pub struct Service<'a> {
    /// The name of the service.
    pub name: &'a str,
    /// The priority of the service.
    pub priority: u16,
    /// The weight of the service.
    pub weight: u16,
    /// The service type. I.e. "_http"
    pub service: &'a str,
    /// The protocol of the service. I.e. "_tcp" or "_udp"
    pub protocol: &'a str,
    /// The TCP/UDP port where the service listens for incoming requests.
    pub port: u16,
    /// The subtypes of the service, if any.
    pub service_subtypes: &'a [&'a str],
    /// The key-value pairs that will be included in the TXT record, as per the DNS-SD spec.
    pub txt_kvs: &'a [(&'a str, &'a str)],
}

impl<'a> Service<'a> {
    fn visit_answers<F, E>(&self, host: &Host, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        let owner = &[self.name, self.service, self.protocol, "local", ""];
        let stype = &[self.service, self.protocol, "local", ""];
        let target = &[host.hostname, "local", ""];

        f(Record::new(
            NameLabels(owner),
            Class::IN,
            host.ttl,
            RecordDataChain::Next(AllRecordData::Srv(Srv::new(
                self.priority,
                self.weight,
                self.port,
                NameLabels(target),
            ))),
        ))?;

        f(Record::new(
            NameLabels(owner),
            Class::IN,
            host.ttl,
            RecordDataChain::This(Txt::new(self.txt_kvs)),
        ))?;

        f(Record::new(
            DNS_SD_OWNER,
            Class::IN,
            host.ttl,
            RecordDataChain::Next(AllRecordData::Ptr(Ptr::new(NameLabels(stype)))),
        ))?;

        f(Record::new(
            NameLabels(stype),
            Class::IN,
            host.ttl,
            RecordDataChain::Next(AllRecordData::Ptr(Ptr::new(NameLabels(owner)))),
        ))?;

        for subtype in self.service_subtypes {
            let subtype_owner = &[subtype, self.name, self.service, self.protocol, "local", ""];
            let subtype = &[subtype, "_sub", self.service, self.protocol, "local", ""];

            f(Record::new(
                NameLabels(subtype_owner),
                Class::IN,
                host.ttl,
                RecordDataChain::Next(AllRecordData::Ptr(Ptr::new(NameLabels(owner)))),
            ))?;

            f(Record::new(
                NameLabels(subtype),
                Class::IN,
                host.ttl,
                RecordDataChain::Next(AllRecordData::Ptr(Ptr::new(NameLabels(subtype_owner)))),
            ))?;

            f(Record::new(
                DNS_SD_OWNER,
                Class::IN,
                host.ttl,
                RecordDataChain::Next(AllRecordData::Ptr(Ptr::new(NameLabels(subtype)))),
            ))?;
        }

        Ok(())
    }
}

/// A wrapper around a `Service` that also provides the Host of the service
/// and thus allows the `HostAnswers` trait contract to be fullfilled for a `Service` instance.
pub struct ServiceAnswers<'a> {
    host: &'a Host<'a>,
    service: &'a Service<'a>,
}

impl<'a> ServiceAnswers<'a> {
    /// Create a new `ServiceAnswers` instance.
    pub const fn new(host: &'a Host<'a>, service: &'a Service<'a>) -> Self {
        Self { host, service }
    }
}

impl<'a> HostAnswers for ServiceAnswers<'a> {
    fn visit<F, E>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(HostAnswer) -> Result<(), E>,
        E: From<MdnsError>,
    {
        self.service.visit_answers(self.host, &mut f)
    }
}
