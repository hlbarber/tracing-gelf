use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Init tracing
    let bg_task = Logger::builder()
        .init_tcp("10.1.1.221:12201")
        .unwrap();
    tokio::spawn(bg_task);

    let span = tracing::info_span!("my span");
    span.in_scope(|| {
        tracing::info!(short_message = "hello there");
    });

    tracing::info!(short_message = "hello there");

    loop {}
}
