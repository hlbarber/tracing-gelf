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
