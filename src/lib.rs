use std::collections::HashMap;
use std::net::ToSocketAddrs;

use serde_json::Value;
use tracing_core::{span, Event, Metadata, Subscriber};
use tracing_serde::AsSerde;
use tracing_subscriber::{Layer as _, Registry};

#[derive(Debug)]
pub struct TcpLogger {
    version: String,
    hostname: String,
    additional_fields: HashMap<String, Value>,
    registry: Registry,
}

impl TcpLogger {
    pub fn builder<A>() -> TcpBuilder<A> {
        TcpBuilder::default()
    }
}

#[derive(Debug)]
pub enum TcpBuilderError {
    MissingHost,
    HostnameResolution(std::io::Error),
    OsString(std::ffi::OsString),
}

#[derive(Debug, Clone)]
pub struct TcpBuilder<A> {
    version: Option<String>,
    address: Option<A>,
    additional_fields: HashMap<String, Value>,
}

impl<A> Default for TcpBuilder<A> {
    fn default() -> Self {
        TcpBuilder {
            version: None,
            address: None,
            additional_fields: HashMap::new(),
        }
    }
}

impl<A: ToSocketAddrs> TcpBuilder<A> {
    pub fn address(&mut self, addr: A) {
        self.address = Some(addr)
    }

    pub fn additional_field<K: ToString, V: Into<Value>>(&mut self, key: K, value: V) {
        self.additional_fields.insert(key.to_string(), value.into());
    }

    pub fn build(self) -> Result<TcpLogger, TcpBuilderError> {
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

        Ok(TcpLogger {
            version,
            hostname,
            additional_fields: self.additional_fields,
            registry: Default::default(),
        })
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
        let value: Result<Value, _> = serde_json::to_value(event.as_serde());
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
