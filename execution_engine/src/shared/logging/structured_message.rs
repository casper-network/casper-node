use std::{
    collections::BTreeMap,
    env,
    fmt::{self, Display, Formatter},
    process,
    time::SystemTime,
};

use log::kv::{self, Key, Value, Visitor};
use once_cell::sync::Lazy;
use serde::{Serialize, Serializer};

use casper_types::SemVer;

use crate::shared::{
    logging::{DEFAULT_MESSAGE_TEMPLATE, MESSAGE_TEMPLATE_KEY},
    utils,
};

static PROCESS_ID: Lazy<u32> = Lazy::new(process::id);
static PROCESS_NAME: Lazy<String> = Lazy::new(|| {
    env::current_exe()
        .ok()
        .and_then(|full_path| {
            full_path
                .file_stem()
                .map(|file_stem| file_stem.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown-process".to_string())
});
static HOST_NAME: Lazy<String> = Lazy::new(|| {
    hostname::get()
        .map(|host_name| host_name.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string())
});
static MESSAGE_TYPE: Lazy<String> = Lazy::new(|| "ee-structured".to_string());
static MESSAGE_TYPE_VERSION: Lazy<MessageTypeVersion> = Lazy::new(MessageTypeVersion::default);

/// container for log message data
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StructuredMessage {
    timestamp: TimestampRfc3999,
    process_id: u32,
    process_name: String,
    host_name: String,
    log_level: String,
    priority: Priority,
    message_type: String,
    message_type_version: MessageTypeVersion,
    message_id: MessageId,
    description: String,
    properties: MessageProperties,
}

impl StructuredMessage {
    pub fn new(log_level: String, message_id: MessageId, properties: MessageProperties) -> Self {
        let timestamp = TimestampRfc3999::default();
        let process_id = *PROCESS_ID;
        let process_name = PROCESS_NAME.clone();
        let host_name = HOST_NAME.clone();
        let priority = Priority::from(log_level.as_str());
        let message_type = MESSAGE_TYPE.clone();
        let message_type_version = *MESSAGE_TYPE_VERSION;
        let description = properties.get_formatted_message();

        StructuredMessage {
            timestamp,
            process_id,
            process_name,
            host_name,
            log_level,
            priority,
            message_type,
            message_type_version,
            message_id,
            description,
            properties,
        }
    }
}

impl Display for StructuredMessage {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let json = utils::jsonify(self, false);
        write!(
            formatter,
            "{timestamp} {loglevel} {priority} {hostname} {facility} payload={payload}",
            timestamp = self.timestamp,
            loglevel = self.log_level.to_string().to_uppercase(),
            priority = self.priority,
            hostname = self.host_name,
            facility = self.process_name,
            payload = json
        )
    }
}

/// newtype to encapsulate log level priority
#[derive(Clone, Copy, Debug, Hash, Serialize)]
struct Priority(u8);

impl From<&str> for Priority {
    fn from(level: &str) -> Self {
        match level {
            "Error" => Priority(3),
            "Warn" => Priority(4),
            "Info" => Priority(5),
            "Debug" => Priority(6),
            "Metric" => Priority(6),
            "Trace" => Priority(7),
            _ => Priority(255),
        }
    }
}

impl Display for Priority {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Copy, Clone)]
struct MessageTypeVersion(SemVer);

impl Display for MessageTypeVersion {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Default for MessageTypeVersion {
    fn default() -> Self {
        MessageTypeVersion(SemVer::V1_0_0)
    }
}

impl Serialize for MessageTypeVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}.{}.{}", self.0.major, self.0.minor, self.0.patch);
        serializer.serialize_str(&s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct MessageId(usize);

impl MessageId {
    pub fn new(id: usize) -> MessageId {
        MessageId(id)
    }
}

/// newtype for Rfc3999 formatted timestamp
#[derive(Clone, Debug, Hash, Serialize)]
pub(crate) struct TimestampRfc3999(String);

impl Default for TimestampRfc3999 {
    fn default() -> Self {
        TimestampRfc3999(humantime::format_rfc3339_millis(SystemTime::now()).to_string())
    }
}

impl Display for TimestampRfc3999 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Hash, Serialize)]
pub(crate) struct MessageProperties(BTreeMap<String, String>);

impl MessageProperties {
    pub fn new(mut properties: BTreeMap<String, String>) -> MessageProperties {
        // add the default message template ("message_template", "{message}") if the template key
        // doesn't already exist.
        properties
            .entry(MESSAGE_TEMPLATE_KEY.to_string())
            .or_insert_with(|| DEFAULT_MESSAGE_TEMPLATE.to_string());
        MessageProperties(properties)
    }

    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        self.0.insert(key, value)
    }

    /// strips out brace encased motes in message_template
    /// and applies them as candidate keys for the encapsulated collection of
    /// message properties. the underlying value of any candidate key that
    /// has an entry in the collection will be spliced into the output in
    /// the place of its corresponding brace encased candidate key
    pub fn get_formatted_message(&self) -> String {
        let message_template = match self.0.get(MESSAGE_TEMPLATE_KEY) {
            Some(message_template) if !message_template.is_empty() => message_template,
            _ => return String::new(),
        };

        let mut buf = String::new();
        let mut candidate_key = String::new();

        let mut key_seek = false;
        let properties = &self.0;

        for c in message_template.chars() {
            match c {
                '{' => {
                    key_seek = true;
                    candidate_key.clear();
                }
                '}' if key_seek => {
                    key_seek = false;
                    if let Some(v) = properties.get(&candidate_key) {
                        buf.push_str(v);
                    }
                }
                '}' => (),
                c if key_seek => candidate_key.push(c),
                c => buf.push(c),
            }
        }
        buf
    }
}

