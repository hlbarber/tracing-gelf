#![warn(missing_debug_implementations, missing_docs, rust_2018_idioms)]

//! Provides a [`tracing`] [`Layer`] for Graylog structured logging.
//!
//! # Usage
//!
//! ```rust
//! use std::net::SocketAddr;
//! use tracing_gelf::Logger;
//!
//! #[tokio::main]
//! async fn main() {
//!    // Graylog address
//!    let address = "127.0.0.1:12201";
//!
//!    // Initialize subscriber
//!    let conn_handle = Logger::builder().init_tcp(address).unwrap();
//!
//!    // Spawn background task
//!    // Any futures executor can be used
//!    tokio::spawn(conn_handle.connect());
//!
//!    // Send a log to Graylog
//!    tracing::info!(message = "oooh, what's in here?");
//!
//!    // Create a span
//!    let span = tracing::info_span!("cave");
//!    span.in_scope(|| {
//!        let test = tracing::info_span!("deeper in cave", smell = "damp");
//!        test.in_scope(|| {
//!            // Send a log to Graylog, inside a nested span
//!            tracing::warn!(message = "oh god, it's dark in here");
//!        })
//!    });
//!
//!    // Send a log to Graylog
//!    tracing::error!(message = "i'm glad to be out", spook_lvl = 3, ruck_sack = ?["glasses", "inhaler", "large bat"]);
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

mod connection;
mod visitor;

use std::{borrow::Cow, collections::HashMap, fmt::Display};

use bytes::Bytes;
use futures_channel::mpsc;
use serde_json::{map::Map, Value};
use tokio::net::ToSocketAddrs;
use tracing_core::{
    dispatcher::SetGlobalDefaultError,
    span::{Attributes, Id, Record},
    Event, Subscriber,
};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
    Registry,
};

pub use connection::*;

const DEFAULT_BUFFER: usize = 512;
const DEFAULT_VERSION: &str = "1.1";

/// A [`Layer`] responsible for sending structured logs to Graylog.
///
/// [`Layer`]: https://docs.rs/tracing-subscriber/0.2.0-alpha.2/tracing_subscriber/layer/trait.Layer.html
#[derive(Debug)]
pub struct Logger {
    base_object: HashMap<Cow<'static, str>, Value>,
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

/// The error type for [`Logger`] building.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BuilderError {
    /// Could not resolve the hostname.
    #[error("hostname resolution failed: {0}")]
    HostnameResolution(#[source] std::io::Error),
    /// Could not coerce the OsString into a string.
    #[error("hostname could not be parsed as an OsString: {}", .0.to_string_lossy().as_ref())]
    OsString(std::ffi::OsString),
    /// Global dispatcher failed.
    #[error("global dispatcher failed to initialize")]
    Global(#[source] SetGlobalDefaultError),

    /// DNS name error.
    #[cfg(feature = "rustls-tls")]
    #[error("invalid DNS name: {0}")]
    Dns(#[source] tokio_rustls::rustls::client::InvalidDnsNameError),
}

/// A builder for [`Logger`].
#[derive(Debug)]
pub struct Builder {
    additional_fields: HashMap<Cow<'static, str>, Value>,
    version: Option<String>,
    file_names: bool,
    line_numbers: bool,
    module_paths: bool,
    spans: bool,
    buffer: Option<usize>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            additional_fields: HashMap::with_capacity(32),
            version: None,
            file_names: true,
            line_numbers: true,
            module_paths: true,
            spans: true,
            buffer: None,
        }
    }
}

impl Builder {
    /// Adds a persistent additional field to the GELF messages.
    pub fn additional_field<K: Display, V: Into<Value>>(mut self, key: K, value: V) -> Self {
        let coerced_value: Value = match value.into() {
            Value::Number(n) => Value::Number(n),
            Value::String(x) => Value::String(x),
            x => Value::String(x.to_string()),
        };
        self.additional_fields
            .insert(format!("_{}", key).into(), coerced_value);
        self
    }

