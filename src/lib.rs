#![warn(
    missing_debug_implementations,
    missing_docs,
    rust_2018_idioms,
    unreachable_pub
)]

//! Provides Graylog structured logging using the [`tracing`].
//!
//! # Usage
//!
//! ```rust
//! use std::net::SocketAddr;
//! use tracing_gelf::Logger;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Graylog address
//!     let address: SocketAddr = "127.0.0.1:12201".parse().unwrap();
//!
//!     // Start tracing
//!     let bg_task = Logger::builder().init_tcp(address).unwrap();
//!
//!     // Spawn background task
//!     // Any futures executor can be used
//!     tokio::spawn(bg_task);
//!
//!     // Send a log to Graylog
//!     tracing::info!(message = "oooh, what's in here?");
//!
//!     // Create a span
//!     let span = tracing::info_span!("cave");
//!     span.in_scope(|| {
//!         // Log inside a span
//!         let test = tracing::info_span!("deeper in cave", smell = "damp");
//!         test.in_scope(|| {
//!             tracing::warn!(message = "oh god, it's dark in here");
//!         })
//!     });
//!
//!     // Log a structured log
//!     tracing::error!(message = "i'm glad to be out", spook_lvl = 3, ruck_sack = ?["glasses", "inhaler", "large bat"]);
//!
//! }
//! ```
//!
//! # GELF Encoding
//!
//! [`Events`] are encoded into [`GELF format`](https://docs.graylog.org/en/3.1/pages/gelf.html)
//! as follows:
//! * [`Event`] fields are inserted as [`GELF`] additional fields, `_field_name`.
//! * [`Event`] field named `message` is renamed to `short_message`.
//! * If `short_message` (or `message`) [`Event`] field is missing then `short_message` is
//! set to the empty string.
//! * [`Event`] fields whose names collide with [`GELF`] required fields are coerced
//! into the required types and overrides defaults given in the builder.
//! * The hierarchy of spans is concatenated and inserted as `span_a:span_b:span_c` and
//! inserted as an additional field `_span`.
//!
//! [`tracing`]: https://docs.rs/tracing
//! [`Event`]: https://docs.rs/tracing/0.1.11/tracing/struct.Event.html
//! [`Events`]: https://docs.rs/tracing/0.1.11/tracing/struct.Event.html
//! [`GELF`]: https://docs.graylog.org/en/3.1/pages/gelf.html

mod no_subscriber;
pub mod visitor;

use std::future::Future;
use std::net::SocketAddr;

use bytes::Bytes;
use futures_channel::mpsc;
use futures_util::stream::Stream;
use futures_util::{SinkExt, StreamExt};
use no_subscriber::NoSubscriber;
use serde_json::{map::Map, Value};
use tokio::net::{lookup_host, TcpStream, ToSocketAddrs, UdpSocket};
use tokio::time;
use tokio_util::codec::{BytesCodec, FramedWrite};
use tokio_util::compat::{FuturesAsyncWriteCompatExt, TokioAsyncWriteCompatExt};
use tokio_util::udp::UdpFramed;
use tracing_core::dispatcher::SetGlobalDefaultError;
use tracing_core::{
    span::{Attributes, Id, Record},
    Event, Subscriber,
};
use tracing_futures::WithSubscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::{registry::LookupSpan, Registry};

const DEFAULT_BUFFER: usize = 512;
const DEFAULT_TIMEOUT: u32 = 10_000;
const DEFAULT_VERSION: &str = "1.1";

/// `Logger` represents a [`Layer`] responsible for sending structured logs to Graylog.
///
/// [`Layer`]: https://docs.rs/tracing-subscriber/0.2.0-alpha.2/tracing_subscriber/layer/trait.Layer.html
#[derive(Debug)]
pub struct Logger {
    base_object: Map<String, Value>,
    line_numbers: bool,
    file_names: bool,
    module_paths: bool,
    spans: bool,
    sender: mpsc::Sender<Bytes>,
}

impl Logger {
    /// Create a default [`Logger`] configuration, which can then be customized.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// The error type for [`Logger`](struct.Logger.html) building.
#[derive(Debug)]
#[non_exhaustive]
pub enum BuilderError {
    /// Could not resolve the hostname.
    HostnameResolution(std::io::Error),
    /// Could not coerce the OsString into a string.
    OsString(std::ffi::OsString),
    /// Global dispatcher failed.
    Global(SetGlobalDefaultError),

