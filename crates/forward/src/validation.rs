//! Shared fail-closed parsers for optional JSON request controls.
//!
//! Missing and explicit null are absence. Once a non-null field is present, its JSON type and
//! numeric domain are part of the public contract and must not silently degrade to a default.

use serde_json::{Map, Value};

pub(crate) fn optional_bool(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<bool>, &'static str> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(field),
    }
}

/// Return the first non-null alias. Higher-precedence aliases are checked first; a null alias is
/// absent and therefore allows the next legacy spelling. Any present value must be a positive
/// integer representable as `u64`.
pub(crate) fn optional_positive_u64(
    object: &Map<String, Value>,
    fields: &'static [&'static str],
) -> Result<Option<u64>, &'static str> {
    for field in fields {
        match object.get(*field) {
            None | Some(Value::Null) => continue,
            Some(value) => {
                return value
                    .as_u64()
                    .filter(|value| *value > 0)
                    .map(Some)
                    .ok_or(*field)
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_bool_distinguishes_absence_from_every_wrong_json_type() {
        for value in [None, Some(Value::Null)] {
            let mut object = Map::new();
            if let Some(value) = value {
                object.insert("stream".to_string(), value);
            }
            assert_eq!(optional_bool(&object, "stream"), Ok(None));
        }
        for value in [json!("false"), json!(0), json!([]), json!({})] {
            let object = Map::from_iter([("stream".to_string(), value)]);
            assert_eq!(optional_bool(&object, "stream"), Err("stream"));
        }
        for value in [false, true] {
            let object = Map::from_iter([("stream".to_string(), Value::Bool(value))]);
            assert_eq!(optional_bool(&object, "stream"), Ok(Some(value)));
        }
    }

    #[test]
    fn positive_integer_matrix_rejects_zero_fraction_strings_objects_and_overflow() {
        for raw in [
            "0",
            "-1",
            "1.5",
            "\"1\"",
            "{}",
            "[]",
            "18446744073709551616",
        ] {
            let value: Value = serde_json::from_str(raw).unwrap();
            let object = Map::from_iter([("max_output_tokens".to_string(), value)]);
            assert_eq!(
                optional_positive_u64(&object, &["max_output_tokens"]),
                Err("max_output_tokens"),
                "{raw}"
            );
        }
        for value in [None, Some(Value::Null)] {
            let mut object = Map::new();
            if let Some(value) = value {
                object.insert("max_output_tokens".to_string(), value);
            }
            assert_eq!(
                optional_positive_u64(&object, &["max_output_tokens"]),
                Ok(None)
            );
        }
        let object = Map::from_iter([("max_output_tokens".to_string(), json!(1))]);
        assert_eq!(
            optional_positive_u64(&object, &["max_output_tokens"]),
            Ok(Some(1))
        );
    }

    #[test]
    fn null_preferred_alias_falls_back_but_invalid_preferred_alias_is_terminal() {
        let object = Map::from_iter([
            ("max_completion_tokens".to_string(), Value::Null),
            ("max_tokens".to_string(), json!(7)),
        ]);
        assert_eq!(
            optional_positive_u64(&object, &["max_completion_tokens", "max_tokens"]),
            Ok(Some(7))
        );

        let object = Map::from_iter([
            ("max_completion_tokens".to_string(), json!(0)),
            ("max_tokens".to_string(), json!(7)),
        ]);
        assert_eq!(
            optional_positive_u64(&object, &["max_completion_tokens", "max_tokens"]),
            Err("max_completion_tokens")
        );
    }
}
