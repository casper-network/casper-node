extern crate alloc;

use alloc::{format, string::String};

use serde::Serialize;
use serde_json::{json, Value};

const MAX_STRING_LEN: usize = 150;

/// Serialize the given data structure as a pretty-printed String of JSON using
/// `serde_json::to_string_pretty()`, but after first reducing any string values over
/// `MAX_STRING_LEN` to display the field's number of chars instead of the actual value.
pub fn json_pretty_print<T>(value: &T) -> serde_json::Result<String>
where
    T: ?Sized + Serialize,
{
    let mut json_value = json!(value);
    shorten_string_field(&mut json_value);

    serde_json::to_string_pretty(&json_value)
}

fn shorten_string_field(value: &mut Value) {
    match value {
        Value::String(string) => {
            if string.len() > MAX_STRING_LEN {
                *string = format!("{} chars", string.len());
            }
        }
        Value::Array(values) => {
            for value in values {
                shorten_string_field(value);
            }
        }
        Value::Object(map) => {
            for map_value in map.values_mut() {
                shorten_string_field(map_value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use core::iter::{self, FromIterator};

    use super::*;

    #[test]
    fn should_shorten_long_strings() {
        let mut long_strings = vec![];
        for i in 1..=5 {
            long_strings.push(String::from_iter(
                iter::repeat('a').take(MAX_STRING_LEN + i),
            ));
        }
        let value = json!({
            "field_1": Option::<usize>::None,
            "field_2": true,
            "field_3": 123,
            "field_4": long_strings[0],
            "field_5": [
                "short string value",
                long_strings[1]
            ],
            "field_6": {
                "f1": Option::<usize>::None,
                "f2": false,
                "f3": -123,
                "f4": long_strings[2],
                "f5": [
                    "short string value",
                    long_strings[3]
                ],
                "f6": {
                    "final long string": long_strings[4]
                }
            }
        });

        let expected = r#"{
  "field_1": null,
  "field_2": true,
  "field_3": 123,
  "field_4": "101 chars",
  "field_5": [
    "short string value",
    "102 chars"
  ],
  "field_6": {
    "f1": null,
    "f2": false,
    "f3": -123,
    "f4": "103 chars",
    "f5": [
      "short string value",
      "104 chars"
    ],
    "f6": {
      "final long string": "105 chars"
    }
  }
}"#;

        let output = json_pretty_print(&value).unwrap();
        assert_eq!(output, expected);
    }

    #[test]
    fn should_not_modify_short_strings() {
        let max_string = String::from_iter(iter::repeat('a').take(MAX_STRING_LEN));
        let value = json!({
            "field_1": Option::<usize>::None,
            "field_2": true,
            "field_3": 123,
            "field_4": max_string,
            "field_5": [
                "short string value",
                "another short string"
            ],
            "field_6": {
                "f1": Option::<usize>::None,
                "f2": false,
                "f3": -123,
                "f4": "short",
                "f5": [
                    "short string value",
                    "another short string"
                ],
                "f6": {
                    "final string": "the last short string"
                }
            }
        });

        let expected = serde_json::to_string_pretty(&value).unwrap();
        let output = json_pretty_print(&value).unwrap();
        assert_eq!(output, expected);
    }
}
