# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased
* Updated dependencies for compatibility with `embassy-time-driver` v0.2

## [0.4.0] - 2024-01-02
* Reduce logging level (#32)
* Support for Captive Portal URLs (#31)
* Option to erase the generics from the IO errors
* Make embassy-time optional

## [0.3.0] - 2024-09-10
* Migrated the client and the server to the `edge-nal` traits
* Migrated the server to only require `UdpSend` and `UdpReceive`, without the need to manipulate raw IP payloads anymore
* Raised MSRV to 1.77

