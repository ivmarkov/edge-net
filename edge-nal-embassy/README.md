# edge-nal-embassy

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

A bare-metal implementation of `edge-nal` based on the [embassy-net](https://crates.io/crates/embassy-net) crate - the networking stack of the Embassy ecosystem.

## Implemented Traits

### TCP

All traits except `Readable` which - while implemented - panics if called.

### UDP

* All traits except `UdpConnect`. 
* `MulticastV6` - while implemented - panics if `join_v6` / `leave_v6` are called.
* `Readable` - while implemented - panics if called.

### Raw sockets

Not implemented yet, as `embassy-net` does not expose raw sockets

## Status

**Needs testing!**