    /// Sets the GELF version number. Defaults to "1.1".
    pub fn version<V: ToString>(mut self, version: V) -> Self {
        self.version = Some(version.to_string());
        self
    }

    /// Sets whether line numbers should be logged. Defaults to true.
    pub fn line_numbers(mut self, value: bool) -> Self {
        self.line_numbers = value;
        self
    }

    /// Sets whether file names should be logged. Defaults to true.
    pub fn file_names(mut self, value: bool) -> Self {
        self.file_names = value;
        self
    }

    /// Sets whether module paths should be logged. Defaults to true.
    pub fn module_paths(mut self, value: bool) -> Self {
        self.module_paths = value;
        self
    }

    /// Sets the buffer length. Defaults to 512.
    pub fn buffer(mut self, length: usize) -> Self {
        self.buffer = Some(length);
        self
    }

    fn connect<A, Conn>(
        self,
        addr: A,
        conn: Conn,
    ) -> Result<(Logger, ConnectionHandle<A, Conn>), BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        // Persistent fields
        let mut base_object = self.additional_fields;

        // Get hostname
        let hostname = hostname::get()
            .map_err(BuilderError::HostnameResolution)?
            .into_string()
            .map_err(BuilderError::OsString)?;
        base_object.insert("host".into(), hostname.into());

        // Add version
        let version = self.version.unwrap_or_else(|| DEFAULT_VERSION.to_string());
        base_object.insert("version".into(), version.into());

        // Set buffer
        let buffer = self.buffer.unwrap_or(DEFAULT_BUFFER);

        // Construct background task
        let (sender, receiver) = mpsc::channel::<Bytes>(buffer);
        let handle = ConnectionHandle {
            addr,
            receiver,
            conn,
        };
        let logger = Logger {
            base_object,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            sender,
        };

        Ok((logger, handle))
    }

    /// Returns a [`Logger`] and its UDP [`ConnectionHandle`].
    pub fn connect_udp<A>(
        self,
        addr: A,
    ) -> Result<(Logger, ConnectionHandle<A, UdpConnection>), BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        self.connect(addr, UdpConnection)
    }

    /// Returns a [`Logger`] and its TCP [`ConnectionHandle`].
    pub fn connect_tcp<A>(
        self,
        addr: A,
    ) -> Result<(Logger, ConnectionHandle<A, TcpConnection>), BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        self.connect(addr, TcpConnection)
    }

    /// Returns a [`Logger`] and a TLS [`ConnectionHandle`].
    #[cfg(feature = "rustls-tls")]
    pub fn connect_tls<A>(
        self,
        addr: A,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
    ) -> Result<(Logger, ConnectionHandle<A, TlsConnection>), BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        use std::convert::TryFrom;
        let server_name =
            tokio_rustls::rustls::ServerName::try_from(domain_name).map_err(BuilderError::Dns)?;

        self.connect(
            addr,
            TlsConnection {
                server_name,
                client_config,
            },
        )
    }

    /// Initialize logging with a given `Subscriber` and return UDP connection background task.
    pub fn init_udp_with_subscriber<S, A>(
        self,
        addr: A,
        subscriber: S,
    ) -> Result<ConnectionHandle<A, UdpConnection>, BuilderError>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
        S: Send + Sync + 'static,
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_udp(addr)?;
        let subscriber = Layer::with_subscriber(logger, subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initializes logging with a given [`Subscriber`] and returns its [`ConnectionHandle`].
    pub fn init_tcp_with_subscriber<A, S>(
        self,
        addr: A,
        subscriber: S,
    ) -> Result<ConnectionHandle<A, TcpConnection>, BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,

        S: Subscriber + for<'a> LookupSpan<'a>,
        S: Send + Sync + 'static,
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

    /// Initialize logging with a given [`Subscriber`] and returns its [`ConnectionHandle`].
    #[cfg(feature = "rustls-tls")]
    pub fn init_tls_with_subscriber<A, S>(
        self,
        addr: A,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
        subscriber: S,
    ) -> Result<ConnectionHandle<A, TlsConnection>, BuilderError>
    where
        A: ToSocketAddrs + Send + Sync + 'static,
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

    /// Initializes TCP logging and returns its [`ConnectionHandle`].
    pub fn init_tcp<A>(self, addr: A) -> Result<ConnectionHandle<A, TcpConnection>, BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        self.init_tcp_with_subscriber(addr, Registry::default())
    }

    /// Initializes TLS logging and returns its [`ConnectionHandle`].
    #[cfg(feature = "rustls-tls")]
    pub fn init_tls<A>(
        self,
        addr: A,
        domain_name: &str,
        client_config: std::sync::Arc<tokio_rustls::rustls::ClientConfig>,
    ) -> Result<ConnectionHandle<A, TlsConnection>, BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        self.init_tls_with_subscriber(addr, domain_name, client_config, Registry::default())
    }

    /// Initialize UDP logging and returns its [`ConnectionHandle`].
    pub fn init_udp<A>(self, addr: A) -> Result<ConnectionHandle<A, UdpConnection>, BuilderError>
    where
        A: ToSocketAddrs,
        A: Send + Sync + 'static,
    {
        self.init_udp_with_subscriber(addr, Registry::default())
    }
}