    /// DNS name error
    #[cfg(feature = "tokio-rustls")]
    Dns(tokio_rustls::webpki::InvalidDNSNameError),
}

/// A builder for [`Logger`](struct.Logger.html).
#[derive(Debug)]
pub struct Builder {
    additional_fields: Map<String, Value>,
    version: Option<String>,
    file_names: bool,
    line_numbers: bool,
    module_paths: bool,
    spans: bool,
    timeout_ms: Option<u32>,
    buffer: Option<usize>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            additional_fields: Map::with_capacity(32),
            version: None,
            file_names: true,
            line_numbers: true,
            module_paths: true,
            spans: true,
            timeout_ms: None,
            buffer: None,
        }
    }
}

type BackgroundTask = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

impl Builder {
    /// Add a persistent additional field to the GELF messages.
    pub fn additional_field<K: ToString, V: Into<Value>>(mut self, key: K, value: V) -> Self {
        let coerced_value: Value = match value.into() {
            Value::Number(n) => Value::Number(n),
            Value::String(x) => Value::String(x),
            x => Value::String(x.to_string()),
        };
        self.additional_fields
            .insert(format!("_{}", key.to_string()), coerced_value);
        self
    }

    /// Set the GELF version number. Defaults to "1.1".
    pub fn version<V: ToString>(mut self, version: V) -> Self {
        self.version = Some(version.to_string());
        self
    }

    /// Set whether line numbers should be logged. Defaults to true.
    pub fn line_numbers(mut self, value: bool) -> Self {
        self.line_numbers = value;
        self
    }

    /// Set whether file names should be logged. Defaults to true.
    pub fn file_names(mut self, value: bool) -> Self {
        self.file_names = value;
        self
    }

    /// Set whether module paths should be logged. Defaults to true.
    pub fn module_paths(mut self, value: bool) -> Self {
        self.module_paths = value;
        self
    }

    /// Set the reconnection timeout in milliseconds. Defaults to 10 seconds.
    pub fn reconnection_timeout(mut self, millis: u32) -> Self {
        self.timeout_ms = Some(millis);
        self
    }

    /// Sets the buffer length. Defaults to 512.
    pub fn buffer(mut self, length: usize) -> Self {
        self.buffer = Some(length);
        self
    }

    /// Return `Logger` and TLS connection background task.
    #[cfg(feature = "tokio-rustls")]
    pub fn connect_tls<T>(
        self,
        addr: T,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
    ) -> Result<(Logger, BackgroundTask), BuilderError>
    where
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        let dnsname = tokio_rustls::webpki::DNSNameRef::try_from_ascii_str(domain_name)
            .map_err(BuilderError::Dns)?
            .to_owned();

