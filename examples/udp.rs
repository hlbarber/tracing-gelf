use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12202";

    // Initialize subscriber, returning a connection handle
    let mut conn_handle = Logger::builder().init_udp(address).unwrap();

    // Spawn background task, this will connect and then forward messages to Graylog
    // Any futures executor can be used
    tokio::spawn(async move { conn_handle.connect().await });

    // Send a log to Graylog
    tracing::info!(message = "our dreams feel real while we're in them");

    // Create a span
    let span = tracing::info_span!("level 1");
    span.in_scope(|| {
        // Send a log to Graylog, inside a span
        tracing::warn!(message = "we need to go deeper", music = "hans zimmer");

        // Create an nested span
        let inner_span = tracing::info_span!("level 5");
        inner_span.in_scope(|| {
            // Send a log to Graylog, inside a nested span
            tracing::error!(message = "you killed me");
        });
    });

    // Send a log to Graylog
    tracing::info!(message = "he's out", spinning_top = true);

    // Don't exit
    loop {}
}
