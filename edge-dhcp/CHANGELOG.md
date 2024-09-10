# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2024-09-10
* Migrated the client and the server to the `edge-nal` traits
* Migrated the server to only require `UdpSend` and `UdpReceive`, without the need to manipulate raw IP payloads anymore
* Raised MSRV to 1.77