        self.connect_tcp_with_wrapper(addr, {
            move |s| {
                let dnsname = dnsname.clone();
                let client_config = client_config.clone();

                async move {
                    let config = tokio_rustls::TlsConnector::from(client_config);
                    config.connect(dnsname.as_ref(), s).await
                }
            }
        })
    }

    /// Return `Logger` and TCP connection background task.
    fn connect_tcp_with_wrapper<T, F, R, I>(
        self,
        addr: T,
        f: F,
    ) -> Result<(Logger, BackgroundTask), BuilderError>
    where
        F: FnMut(TcpStream) -> R + Send + Sync + Clone + 'static,
        R: Future<Output = Result<I, std::io::Error>> + Send,
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin,
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        // Persistent fields
        let mut base_object = self.additional_fields;

        // Get hostname
        let hostname = hostname::get()
            .map_err(BuilderError::HostnameResolution)?
            .into_string()
            .map_err(BuilderError::OsString)?;
        base_object.insert("host".to_string(), hostname.into());

        // Add version
        let version = self.version.unwrap_or_else(|| DEFAULT_VERSION.to_string());
        base_object.insert("version".to_string(), version.into());

        // Get timeout
        let timeout_ms = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT);
        let buffer = self.buffer.unwrap_or(DEFAULT_BUFFER);

        // Construct background task
        let (sender, receiver) = mpsc::channel::<Bytes>(buffer);
        let mut ok_receiver = receiver.map(Ok);

        let bg_task = Box::pin(
            async move {
                // Reconnection loop
                loop {
                    // Do a DNS lookup if `addr` is a hostname
                    let addrs = lookup_host(&addr).await.into_iter().flatten();

                    // Loop through the IP addresses that the hostname resolved to
                    for addr in addrs {
                        if let Err(_err) =
                            handle_tcp_connection(addr, f.clone(), &mut ok_receiver).await
                        {
                            // TODO: Add handler
                        }
                    }

                    // Sleep before re-attempting
                    time::sleep(time::Duration::from_millis(timeout_ms as u64)).await;
                }
            }
            .with_subscriber(NoSubscriber),
        );

        let logger = Logger {
            base_object,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            sender,
        };

        Ok((logger, bg_task))
    }

    /// Return `Logger` and TCP connection background task.
    fn connect_tcp<T>(self, addr: T) -> Result<(Logger, BackgroundTask), BuilderError>
    where
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        self.connect_tcp_with_wrapper(addr, |s| async { Ok(s) })
    }

    /// Initialize logging with a given `Subscriber` and return TCP connection background task.
    pub fn init_tcp_with_subscriber<S, T>(
        self,
        addr: T,
        subscriber: S,
    ) -> Result<BackgroundTask, BuilderError>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        S: Send + Sync + 'static,
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_tcp(addr)?;

        // If a subscriber was set then use it as the inner subscriber.
        let subscriber = Layer::with_subscriber(logger, subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initialize logging with a given `Subscriber` and return TCP connection background task.
    #[cfg(feature = "tokio-rustls")]
    pub fn init_tls_with_subscriber<S>(
        self,
        addr: SocketAddr,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
        subscriber: S,
    ) -> Result<BackgroundTask, BuilderError>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        S: Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_tls(addr, domain_name, client_config)?;

        // If a subscriber was set then use it as the inner subscriber.
        let subscriber = Layer::with_subscriber(logger, subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initialize logging and return TCP connection background task.
    pub fn init_tcp<T>(self, addr: T) -> Result<BackgroundTask, BuilderError>
    where
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        self.init_tcp_with_subscriber(addr, Registry::default())
    }

    /// Initialize logging and return TCP connection background task.
    #[cfg(feature = "tokio-rustls")]
    pub fn init_tls(
        self,
        addr: SocketAddr,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
    ) -> Result<BackgroundTask, BuilderError> {
        self.init_tls_with_subscriber(addr, domain_name, client_config, Registry::default())
    }

    /// Return `Logger` layer and a UDP connection background task.
    pub fn connect_udp<T>(self, addr: T) -> Result<(Logger, BackgroundTask), BuilderError>
    where
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        // Persistent fields
        let mut base_object = self.additional_fields;

        // Get hostname
        let hostname = hostname::get()
            .map_err(BuilderError::HostnameResolution)?
            .into_string()
            .map_err(BuilderError::OsString)?;
        base_object.insert("host".to_string(), hostname.into());

        // Add version
        let version = self.version.unwrap_or_else(|| DEFAULT_VERSION.to_string());
        base_object.insert("version".to_string(), version.into());

        // Get timeout
        let timeout_ms = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT);
        let buffer = self.buffer.unwrap_or(DEFAULT_BUFFER);

        // Construct background task
        let (sender, mut receiver) = mpsc::channel::<Bytes>(buffer);

        let bg_task = Box::pin(
            async move {
                // Reconnection loop
                loop {
                    // Do a DNS lookup if `addr` is a hostname
                    let addrs = lookup_host(&addr).await.into_iter().flatten();

                    // Loop through the IP addresses that the hostname resolved to
                    for addr in addrs {
                        if let Err(_err)=handle_udp_connection(addr, &mut receiver).await {
                            // TODO: Add handler
                        }
                    }

                    // Sleep before re-attempting
                    time::sleep(time::Duration::from_millis(timeout_ms as u64)).await;
                }
            }
            .with_subscriber(NoSubscriber),
        );
        let logger = Logger {
            base_object,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            sender,
        };

        Ok((logger, bg_task))
    }

    /// Initialize logging with a given `Subscriber` and return UDP connection background task.
    pub fn init_udp_with_subscriber<S, T>(
        self,
        addr: T,
        subscriber: S,
    ) -> Result<BackgroundTask, BuilderError>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        S: Send + Sync + 'static,
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_udp(addr)?;
        let subscriber = Layer::with_subscriber(logger, subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initialize logging and return UDP connection background task.
    pub fn init_udp<T>(self, addr: T) -> Result<BackgroundTask, BuilderError>
    where
        T: ToSocketAddrs,
        T: Send + Sync + 'static,
    {
        self.init_udp_with_subscriber(addr, Registry::default())
    }
}

