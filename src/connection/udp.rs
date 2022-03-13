use std::net::SocketAddr;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use tokio::net::UdpSocket;
use tokio_util::{codec::BytesCodec, udp::UdpFramed};

/// A UDP connection to Graylog.
#[derive(Debug)]
pub struct UdpConnection;

impl UdpConnection {
    pub(super) async fn handle<S>(
        &self,
        addr: SocketAddr,
        receiver: &mut S,
    ) -> Result<(), std::io::Error>
    where
        S: Stream<Item = Bytes>,
        S: Unpin,
    {
        // Bind address version must match address version
        let bind_addr = if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        // Try connect
        let udp_socket = UdpSocket::bind(bind_addr).await?;

        // Writer
        let udp_stream = UdpFramed::new(udp_socket, BytesCodec::new());
        let (sink, _) = udp_stream.split();
        receiver
            .map(|bytes| Ok((bytes, addr)))
            .forward(sink)
            .await?;

        Ok(())
    }
}
