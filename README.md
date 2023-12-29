# edge-net

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

This crate ships async + `no_std` + no-alloc implementations of various network protocols.

Suitable for MCU development, bare-metal in particular.

Working:
* WS client
* MQTT client (just a slim wrapper around `rumqttc`, so currently needs STD)
* DHCP
* DNS Captive Portal
* MDNS responder

Needs bugfixing:
* HTTP / WS server
* HTTP client

PRs welcome!
