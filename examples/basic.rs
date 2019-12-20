use tracing_gelf::TcpLogger;

#[tokio::main]
async fn main() {
    // Init tracing
    let (logger, bg_task) = TcpLogger::builder()
        .connect("10.1.1.221:12201", 300)
        .unwrap();
    tokio::spawn(bg_task);
    tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(logger))
        .expect("happen, happening, happened");

    let span = tracing::info_span!("my span");
    span.in_scope(|| {
        tracing::info!(short_message = "hello there");
    });

    tracing::info!(short_message = "hello there");

    loop {}
}
