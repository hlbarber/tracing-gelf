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
