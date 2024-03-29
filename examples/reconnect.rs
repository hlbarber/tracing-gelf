use std::time::Duration;

use tokio::time::sleep;
use tracing_gelf::Logger;

#[tokio::main]
async fn main() {
    // Graylog address
    let address = "127.0.0.1:12201";

    // Initialize subscriber, returning a connection handle
    let mut conn_handle = Logger::builder().init_tcp(address).unwrap();

    // Reconnection loop
    let reconnect = async move {
        loop {
            // Attempt to connect
            let errors = conn_handle.connect().await;

            // Process errors
            for (socket, error) in errors.0 {
                // Perhaps log errors to an active layer
                tracing::error!(%socket, %error);
            }

            // Don't attempt reconnect immediately
            sleep(Duration::from_secs(5)).await;
        }
    };

    // Spawn background task
    // Any futures executor can be used
    tokio::spawn(reconnect);

    // Send a log to Graylog
    tracing::info!("one day");

    // Don't exit
    loop {}
}
