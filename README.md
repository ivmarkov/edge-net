# edge-net

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

This crate ships async + `no_std` + no-alloc implementations of various network protocols.

Suitable for microcontrollers and embedded systems in general.

Supported protocols:
* [HTTP client and server](edge-http)
* [Websocket send/receive](edge-ws)
* [DNS Captive Portal](edge-captive)
* [mDNS responder](edge-mdns)
* [DHCP cient and server](edge-dhcp)
* [Raw IP & UDP packet send/receive](edge-raw) (useful in combination with the DHCP client and server)
* [MQTT client](edge-mqtt) (currently just a slim wrapper around [`rumqttc`](https://github.com/bytebeamio/rumqtt/tree/main/rumqttc), so needs STD)

PRs welcome!
