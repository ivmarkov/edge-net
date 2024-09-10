# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2024-09-10
Almost a complete rewrite:
* New query API (`Mdns::query`) complementing the responder / query answers' processing one (`Mdns::run`)
* `domain` API is now also a public API of `edge-mdns`, re-exported as `edge_mdns::domain`
* IO layer now uses the UDP traits from `edge-net`
* Traits:
  * `MdnsHandler` - abstracts the overall processing of an incoming mDNS message
  * `HostAnswers` - abstracts the generation of answers to peer queries (implemented by the pre-existing `Host` and `Service` struct types)
  * `HostQuestions` - , `PeerAnswers`
Smaller items:
* Raised the `domain` dependency to 0.10
* Optimized memory usage by avoiding on-stack allocation of large `heapless::String`s
* IO layer of `edge-mdns` can now share its buffers with other code (see the `BufferAccess` trait)
* Raised MSRV to 1.77
