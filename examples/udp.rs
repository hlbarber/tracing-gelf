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

    // Don't exit
    loop {}
}
