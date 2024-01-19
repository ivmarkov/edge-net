# edge-net

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

This crate ships async + `no_std` + no-alloc implementations of various network protocols.

Suitable for MCU development, bare-metal in particular.

Supported protocols:
* [Websocket client and server](edge-ws)
* [HTTP client and server](edge-http)
* [MQTT client](edge-mqtt) (just a slim wrapper around `rumqttc`, so currently needs STD)
* [DHCP cient and server](edge-dhcp)
* [DNS Captive Portal](edge-captive)
* [MDNS responder](edge-mdns)

Needs testing & bugfixing:
* HTTP chunked-transfer decoder

PRs welcome!
