mod tcp;
mod udp;

use std::{io, net::SocketAddr};

use bytes::Bytes;
use futures_channel::mpsc;
use tokio::net::{lookup_host, ToSocketAddrs};
use tracing_core::subscriber::NoSubscriber;
use tracing_futures::WithSubscriber;

pub use tcp::*;
pub use udp::*;

/// A sequence of [errors](std::io::Error) which occurred during a connection attempt.
///
/// These are paired with a [`SocketAddr`] because the connection attempt might fail multiple times
/// during DNS resolution.
#[derive(Debug)]
pub struct ConnectionErrors(pub Vec<(SocketAddr, io::Error)>);

/// Provides an interface for connecting (and reconnecting) to Graylog. Without an established
/// connection logs will not be sent. Messages logged without an established connection will sit in
/// the buffer until they can be drained.
#[derive(Debug)]
#[must_use]
pub struct ConnectionHandle<A, Conn> {
    pub(crate) addr: A,
    pub(crate) receiver: mpsc::Receiver<Bytes>,
    pub(crate) conn: Conn,
}

impl<A, Conn> ConnectionHandle<A, Conn> {
    /// Returns the connection address.
    pub fn address(&self) -> &A {
        &self.addr
    }
}

impl<A> ConnectionHandle<A, TcpConnection>
where
    A: ToSocketAddrs,
{
    /// Connects to Graylog via TCP using the address provided.
    ///
    /// This will perform DNS resolution and attempt to connect to each [`SocketAddr`] provided.
    pub async fn connect(&mut self) -> ConnectionErrors {
        // Do a DNS lookup if `addr` is a hostname
        let addrs = lookup_host(&self.addr).await.into_iter().flatten();

        // Loop through the IP addresses that the hostname resolved to
        let mut errors = Vec::new();
        for addr in addrs {
            let fut = self
                .conn
                .handle(addr, &mut self.receiver)
                .with_subscriber(NoSubscriber::default());
            if let Err(err) = fut.await {
                errors.push((addr, err));
            }
        }
        ConnectionErrors(errors)
    }
}

#[cfg(feature = "rustls-tls")]
impl<A> ConnectionHandle<A, TlsConnection>
where
    A: ToSocketAddrs,
{
    /// Connects to Graylog via TLS using the address provided.
    ///
    /// This will perform DNS resolution and attempt to connect to each [`SocketAddr`] provided.
    pub async fn connect(&mut self) -> ConnectionErrors {
        // Do a DNS lookup if `addr` is a hostname
        let addrs = lookup_host(&self.addr).await.into_iter().flatten();

        // Loop through the IP addresses that the hostname resolved to
        let mut errors = Vec::new();
        for addr in addrs {
            let fut = self
                .conn
                .handle(addr, &mut self.receiver)
                .with_subscriber(NoSubscriber::default());
            if let Err(err) = fut.await {
                errors.push((addr, err));
            }
        }
        ConnectionErrors(errors)
    }
}

impl<A> ConnectionHandle<A, UdpConnection>
where
    A: ToSocketAddrs,
{
    /// Connects to Graylog via UDP using the address provided.
    ///
    /// This will perform DNS resolution and attempt to connect to each [`SocketAddr`] provided.
    pub async fn connect(&mut self) -> ConnectionErrors {
        // Do a DNS lookup if `addr` is a hostname
        let addrs = lookup_host(&self.addr).await.into_iter().flatten();

        // Loop through the IP addresses that the hostname resolved to
        let mut errors = Vec::new();
        for addr in addrs {
            let fut = self
                .conn
                .handle(addr, &mut self.receiver)
                .with_subscriber(NoSubscriber::default());
            if let Err(err) = fut.await {
                errors.push((addr, err));
            }
        }
        ConnectionErrors(errors)
    }
}