impl<S> Layer<S> for Logger
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("span not found, this is a bug");

        let mut extensions = span.extensions_mut();

        if extensions.get_mut::<Map<String, Value>>().is_none() {
            let mut object = HashMap::with_capacity(16);
            let mut visitor = visitor::AdditionalFieldVisitor::new(&mut object);
            attrs.record(&mut visitor);
            extensions.insert(object);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("span not found, this is a bug");
        let mut extensions = span.extensions_mut();
        if let Some(object) = extensions.get_mut::<HashMap<Cow<'static, str>, Value>>() {
            let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(object);
            values.record(&mut add_field_visitor);
        } else {
            let mut object = HashMap::with_capacity(16);
            let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(&mut object);
            values.record(&mut add_field_visitor);
            extensions.insert(object)
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // GELF object
        let mut object = self.base_object.clone();

        // Get span name
        if self.spans {
            let span = ctx.current_span().id().and_then(|id| {
                ctx.span_scope(id).map(|scope| {
                    scope.from_root().fold(String::new(), |mut spans, span| {
                        // Add span fields to the base object
                        if let Some(span_object) =
                            span.extensions().get::<HashMap<Cow<'static, str>, Value>>()
                        {
                            object.extend(span_object.clone());
                        }
                        if !spans.is_empty() {
                            spans = format!("{}:{}", spans, span.name());
                        } else {
                            spans = span.name().to_string();
                        }

                        spans
                    })
                })
            });

            if let Some(span) = span {
                object.insert("_span".into(), span.into());
            }
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
        object.insert("level".into(), level_num.into());

        // Insert file
        if self.file_names {
            if let Some(file) = metadata.file() {
                object.insert("_file".into(), file.into());
            }
        }

        // Insert line
        if self.line_numbers {
            if let Some(line) = metadata.line() {
                object.insert("_line".into(), line.into());
            }
        }

        // Insert module path
        if self.module_paths {
            if let Some(module_path) = metadata.module_path() {
                object.insert("_module_path".into(), module_path.into());
            }
        }

        // Append additional fields
        let mut add_field_visitor = visitor::AdditionalFieldVisitor::new(&mut object);
        event.record(&mut add_field_visitor);

        if !object.contains_key("short_message") {
            object.insert("short_message".into(), "".into());
        }

        // Serialize
        let object = object
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        let final_object = Value::Object(object);
        let mut raw = serde_json::to_vec(&final_object).unwrap(); // This is safe
        raw.push(0);

        // Send
        if let Err(_err) = self.sender.clone().try_send(Bytes::from(raw)) {
            // TODO: Add handler
        };
    }
}
