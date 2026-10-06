//! Small-model output parsing. Repair punctuation, never invent action arguments.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Debug, PartialEq)]
pub enum Action {
    Final(String),
    Tool { name: String, args: Value },
}

pub fn parse(text: &str) -> Result<Action, String> {
    if text.len() > 256 * 1024 {
        return Err("Action exceeds the maximum size".into());
    }
    let mut text = text.trim();
    let mut hermes = false;
    // A fence may appear inside a Hermes wrapper, or the reverse.
    for _ in 0..2 {
        if text.starts_with("<tool_call>") {
            text = text
                .strip_prefix("<tool_call>")
                .and_then(|s| s.strip_suffix("</tool_call>"))
                .ok_or("Unclosed or multiple tool_call wrappers")?
                .trim();
            hermes = true;
        } else if text.starts_with("```") {
            let (language, body) = text[3..]
                .split_once('\n')
                .ok_or("JSON fence must contain a newline")?;
            if !language.trim().is_empty() && !language.trim().eq_ignore_ascii_case("json") {
                return Err("Only a JSON code fence is accepted for actions".into());
            }
            text = body
                .strip_suffix("```")
                .ok_or("Unclosed or multiple JSON fences")?
                .trim();
        } else {
            break;
        }
    }
    let repaired = remove_trailing_commas(text);
    // serde_json::Value normally accepts duplicate keys. A strict visitor avoids
    // approving one interpretation of an ambiguous action.
    let StrictValue(value) =
        serde_json::from_str(&repaired).map_err(|err| format!("Invalid action JSON: {err}"))?;
    let object = value.as_object().ok_or("Action must be a JSON object")?;
    if object.len() == 1 && object.contains_key("final") {
        let answer = object["final"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or("'final' must be a nonempty string")?;
        return Ok(Action::Final(answer.to_owned()));
    }
    let (name_key, args_key) =
        if object.len() == 2 && object.contains_key("tool") && object.contains_key("args") {
            ("tool", "args")
        } else if hermes
            && object.len() == 2
            && object.contains_key("name")
            && object.contains_key("arguments")
        {
            ("name", "arguments")
        } else {
            return Err(
                "Use exactly {\"final\":\"answer\"} or {\"tool\":\"name\",\"args\":{...}}".into(),
            );
        };
    let name = object[name_key]
        .as_str()
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .ok_or("Tool name must be lowercase snake_case")?;
    let args = object[args_key]
        .as_object()
        .ok_or("Tool args must be an object")?;
    Ok(Action::Tool {
        name: name.to_owned(),
        args: Value::Object(args.clone()),
    })
}

fn remove_trailing_commas(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if quoted {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        }
        if byte == b',' {
            let next = bytes[index + 1..].iter().find(|b| !b.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                continue;
            }
        }
        output.push(byte);
    }
    // Removing ASCII punctuation cannot break UTF-8.
    String::from_utf8(output).expect("repair preserves UTF-8")
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(StrictValue(
                    serde_json::Number::from_f64(value)
                        .ok_or_else(|| E::custom("Non-finite number"))?
                        .into(),
                ))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(StrictValue(value)) = seq.next_element()? {
                    items.push(value);
                }
                Ok(StrictValue(Value::Array(items)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut fields = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if fields.contains_key(&key) {
                        return Err(de::Error::custom(format!("Duplicate key '{key}'")));
                    }
                    let StrictValue(value) = map.next_value()?;
                    fields.insert(key, value);
                }
                Ok(StrictValue(Value::Object(fields)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repairs_fenced_unicode_and_preserves_string_punctuation() {
        assert_eq!(
            parse("```json\n{\"tool\":\"write_file\",\"args\":{\"path\":\"猫.md\",\"content\":\"comma,} and \\\"quote\\\"\",},}\n```"),
            Ok(Action::Tool {
                name: "write_file".into(),
                args: json!({"path":"猫.md","content":"comma,} and \"quote\""})
            })
        );
    }

    #[test]
    fn accepts_hermes_without_inventing_arguments() {
        assert_eq!(
            parse("<tool_call>{\"name\":\"read_file\",\"arguments\":{\"path\":\"notes.md\"}}</tool_call>"),
            Ok(Action::Tool {
                name: "read_file".into(),
                args: json!({"path":"notes.md"})
            })
        );
        assert!(parse("{\"tool\":\"write_file\"}").is_err());
        assert!(parse("{'tool':'read_file','args':{}}").is_err());
    }

    #[test]
    fn rejects_ambiguous_shapes_duplicates_and_multiple_actions() {
        for text in [
            "{\"final\":\"okay\",\"tool\":\"read_file\",\"args\":{}}",
            "{\"tool\":\"read_file\",\"args\":{},\"approve\":true}",
            "{\"tool\":\"read_file\",\"tool\":\"write_file\",\"args\":{}}",
            "{\"tool\":\"read_file\",\"args\":{\"path\":\"one\",\"path\":\"two\"}}",
            "{\"final\":\"one\"}{\"final\":\"two\"}",
            "Prose {\"final\":\"okay\"}",
            "{\"tool\":\"read_file\",\"args\":\"notes\"}",
            "{\"final\":42}",
        ] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
        assert_eq!(
            parse("{\"final\":\"Done.\"}"),
            Ok(Action::Final("Done.".into()))
        );
    }
}
