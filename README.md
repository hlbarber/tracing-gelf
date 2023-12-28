# tracing-gelf

A Graylog [`tracing`](https://github.com/tokio-rs/tracing) library.

[![CI](https://img.shields.io/github/actions/workflow/status/hlbarber/tracing-gelf/ci.yml?label=build&logo=github)](https://github.com/hlbarber/tracing-gelf/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Cargo](https://img.shields.io/crates/v/tracing-gelf.svg)](https://crates.io/crates/tracing-gelf)
[![Documentation](https://docs.rs/tracing-gelf/badge.svg)](
https://docs.rs/tracing-gelf)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
tracing-gelf = "0.8.0"
```

### TCP Logging

```rust
use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12201";

    // Initialize subscriber
    let conn_handle = Logger::builder().init_tcp(address).unwrap();

    // Spawn background task
    // Any futures executor can be used
    tokio::spawn(conn_handle.connect());

    // Send a log to Graylog
    tracing::info!(message = "oooh, what's in here?");

    // Create a span
    let span = tracing::info_span!("cave");
    span.in_scope(|| {
        let test = tracing::info_span!("deeper in cave", smell = "damp");
        test.in_scope(|| {
            // Send a log to Graylog, inside a nested span
            tracing::warn!(message = "oh god, it's dark in here");
        })
    });

    // Send a log to Graylog
    tracing::error!(message = "i'm glad to be out", spook_lvl = 3, ruck_sack = ?["glasses", "inhaler", "large bat"]);
}

```

### UDP Logging

```rust
use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12202";

    // Start tracing
    let bg_task = Logger::builder().init_udp(address).unwrap();

    // Spawn background task
    // Any futures executor can be used
    tokio::spawn(bg_task);

    // Send a log to Graylog
    tracing::info!(message = "our dreams feel real while we're in them");

    // Create a span
    let span = tracing::info_span!("level 1");
    span.in_scope(|| {
        // Log inside a span
        tracing::warn!(message = "we need to go deeper", music = "hans zimmer");

        // Create an nested span
        let inner_span = tracing::info_span!("level 5");
        inner_span.in_scope(|| {
            // Log inside nested span
            tracing::error!(message = "you killed me");
        });
    });

    // Log a structured log
    tracing::info!(message = "he's out", spinning_top = true);
}

```
