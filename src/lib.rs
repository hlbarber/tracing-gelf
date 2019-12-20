use std::collections::HashMap;
use std::future::Future;

use bytes::Bytes;
use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio_util::codec::{BytesCodec, FramedWrite};
use tracing_core::{span, Event, Metadata, Subscriber};
use tracing_serde::AsSerde;
use tracing_subscriber::Registry;

#[derive(Debug)]
pub struct TcpLogger {
    version: String,
    hostname: String,
    additional_fields: HashMap<String, Value>,
    line_numbers: bool,
    file_names: bool,
    module_paths: bool,
    spans: bool,
    registry: Registry,
    sender: mpsc::Sender<Bytes>,
}

impl TcpLogger {
    pub fn builder() -> TcpBuilder {
        TcpBuilder::default()
    }
}

#[derive(Debug)]
pub enum TcpBuilderError {
    MissingHost,
    HostnameResolution(std::io::Error),
    OsString(std::ffi::OsString),
}

#[derive(Debug)]
pub struct TcpBuilder {
    additional_fields: HashMap<String, Value>,
    version: Option<String>,
    file_names: bool,
    line_numbers: bool,
    module_paths: bool,
    spans: bool,
}

impl Default for TcpBuilder {
    fn default() -> Self {
        TcpBuilder {
            additional_fields: HashMap::with_capacity(32),
            version: None,
            file_names: true,
            line_numbers: true,
            module_paths: true,
            spans: true,
        }
    }
}

type BackgroundTask = std::pin::Pin<Box<dyn Future<Output = ()>>>;

impl TcpBuilder {
    /// Add a persistent additional field to the GELF messages.
    pub fn additional_field<K: ToString, V: Into<Value>>(&mut self, key: K, value: V) -> &mut Self {
        self.additional_fields.insert(key.to_string(), value.into());
        self
    }

    /// Set whether line numbers should be logged. Defaults to true.
    pub fn line_numbers(&mut self, value: bool) -> &mut Self {
        self.line_numbers = value;
        self
    }

    /// Sets whether file names should be logged. Defaults to true.
    pub fn file_names(&mut self, value: bool) -> &mut Self {
        self.file_names = value;
        self
    }

    /// Sets whether module paths should be logged. Defaults to true.
    pub fn module_paths(&mut self, value: bool) -> &mut Self {
        self.module_paths = value;
        self
    }

    pub fn connect<A>(
        self,
        addr: A,
        buffer: usize,
    ) -> Result<(TcpLogger, BackgroundTask), TcpBuilderError>
    where
        A: ToSocketAddrs + 'static + Clone,
    {
        // Get hostname
        let hostname = hostname::get()
            .map_err(TcpBuilderError::HostnameResolution)?
            .into_string()
            .map_err(TcpBuilderError::OsString)?;
        let version = if let Some(version) = self.version {
            version
        } else {
            "1.1".to_string()
        };

        // Construct background task
        let (sender, receiver) = mpsc::channel::<Bytes>(buffer);
        let mut ok_receiver = receiver.map(|x| Ok(x));

        let bg_task = Box::pin(async move {
            // Reconnection loop
            loop {
                // Try connect
                let mut tcp_stream = match TcpStream::connect(addr.clone()).await {
                    Ok(ok) => ok,
                    Err(_) => continue,
                };

                // Writer
                let (_, writer) = tcp_stream.split();
                let mut sink = FramedWrite::new(writer, BytesCodec::new());
                sink.send_all(&mut ok_receiver).await;
            }
        });
        let logger = TcpLogger {
            additional_fields: self.additional_fields,
            hostname,
            version,
            file_names: self.file_names,
            line_numbers: self.line_numbers,
            module_paths: self.module_paths,
            spans: self.spans,
            registry: Default::default(),
            sender,
        };

        Ok((logger, bg_task))
    }
}

impl Subscriber for TcpLogger {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        // TODO: Log level filtering or layering will replace this
        true
    }

    // I'm guessing we do nothing here because we don't want to broadcast
    // a GELF message when we enter spans.
    #[inline]
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    #[inline]
    fn new_span(&self, span: &span::Attributes<'_>) -> span::Id {
        self.registry.new_span(span)
    }

    fn event(&self, event: &Event<'_>) {
        // GELF object
        let mut object: HashMap<String, Value> = HashMap::with_capacity(32);

        // Get span name
        if self.spans {
            if let Some(metadata) = self.current_span().metadata() {
                object.insert("_span".to_string(), metadata.name().into());
            }
        }

        /////
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

        if let Ok(Value::Object(mut map)) = serde_json::to_value(event.as_serde()) {
            // Must contain short_message
            if !map.contains_key("short_message") {
                return;
            }

            for (key, value) in &self.additional_fields {
                map.insert(format!("_{}", key), value.clone());
            }
            map.insert("version".to_string(), self.version.clone().into());
        }
    }

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn enter(&self, id: &span::Id) {
        self.registry.enter(id)
    }

    fn exit(&self, id: &span::Id) {
        self.registry.exit(id)
    }

    fn clone_span(&self, id: &span::Id) -> span::Id {
        self.registry.clone_span(id)
    }

    fn current_span(&self) -> span::Current {
        self.registry.current_span()
    }

    fn try_close(&self, id: span::Id) -> bool {
        self.registry.try_close(id)
    }
}
