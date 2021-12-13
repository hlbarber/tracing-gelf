//! This module contains lower-level primitives for visiting fields.

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{self, Display},
};

use serde_json::Value;
use tracing_core::field::{Field, Visit};

/// The visitor necessary to record values in GELF format.
#[derive(Debug)]
pub struct AdditionalFieldVisitor<'a> {
    object: &'a mut HashMap<Cow<'static, str>, Value>,
}

impl<'a> AdditionalFieldVisitor<'a> {
    /// Create a new [`AdditionalFieldVisitor`] from a [`Map`].
    pub fn new(object: &'a mut HashMap<Cow<'static, str>, Value>) -> Self {
        AdditionalFieldVisitor { object }
    }

    fn record_additional_value<Field, V>(&mut self, field: Field, value: V)
    where
        Field: Display,
        V: Into<Value>,
    {
        let new_key = format!("_{}", field);
        self.object.insert(new_key.into(), value.into());
    }

    fn record_value<Field, V>(&mut self, field: Field, value: V)
    where
        Field: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        self.object.insert(field.into(), value.into());
    }
}

impl<'a> Visit for AdditionalFieldVisitor<'a> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let value = format!("{:#?}", value);
        let field_name = field.name();
        match field_name {
            "version" => self.record_value(field_name, value),
            "host" => self.record_value(field_name, value),
            // "message" -> "short_message"
            "message" => self.record_value("short_message", value),
            "short_message" => self.record_value(field_name, value),
            "full_message" => self.record_value(field_name, value),
            // GELF requires level: Integer
            "level" => {
                // Ignore if can't coerce into u8
                if let Ok(ok) = value.parse::<u8>() {
                    // Must be syslog level [0, 7]
                    if ok <= 7 {
                        self.record_additional_value(field_name, ok)
                    }
                }
            }
            // GELF requires level: Integer
            "timestamp" => {
                // Ignore if can't coerce into f64
                if let Ok(ok) = value.parse::<f64>() {
                    // Mut be positive
                    if 0. <= ok {
                        self.record_value(field_name, value)
                    }
                }
            }
            _ => self.record_additional_value(field_name, value),
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let field_name = field.name();
        match field_name {
            // GELF requires version: String
            "version" => self.record_value(field_name, value.to_string()),
            // GELF requires host: String
            "host" => self.record_value(field_name, value.to_string()),
            // "message" -> "short_message"
            "message" => self.record_value("short_message", value.to_string()),
            // GELF requires short_message: String
            "short_message" => self.record_value(field_name, value.to_string()),
            // GELF requires full_message: String
            "full_message" => self.record_value(field_name, value.to_string()),
            "level" => self.record_value(field_name, value),
            "timestamp" => self.record_value(field_name, value),
            _ => self.record_additional_value(field_name, value),
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let field_name = field.name();
        match field.name() {
            // GELF requires version: String
            "version" => self.record_value(field_name, value.to_string()),
            // GELF requires host: String
            "host" => self.record_value(field_name, value.to_string()),
            // "message" -> "short_message"
            "message" => self.record_value("short_message", value.to_string()),
            // GELF requires short_message: String
            "short_message" => self.record_value(field_name, value.to_string()),
            // GELF requires full_message: String
            "full_message" => self.record_value(field_name, value.to_string()),
            "level" => self.record_value(field_name, value),
            "timestamp" => self.record_value(field_name, value),
            _ => self.record_additional_value(field_name, value),
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let field_name = field.name();
        match field_name {
            // GELF requires version: String
            "version" => self.record_value(field_name, value.to_string()),
            // GELF requires host: String
            "host" => self.record_value(field_name, value.to_string()),
            // "message" -> "short_message"
            "message" => self.record_value("short_message", value.to_string()),
            // GELF requires short_message: String
            "short_message" => self.record_value(field_name, value.to_string()),
            // GELF requires full_message: String
            "full_message" => self.record_value(field_name, value.to_string()),
            "level" => {
                // Do not coerce bool into level
            }
            "timestamp" => {
                // Do not coerce bool into timestamp
            }
            _ => {
                // Booleans are not valid under GELF
                self.record_additional_value(field_name, value.to_string())
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let field_name = field.name();
        match field_name {
            "hostname" => self.record_value(field_name, value),
            "version" => self.record_value(field_name, value),
            "host" => self.record_value(field_name, value),
            // "message" -> "short_message"
            "message" => self.record_value("short_message", value),
            "short_message" => self.record_value(field_name, value),
            "full_message" => self.record_value(field_name, value),
            // GELF requires level: Integer
            "level" => {
                // Ignore if can't coerce into u8
                if let Ok(ok) = value.parse::<u8>() {
                    // Must be syslog level [0, 7]
                    if ok <= 7 {
                        self.record_value(field_name, ok)
                    }
                }
            }
            // GELF requires level: Integer
            "timestamp" => {
                // Ignore if can't coerce into f64
                if let Ok(ok) = value.parse::<f64>() {
                    // Mut be positive
                    if 0. <= ok {
                        self.record_value(field_name, value)
                    }
                }
            }
            _ => self.record_additional_value(field_name, value),
        }
    }
}