impl Default for MessageProperties {
    fn default() -> Self {
        MessageProperties::new(BTreeMap::new())
    }
}

/// This impl allows us to populate a `MessageProperties` map from a log `Record::key_values()`.
impl<'kvs> Visitor<'kvs> for MessageProperties {
    fn visit_pair(&mut self, key: Key<'kvs>, value: Value<'kvs>) -> Result<(), kv::Error> {
        // The value was generated via the Debug impl, i.e. it has been wrapped in quotation marks
        // and inner chars have been escaped as required.  Undo this for passing to `jsonify`, or we
        // get double-escaped chars.
        let value = value
            .to_string()
            .trim_matches('"')
            .replace(r#"\'"#, r#"'"#)
            .replace(r#"\""#, r#"""#)
            .replace(r#"\\"#, r#"\"#);
        self.0.insert(key.to_string(), value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::logging::DEFAULT_MESSAGE_KEY;

    #[test]
    fn should_get_process_id() {
        assert!(
            *PROCESS_ID != 0,
            "PROCESS_ID should not be 0: {}",
            *PROCESS_ID
        );
    }

    #[test]
    fn should_get_process_name() {
        assert!(!PROCESS_NAME.is_empty(), "PROCESS_NAME should have chars")
    }

    #[test]
    fn should_get_host_name() {
        assert!(!HOST_NAME.is_empty(), "HOST_NAME should have chars")
    }

    #[test]
    fn should_format_message_template_default_use_case() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        properties.insert(
            DEFAULT_MESSAGE_KEY.to_string(),
            "i am a log message".to_string(),
        );

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(
            formatted,
            "i am a log message".to_string(),
            "message malformed"
        )
    }

    #[test]
    fn should_format_message_template_starting_and_ending_with_braces() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        properties.insert(
            DEFAULT_MESSAGE_KEY.to_string(),
            "i convey meaning".to_string(),
        );
        properties.insert("abc".to_string(), "some text".to_string());
        properties.insert("some-hash".to_string(), "A@#$!@#".to_string());
        properties.insert("byz".to_string(), "".to_string());
        let template =
            "{abc} i'm a message temp{byz}late some-hash:{some-hash} msg:{message}".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(
            formatted,
            "some text i\'m a message template some-hash:A@#$!@# msg:i convey meaning".to_string(),
            "message malformed"
        )
    }

    #[test]
    fn should_format_message_template_with_escaped_braces() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        properties.insert(DEFAULT_MESSAGE_KEY.to_string(), "a message".to_string());
        properties.insert("more-data".to_string(), "some additional data".to_string());
        let template = "this is {{message}} with {{{more-data}}}".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(
            formatted,
            "this is a message with some additional data".to_string(),
            "message malformed"
        )
    }

    #[test]
    fn should_format_message_template_with_no_properties() {
        let properties: BTreeMap<String, String> = BTreeMap::new();

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(formatted, "".to_string(), "message malformed")
    }

    #[test]
    fn should_format_message_template_with_unclosed_brace() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        let template = "{message".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(formatted, "".to_string(), "message malformed")
    }

    #[test]
    fn should_format_message_template_with_unopened_brace() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        let template = "message}".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(formatted, "message".to_string(), "message malformed")
    }

    #[test]
    fn should_format_message_template_with_mismatched_braces_left() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        let template = "{{message}".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(formatted, "".to_string(), "message malformed")
    }

    #[test]
    fn should_format_message_template_with_mismatched_braces_right() {
        let mut properties: BTreeMap<String, String> = BTreeMap::new();
        let template = "{message}}".to_string();
        properties.insert(MESSAGE_TEMPLATE_KEY.to_string(), template);

        let props = MessageProperties::new(properties);

        let formatted = props.get_formatted_message();

        assert_eq!(formatted, "".to_string(), "message malformed")
    }

    #[test]
    fn should_validate_log_message() {
        let test_msg = "test_message".to_string();

        let mut properties = MessageProperties::default();
        properties.insert(DEFAULT_MESSAGE_KEY.to_string(), test_msg);

        let l = StructuredMessage::new("Error".to_string(), MessageId::new(1), properties);

        assert!(
            should_have_rfc3339_timestamp(&l),
            "rfc3339 timestamp required"
        );

        assert!(should_have_log_level(&l), "log level required");
        assert!(should_have_process_id(&l), "process id required");
        assert!(should_have_process_name(&l), "process name required");
        assert!(should_have_host_name(&l), "host name required");
        assert!(should_have_at_least_one_property(&l), "properties required");
        assert!(should_have_description(&l), "description required");
    }

    fn should_have_rfc3339_timestamp(l: &StructuredMessage) -> bool {
        // ISO 8601 / RFC 3339
        // rfc3339 = "YYYY-MM-DDTHH:mm:ss+00:00"
        humantime::parse_rfc3339(&l.timestamp.0).is_ok()
    }

    fn should_have_log_level(l: &StructuredMessage) -> bool {
        !l.log_level.is_empty()
    }

    fn should_have_description(l: &StructuredMessage) -> bool {
        !l.description.is_empty()
    }

    fn should_have_process_id(l: &StructuredMessage) -> bool {
        l.process_id > 0
    }

    fn should_have_process_name(l: &StructuredMessage) -> bool {
        !l.process_name.is_empty()
    }

    fn should_have_host_name(l: &StructuredMessage) -> bool {
        !l.host_name.is_empty()
    }

    fn should_have_at_least_one_property(l: &StructuredMessage) -> bool {
        !l.properties.0.is_empty()
    }
}
