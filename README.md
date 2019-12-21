# tracing-gelf

A Graylog [`tracing`](https://github.com/tokio-rs/tracing) library.

[![Build Status](https://travis-ci.org/hlb8122/tracing-gelf.svg?branch=master)](https://travis-ci.org/hlb8122/tracing-gelf)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Cargo](https://img.shields.io/crates/v/tracing-gelf.svg)](https://crates.io/crates/tracing-gelf)
[![Documentation](https://docs.rs/tracing-gelf/badge.svg)](
https://docs.rs/tracing-gelf)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
tracing-gelf = "0.3"
```

### TCP Logging

```rust
use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12201";

    // Start tracing
    let bg_task = Logger::builder().init_tcp(address).unwrap();

    // Spawn background task
    // Any futures executor can be used
    tokio::spawn(bg_task);

    // Send a log to Graylog
    tracing::info!(message = "oooh, what's in here?");

    // Create a span
    let span = tracing::info_span!("cave");
    span.in_scope(|| {
        // Log inside a span
        tracing::info!(message = "oh god, it's dark in here");
    });

    // Log a structured log
    tracing::info!(message = "i'm glad to be out", spook_lvl = 3, ruck_sack = ?["glasses", "inhaler", "large bat"]);

    // Don't exit
    loop {}
}
```

### UDP Logging

```rust
use tracing_gelf::Logger;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    // Graylog address
    let address: SocketAddr = "127.0.0.1:12201".parse().unwrap();

    // Start tracing
    let bg_task = Logger::builder().init_udp(address).unwrap();

    // Spawn background task
    // Any futures executor can be used
    tokio::spawn(bg_task);

    // Send a log to Graylog
    tracing::info!(message = "you need to go home little guy", spook_lvl = 3, ruck_sack = ?["glasses", "inhaler", "large bat"]);

    // Create a span
    let span = tracing::info_span!("cave");
    span.in_scope(|| {
        // Log inside a span
        tracing::info!(message = "be free", spook_lvl = 5, ruck_sack = ?["glasses", "inhaler"]);
    });

    // Log a structured log
    tracing::info!(message = "time to go home", ruck_sack = ?["glasses", "inhaler", "large spider"]);

    // Don't exit
    loop {}
}
```
