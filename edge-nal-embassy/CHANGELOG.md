# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2025-01-15
* Updated dependencies for compatibility with `embassy-time-driver` v0.2

## [0.4.1] - 2025-01-05
* Fix regression: ability to UDP/TCP bind to socket 0.0.0.0

## [0.4.0] - 2024-01-02
* Proper TCP socket shutdown; Generic TCP timeout utils; built-in HTTP server timeouts; update docu (#34)
* fix a typo (#44)
* Document the N generic for Udp as done for Tcp (#47)
* Update to embassy-net 0.5 (#50)

## [0.3.0] - 2024-09-10
* First release (with version 0.3.0 to align with the other `edge-net` crates)
