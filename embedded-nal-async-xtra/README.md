# embedded-nal-async-xtra

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

A placeholder for a bunch of traits which hopefully will be upstreamed into [embedded-nal-async](https://github.com/rust-embedded-community/embedded-nal/tree/master/embedded-nal-async) soon - in one shape or another.

## Justification

These traits are necessary to unlock the full functionality of some crates in `edge-net`. Namely:
* [edge-mdns](../edge-mdns) - needs UDP multicast capabilities
* [edge-dhcp](../edge-dhcp) - needs raw ethernet socket capabilities or at least sending/receiving UDP packets to/from peers identified by their MAC addresses rather than by their IP addresses
* [edge-http](../edge-http) - (full server only) needs a way to bind to a server-side TCP socket

## TCP traits

* [TcpListen](src/stack/tcp.rs)
  * Server-side TCP socket similar in spirit to STD's `std::net::TcpListener::bind` bind method
* [TcpAccept](src/stack/tcp.rs)
  * Server-side TCP socket similar in spirit to STD's `std::net::TcpListener` struct

## UDP traits
* [Multicast](src/stack/multicast.rs)
  * Extra trait for UDP sockets allowing subscription to multicast groups
* [UnconnectedUdpWithMac](src/stack/udp.rs)
  * Extra trait for unconnected UDP sockets allowing broadcasting packets to specific Ethernet MACs
  * Additionally - when receiving packets - this trait provides the sender's MAC in addition to its socket address

## Traits for sending/receiving raw ethernet payloads (a.k.a. raw sockets)

* [RawStack](src/stack/raw.rs)
  * Similar in spirit to `UdpStack`, yet allowing sending/receiving complete IPv4 and IPv6 frames, rather than just UDP packets
* [RawSocket](src/stack/raw.rs)
  * The socket type for `RawStack`. Similar in spirit to `UnconnectedUdp`
