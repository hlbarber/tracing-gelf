use tracing_gelf::TcpLogger;

#[tokio::main]
async fn main() {
    // Init tracing
    let x = TcpLogger::builder().connect("127.0.0.1").unwrap();
    tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(x))
        .expect("happen, happening, happened");

    let span = tracing::info_span!("my span");
    span.in_scope(|| {
        tracing::info!(short_message = "hello there");
    });

    tracing::info!(short_message = "hello there");
}
