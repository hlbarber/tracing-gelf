use std::{future::Future, net::SocketAddr};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use tokio::{io, net::TcpStream};
use tokio_util::codec::{BytesCodec, FramedWrite};

/// Handle TCP connection, generic over TCP/TLS via `F`.
async fn handle_tcp<F, R, S, I>(
    addr: SocketAddr,
    f: F,
    receiver: &mut S,
) -> Result<(), std::io::Error>
where
    S: Stream<Item = Bytes>,
    S: Unpin,
    I: io::AsyncRead + io::AsyncWrite + Send + Unpin,
    F: FnOnce(TcpStream) -> R,
    R: Future<Output = Result<I, std::io::Error>> + Send,
{
    let tcp = TcpStream::connect(addr).await?;
    let wrapped = (f)(tcp).await?;
    let (_, writer) = io::split(wrapped);

    // Writer
    let sink = FramedWrite::new(writer, BytesCodec::new());
    receiver.map(Ok).forward(sink).await?;

    Ok(())
}

/// A TCP connection to Graylog.
#[derive(Debug)]
pub struct TcpConnection;

impl TcpConnection {
    pub(super) async fn handle<S>(
        &self,
        addr: SocketAddr,
        receiver: &mut S,
    ) -> Result<(), std::io::Error>
    where
        S: Stream<Item = Bytes> + Unpin,
    {
        let wrapper = |tcp_stream| async { Ok(tcp_stream) };
        handle_tcp(addr, wrapper, receiver).await
    }
}

/// A TLS connection to Graylog.
#[cfg(feature = "rustls-tls")]
pub struct TlsConnection {
    pub(crate) server_name: rustls_pki_types::ServerName<'static>,
    pub(crate) client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
}

#[cfg(feature = "rustls-tls")]
impl TlsConnection {
    pub(super) async fn handle<S>(
        &self,
        addr: SocketAddr,
        receiver: &mut S,
    ) -> Result<(), std::io::Error>
    where
        S: Stream<Item = Bytes> + Unpin,
    {
        let wrapper = move |tcp_stream| {
            let server_name = self.server_name.clone();
            tokio_rustls::TlsConnector::from(self.client_config.clone())
                .connect(server_name, tcp_stream)
        };
        handle_tcp(addr, wrapper, receiver).await
    }
}

#[cfg(feature = "rustls-tls")]
impl std::fmt::Debug for TlsConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsConnection")
            .field("server_name", &self.server_name)
            .finish_non_exhaustive()
    }
}
