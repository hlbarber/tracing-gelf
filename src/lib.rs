mod visitor;

use std::future::Future;
use std::net::SocketAddr;

use bytes::Bytes;
use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use serde_json::{map::Map, Value};
use tokio::net::{TcpStream, ToSocketAddrs, UdpSocket};
use tokio::time;
use tokio_util::codec::{BytesCodec, FramedWrite};
use tokio_util::udp::UdpFramed;
use tracing_core::dispatcher::SetGlobalDefaultError;
use tracing_core::{span, Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::Registry;

const DEFAULT_BUFFER: usize = 512;
const DEFAULT_TIMEOUT: u32 = 10_000;
const DEFAULT_VERSION: &str = "1.1";

#[derive(Debug)]
pub struct Logger {
    version: String,
    hostname: String,
    additional_fields: Map<String, Value>,
    line_numbers: bool,
    file_names: bool,
    module_paths: bool,
    spans: bool,
    sender: mpsc::Sender<Bytes>,
}

impl Logger {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// The error type for `Logger` building.
#[derive(Debug)]
pub enum BuilderError {
    /// Could not resolve the hostname.
    HostnameResolution(std::io::Error),
    /// Could not coerce the OsString into a string.
    OsString(std::ffi::OsString),
    /// Global dispatcher failed.
    Global(SetGlobalDefaultError),
}

/// A builder for `Logger`.
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
    pub fn additional_field<K: ToString, V: Into<Value>>(&mut self, key: K, value: V) -> &mut Self {
        self.additional_fields.insert(key.to_string(), value.into());
        self
    }

    /// Set the GELF version number. Defaults to "1.1".
    pub fn version<V: ToString>(&mut self, version: V) {
        self.version = Some(version.to_string());
    }

    /// Set whether line numbers should be logged. Defaults to true.
    pub fn line_numbers(&mut self, value: bool) -> &mut Self {
        self.line_numbers = value;
        self
    }

    /// Set whether file names should be logged. Defaults to true.
    pub fn file_names(&mut self, value: bool) -> &mut Self {
        self.file_names = value;
        self
    }

    /// Set whether module paths should be logged. Defaults to true.
    pub fn module_paths(&mut self, value: bool) -> &mut Self {
        self.module_paths = value;
        self
    }

    /// Set the reconnection timeout in milliseconds. Defaults to 10 seconds.
    pub fn reconnection_timeout(&mut self, millis: u32) -> &mut Self {
        self.timeout_ms = Some(millis);
        self
    }

    /// Sets the buffer length. Defaults to 512.
    pub fn buffer(&mut self, length: usize) -> &mut Self {
        self.buffer = Some(length);
        self
    }

    /// Return `Logger` and TCP connection background task.
    pub fn connect_tcp<A>(self, addr: A) -> Result<(Logger, BackgroundTask), BuilderError>
    where
        A: ToSocketAddrs,
        A: 'static + Clone + Send + Sync,
    {
        // Get hostname
        let hostname = hostname::get()
            .map_err(BuilderError::HostnameResolution)?
            .into_string()
            .map_err(BuilderError::OsString)?;

        // Get timeout
        let timeout_ms = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT);
        let buffer = self.buffer.unwrap_or(DEFAULT_BUFFER);
        let version = self.version.unwrap_or(DEFAULT_VERSION.to_string());

        // Construct background task
        let (sender, receiver) = mpsc::channel::<Bytes>(buffer);
        let mut ok_receiver = receiver.map(Ok);

        let bg_task = Box::pin(async move {
            // Reconnection loop
            loop {
                // Try connect
                let mut tcp_stream = match TcpStream::connect(addr.clone()).await {
                    Ok(ok) => ok,
                    Err(_) => {
                        time::delay_for(time::Duration::from_millis(timeout_ms as u64)).await;
                        continue;
                    }
                };

                // Writer
                let (_, writer) = tcp_stream.split();
                let mut sink = FramedWrite::new(writer, BytesCodec::new());
                sink.send_all(&mut ok_receiver).await;
            }
        });
        let logger = Logger {
            additional_fields: self.additional_fields,
            hostname,
            version,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            sender,
        };

        Ok((logger, bg_task))
    }

    /// Initialize logging with a given `Subscriber` and return TCP connection background task.
    pub fn init_tcp_with_subscriber<A, S>(
        self,
        addr: A,
        subscriber: S,
    ) -> Result<BackgroundTask, BuilderError>
    where
        A: ToSocketAddrs,
        A: 'static + Clone + Send + Sync,
        S: Subscriber + Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_tcp(addr)?;

        // If a subscriber was set then use it as the inner subscriber.
        let subscriber = logger.with_subscriber(subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initialize logging and return TCP connection background task.
    pub fn init_tcp<A>(self, addr: A) -> Result<BackgroundTask, BuilderError>
    where
        A: ToSocketAddrs,
        A: 'static + Clone + Send + Sync,
    {
        self.init_tcp_with_subscriber(addr, Registry::default())
    }

    /// Return `Logger` layer and a UDP connection background task.
    pub fn connect_udp(self, addr: SocketAddr) -> Result<(Logger, BackgroundTask), BuilderError> {
        // Get hostname
        let hostname = hostname::get()
            .map_err(BuilderError::HostnameResolution)?
            .into_string()
            .map_err(BuilderError::OsString)?;

        // Get timeout
        let timeout_ms = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT);
        let buffer = self.buffer.unwrap_or(DEFAULT_BUFFER);
        let version = self.version.unwrap_or(DEFAULT_VERSION.to_string());

        // Construct background task
        let (sender, receiver) = mpsc::channel::<Bytes>(buffer);
        let mut ok_receiver = receiver.map(move |bytes| Ok((bytes, addr)));

        // Bind address version must match address version
        let bind_addr = if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };

        let bg_task = Box::pin(async move {
            // Reconnection loop
            loop {
                // Try connect
                let udp_socket = match UdpSocket::bind(bind_addr).await {
                    Ok(ok) => ok,
                    Err(_) => {
                        time::delay_for(time::Duration::from_millis(timeout_ms as u64)).await;
                        continue;
                    }
                };

                // Writer
                let udp_stream = UdpFramed::new(udp_socket, BytesCodec::new());
                let (mut sink, _) = udp_stream.split();
                sink.send_all(&mut ok_receiver).await;
            }
        });
        let logger = Logger {
            additional_fields: self.additional_fields,
            hostname,
            version,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            sender,
        };

        Ok((logger, bg_task))
    }

    /// Initialize logging with a given `Subscriber` and return UDP connection background task.
    pub fn init_udp_with_subscriber<S>(
        self,
        addr: SocketAddr,
        subscriber: S,
    ) -> Result<BackgroundTask, BuilderError>
    where
        S: Subscriber + Send + Sync + 'static,
    {
        let (logger, bg_task) = self.connect_udp(addr)?;
        let subscriber = logger.with_subscriber(subscriber);
        tracing_core::dispatcher::set_global_default(tracing_core::dispatcher::Dispatch::new(
            subscriber,
        ))
        .map_err(BuilderError::Global)?;

        Ok(bg_task)
    }

    /// Initialize logging and return UDP connection background task.
    pub fn init_udp<A>(self, addr: SocketAddr) -> Result<BackgroundTask, BuilderError> {
        self.init_udp_with_subscriber(addr, Registry::default())
    }
}

