# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-02-01
* Do not compile the `RawSocket` implementation for the ESP IDF, as it is missing the `sockaddr_ll` structure in `libc`
* Retire `StdTcpSocketRef` in favor of simply using `&StdTcpSocket`
* ESP-IDF bugfix: The `TcpAccept` implementation was incorrect, because lwIP - unfortunately - does not support `select`-ing on server-side listening sockets
