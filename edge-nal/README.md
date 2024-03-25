# edge-nal

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

Hosts a bunch of traits which hopefully will be upstreamed into [embedded-nal-async](https://github.com/rust-embedded-community/embedded-nal/tree/master/embedded-nal-async) soon - in one shape or another.

## Justification

These traits are necessary to unlock the full functionality of some crates in `edge-net`. Namely:
* [edge-mdns](../edge-mdns) - needs UDP multicast capabilities as well as socket splitting
* [edge-dhcp](../edge-dhcp) - needs raw ethernet socket capabilities or at least sending/receiving UDP packets to/from peers identified by their MAC addresses rather than by their IP addresses
* [edge-http](../edge-http) - (full server only) needs a way to bind to a server-side TCP socket
* [edge-ws](../edge-ws) - Most WebSocket use cases do require a splittable TCP socket (separate read and write halves)

## TCP traits
* [TcpSplit](src/stack/tcp.rs)
  * A trait that - when implemented on a TCP socket - allows for splitting the send and receive halves of the socket for full-duplex functionality
* [TcpConnect](src/stack/tcp.rs)
  * Client-side TCP socket factory similar in spirit to STD's `std::net::TcpListener::connect` method
* [TcpAccept](src/stack/tcp.rs)
  * Server-side TCP socket factory similar in spirit to STD's `std::net::TcpListener::bind` method and `std::net::TcpListener` struct
* [TcpStack](src/stack/tcp.rs)
  * `TcpConnect` + `TcpAccept`

## UDP traits
* [UdpReceive](src/udp.rs)
  * The receiver half of a UDP socket
* [UdpSend](src/udp.rs)
  * The sender half of a UDP socket
* [UdpSplit](src/stack/udp.rs)
  * A trait that - when implemented on a UDP socket - allows for splitting the send and receive halves of the socket for full-duplex functionality
* [UdpStack](src/stack/udp.rs)
  * Udp socket factory similar in spirit to STD's `std::net::UdpSocket::bind` method
* [Multicast](src/multicast.rs)
  * Extra trait for UDP sockets allowing subscription to multicast groups

## Traits for sending/receiving raw ethernet payloads (a.k.a. raw sockets)
* [RawReceive](src/raw.rs)
  * The receiver half of a raw socket
* [RawSend](src/raw.rs)
  * The sender half of a raw socket
* [RawSplit](src/stack/raw.rs)
  * A trait that - when implemented on a raw socket - allows for splitting the send and receive halves of the socket for full-duplex functionality
* [RawStack](src/stack/raw.rs)
  * A raw socket factory
