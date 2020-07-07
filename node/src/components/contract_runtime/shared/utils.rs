use serde::Serialize;

/// serializes value to json;
/// pretty_print: false = inline
/// pretty_print: true  = pretty printed / multiline
pub fn jsonify<T>(value: T, pretty_print: bool) -> String
where
    T: Serialize,
{
    let fj = if pretty_print {
        serde_json::to_string_pretty
    } else {
        serde_json::to_string
    };

    match fj(&value) {
        Ok(json) => json,
        Err(_) => r#"{"error": "encountered error serializing value"}"#.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Deserialize, Eq, Serialize)]
    struct SerMock {
        foo: String,
        bar: u32,
    }

    impl PartialEq for SerMock {
        fn eq(&self, other: &SerMock) -> bool {
            self.foo.eq(&other.foo) && self.bar == other.bar
        }
    }

    #[test]
    fn should_ser_to_json() {
        let sermock = SerMock {
            foo: "foo".to_string(),
            bar: 1,
        };

        let json = jsonify(sermock, false);

        assert_eq!(json, r#"{"foo":"foo","bar":1}"#, "json expected to match");
    }

    #[test]
    fn should_ser_to_pretty_json() {
        let sermock = SerMock {
            foo: "foo".to_string(),
            bar: 1,
        };

        let json = jsonify(sermock, true);

        assert!(json.contains('\n'), "json expected to be multiline");
    }

    #[test]
    fn should_deser_from_json() {
        let sermock = SerMock {
            foo: "foo".to_string(),
            bar: 1,
        };

        let json = jsonify(&sermock, false);

        let sermock_clone: SerMock = serde_json::from_str(&json).expect("should deser");

        assert!(
            sermock.eq(&sermock_clone),
            "instances should contain the same data"
        );
    }
}