impl<S> Layer<S> for Logger
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");

        let mut extensions = span.extensions_mut();

        if extensions.get_mut::<Map<String, Value>>().is_none() {
            let mut object = Map::with_capacity(16);
            let mut visitor = visitor::AdditionalFieldVisitor::new(&mut object);
            attrs.record(&mut visitor);
            extensions.insert(object);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found, this is a bug");
        let mut extensions = span.extensions_mut();
        if let Some(mut object) = extensions.get_mut::<Map<String, Value>>() {
            let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(&mut object);
            values.record(&mut add_field_visitor);
        } else {
            let mut object = Map::with_capacity(16);
            let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(&mut object);
            values.record(&mut add_field_visitor);
            extensions.insert(object)
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // GELF object
        let mut object: Map<String, Value> = self.base_object.clone();

        // Get span name
        if self.spans {
            let span = ctx.scope().fold(String::new(), |mut spans, span| {
                // Add span fields to the base object
                if let Some(span_object) = span.extensions().get::<Map<String, Value>>() {
                    object.extend(span_object.clone());
                }
                if spans != String::new() {
                    spans = format!("{}:{}", spans, span.name());
                } else {
                    spans = span.name().to_string();
                }

                spans
            });

            object.insert("_span".to_string(), span.into());
        }

        // Extract metadata
        // Insert level
        let metadata = event.metadata();
        let level_num = match *metadata.level() {
            tracing_core::Level::ERROR => 3,
            tracing_core::Level::WARN => 4,
            tracing_core::Level::INFO => 5,
            tracing_core::Level::DEBUG => 6,
            tracing_core::Level::TRACE => 7,
        };
        object.insert("level".to_string(), level_num.into());

        // Insert file
        if self.file_names {
            if let Some(file) = metadata.file() {
                object.insert("_file".to_string(), file.into());
            }
        }

        // Insert line
        if self.line_numbers {
            if let Some(line) = metadata.line() {
                object.insert("_line".to_string(), line.into());
            }
        }

        // Insert module path
        if self.module_paths {
            if let Some(module_path) = metadata.module_path() {
                object.insert("_module_path".to_string(), module_path.into());
            }
        }

        // Append additional fields
        let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(&mut object);
        event.record(&mut add_field_visitor);

        if !object.contains_key("short_message") {
            object.insert("short_message".to_string(), "".into());
        }

        // Serialize
        let final_object = Value::Object(object);
        let mut raw = serde_json::to_vec(&final_object).unwrap(); // This is safe
        raw.push(0);

        // Send
        if let Err(_err) = self.sender.clone().try_send(Bytes::from(raw)) {
            // TODO: Add handler
        };
    }
}

async fn handle_tcp_connection<F, R, S, I>(
    addr: SocketAddr,
    mut f: F,
    receiver: &mut S,
) -> Result<(), std::io::Error>
where
    S: Stream<Item = Result<Bytes, std::io::Error>>,
    S: Unpin,
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin,
    F: FnMut(TcpStream) -> R + Send + Sync + Clone + 'static,
    R: Future<Output = Result<I, std::io::Error>> + Send,
{
    let tcp = TcpStream::connect(addr).await?;
    let wrapped = (f)(tcp).await?.compat_write();
    let (_, writer) = futures_util::AsyncReadExt::split(wrapped);

    // Writer
    let mut sink = FramedWrite::new(writer.compat_write(), BytesCodec::new());
    sink.send_all(receiver).await?;

    Ok(())
}

async fn handle_udp_connection<S>(addr: SocketAddr, receiver: &mut S) -> Result<(), std::io::Error>
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
    let (mut sink, _) = udp_stream.split();
    while let Some(bytes) = receiver.next().await {
        sink.send((bytes, addr)).await?;
    }

    Ok(())
}
