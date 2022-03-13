use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12201";

    // Initialize subscriber, returning a connection handle
    let conn_handle = Logger::builder().init_tcp(address).unwrap();

    // Spawn background task, this will connect and then forward messages to Graylog
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

    // Don't exit
    loop {}
}