impl<S: Subscriber> Layer<S> for Logger {
    // I'm guessing we do nothing here because we don't want to broadcast
    // a GELF message when we enter spans?
    #[inline]
    fn on_record(&self, _: &span::Id, _: &span::Record<'_>, ctx: Context<S>) {}

    fn on_event(&self, event: &Event<'_>, ctx: Context<S>) {
        // GELF object
        let mut object: Map<String, Value> = Map::with_capacity(32);

        // Add persistent fields
        object.insert("version".to_string(), self.version.clone().into());
        object.insert("host".to_string(), self.hostname.clone().into());

        // Get span name
        if self.spans {
            if let Some(metadata) = ctx.current_span().metadata() {
                object.insert("_span".to_string(), metadata.name().into());
            }
        }

        // Extract metadata
        // Insert level
        let metadata = event.metadata();
        let level_num = match *metadata.level() {
            tracing_core::Level::ERROR => 3,
            tracing_core::Level::WARN => 4,
            tracing_core::Level::INFO => 5,
            tracing_core::Level::TRACE => 6,
            tracing_core::Level::DEBUG => 7,
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

        // Serialize
        let final_object = Value::Object(object);
        let mut raw = serde_json::to_vec(&final_object).unwrap(); // This is safe
        raw.push(0);

        // Send
        self.sender.clone().try_send(Bytes::from(raw));
    }
}
