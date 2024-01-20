# edge-std-nal-async

[![CI](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml/badge.svg)](https://github.com/ivmarkov/edge-net/actions/workflows/ci.yml)
![crates.io](https://img.shields.io/crates/v/edge-net.svg)
[![Documentation](https://docs.rs/edge-net/badge.svg)](https://docs.rs/edge-net)

A stop-gap STD implementation of [embedded-nal-async](https://github.com/rust-embedded-community/embedded-nal/tree/master/embedded-nal-async), *including the extra traits defined in [embedded-nal-async-xtra](../embedded-nal-async-xtra)*.

The implementation is based on the minimalistic [async-io](https://github.com/smol-rs/async-io) crate - from the [smol](https://github.com/smol-rs/smol) async echosystem - and thus works out of the box on a variety of operating systems, including [Espressif's ESP IDF](https://github.com/espressif/esp-idf).

## Plan Forward

Once the traits are upstreamed, the hope is that the "other" - and a bit more known - STD-based embedded-nal-async implementation - [std-embedded-nal-async](https://gitlab.com/chrysn/std-embedded-nal/-/tree/master/std-embedded-nal-async?ref_type=heads) will implement those.

There is also an [open PR](https://gitlab.com/chrysn/std-embedded-nal/-/merge_requests/6) against `std-embedded-nal-async` to remove its dependency on [async-std](https://github.com/async-rs/async-std) and switch to [async-io](https://github.com/smol-rs/async-io).

