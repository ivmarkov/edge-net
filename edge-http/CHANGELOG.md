# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - ????-??-??
* Remove unnecessary lifetimes when implementing the `embedded-svc` traits
* Server: new trait, `TaskHandler` which has an extra `task_id` parameter of type `usize`. This allows the request handling code to take advantage of the fact that - since the number of handlers when running a `Server` instance is fixed - it can store data related to handlers in a simple static array of the same size as the number of handlers that the server is running
* Breaking change: structures `Server` and `ServerBuffers` united, because `Server` was actually stateless. Turbofish syntax for specifying max number of HTTP headers and queue size is no longer necessary
