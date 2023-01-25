use edge_net::captive::{DnsConf, DnsServer};
fn main() {
    let mut dns_conf = DnsConf::new("192.168.71.1".parse().unwrap());
    dns_conf.bind_port = 1053;
    let mut dns_server = DnsServer::new(dns_conf);
    dns_server.start().unwrap();
    loop {}
}
