//! Canonical capability types.
//!
//! A capability signature is two of these values, one for the request and one
//! for the response. The host validates both directions, so a provider cannot
//! return a shape its consumer did not approve.
//!
//! The format is deliberately small. A schema is data from a third party, so
//! it has depth, count and size bounds, and it is canonicalised before hashing:
//! two manifests that describe the same shape get the same hash regardless of
//! map order.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Deepest nesting a schema may use.
pub const MAX_DEPTH: usize = 16;

/// Most fields one record may declare.
pub const MAX_FIELDS: usize = 64;

/// Most variants one union may declare.
pub const MAX_VARIANTS: usize = 32;

/// Most values one enum may list.
pub const MAX_ENUM_VALUES: usize = 64;

/// Longest schema, measured on its canonical JSON.
pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;

/// Largest array or map a value may carry in one call.
pub const MAX_ITEMS: usize = 65_536;

/// Largest byte string a value may carry.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The type of one request or response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Type {
    Bool,
    Int {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
    },
    Float,
    String {
        #[serde(default, skip_serializing_if = "StringFormat::is_plain")]
        format: StringFormat,
    },
    Bytes,
    Timestamp,
    Json,
    Record {
        fields: BTreeMap<String, Field>,
    },
    Array {
        items: Box<Type>,
    },
    Map {
        values: Box<Type>,
    },
    Union {
        variants: BTreeMap<String, Type>,
    },
    Nullable {
        value: Box<Type>,
    },
}

impl Type {
    pub fn int(min: Option<i64>, max: Option<i64>) -> Self {
        Self::Int { min, max }
    }

    pub fn string() -> Self {
        Self::String {
            format: StringFormat::Plain,
        }
    }

    pub fn path(scope: PathScope) -> Self {
        Self::String {
            format: StringFormat::Path { scope },
        }
    }

    pub fn id(domain: impl Into<String>) -> Self {
        Self::String {
            format: StringFormat::Id {
                domain: domain.into(),
            },
        }
    }

    pub fn enumeration(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::String {
            format: StringFormat::Enum {
                values: values.into_iter().map(Into::into).collect(),
            },
        }
    }

    pub fn record(fields: impl IntoIterator<Item = (&'static str, Type)>) -> Self {
        Self::Record {
            fields: fields
                .into_iter()
                .map(|(name, schema)| (name.to_string(), Field::required(schema)))
                .collect(),
        }
    }

    pub fn array(items: Type) -> Self {
        Self::Array {
            items: Box::new(items),
        }
    }

    pub fn nullable(value: Type) -> Self {
        Self::Nullable {
            value: Box::new(value),
        }
    }

    /// Validates the schema itself, not a value.
    pub fn validate(&self) -> Result<(), String> {
        self.check(0)?;
        if self.canonical_json()?.len() > MAX_SCHEMA_BYTES {
            return Err(format!("a schema is at most {MAX_SCHEMA_BYTES} bytes"));
        }
        Ok(())
    }

    fn check(&self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(format!("a schema nests at most {MAX_DEPTH} levels"));
        }
        match self {
            Self::Int { min, max } => {
                if let (Some(min), Some(max)) = (min, max)
                    && min > max
                {
                    return Err("an int range has min greater than max".to_string());
                }
            }
            Self::String { format } => match format {
                StringFormat::Enum { values } => {
                    if values.is_empty() || values.len() > MAX_ENUM_VALUES {
                        return Err(format!("an enum lists 1 to {MAX_ENUM_VALUES} values"));
                    }
                    let unique: BTreeSet<_> = values.iter().collect();
                    if unique.len() != values.len() {
                        return Err("an enum repeats a value".to_string());
                    }
                    if values.iter().any(|value| value.len() > 128) {
                        return Err("an enum value is at most 128 bytes".to_string());
                    }
                }
                StringFormat::Id { domain } if domain.is_empty() || domain.len() > 64 => {
                    return Err("an id domain is 1 to 64 bytes".to_string());
                }
                _ => {}
            },
            Self::Record { fields } => {
                if fields.len() > MAX_FIELDS {
                    return Err(format!("a record declares at most {MAX_FIELDS} fields"));
                }
                for (name, field) in fields {
                    if name.is_empty() || name.len() > 64 {
                        return Err("a field name is 1 to 64 bytes".to_string());
                    }
                    if name.chars().any(char::is_control) {
                        return Err("a field name has a control character".to_string());
                    }
                    field.schema.check(depth + 1)?;
                }
            }
            Self::Array { items } => items.check(depth + 1)?,
            Self::Map { values } => values.check(depth + 1)?,
            Self::Union { variants } => {
                if variants.is_empty() || variants.len() > MAX_VARIANTS {
                    return Err(format!("a union declares 1 to {MAX_VARIANTS} variants"));
                }
                for (tag, variant) in variants {
                    if tag.is_empty() || tag.len() > 64 {
                        return Err("a union tag is 1 to 64 bytes".to_string());
                    }
                    variant.check(depth + 1)?;
                }
            }
            Self::Nullable { value } => value.check(depth + 1)?,
            _ => {}
        }
        Ok(())
    }

    /// Canonical JSON: compact, sorted maps, stable optionality.
    pub fn canonical_json(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|error| format!("encoding the schema: {error}"))
    }

    /// SHA-256 of the canonical schema.
    pub fn hash(&self) -> Result<String, String> {
        Ok(hex(&Sha256::digest(self.canonical_json()?)))
    }

    /// Validates one decoded JSON value against this schema.
    pub fn validate_value(&self, value: &serde_json::Value) -> Result<(), String> {
        self.check_value(value, 0)
    }

    fn check_value(&self, value: &serde_json::Value, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("a value nests too deeply".to_string());
        }
        match self {
            Self::Bool => value
                .is_boolean()
                .then_some(())
                .ok_or_else(|| "expected a boolean".to_string()),
            Self::Int { min, max } => {
                let number = value
                    .as_i64()
                    .ok_or_else(|| "expected an integer".to_string())?;
                if min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max) {
                    return Err("the integer is outside its range".to_string());
                }
                Ok(())
            }
            Self::Float => value
                .as_f64()
                .map(|_| ())
                .ok_or_else(|| "expected a number".to_string()),
            Self::String { format } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| "expected a string".to_string())?;
                check_string(format, text)
            }
            Self::Bytes => {
                let text = value
                    .as_str()
                    .ok_or_else(|| "expected a base64 string".to_string())?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|_| "the byte string is not valid base64".to_string())?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err(format!("a byte string is at most {MAX_BYTES} bytes"));
                }
                Ok(())
            }
            Self::Timestamp => value
                .as_u64()
                .map(|_| ())
                .ok_or_else(|| "expected a Unix-millisecond timestamp".to_string()),
            Self::Json => Ok(()),
            Self::Record { fields } => {
                let object = value
                    .as_object()
                    .ok_or_else(|| "expected an object".to_string())?;
                for key in object.keys() {
                    if !fields.contains_key(key) {
                        return Err(format!("the object carries an unknown field `{key}`"));
                    }
                }
                for (name, field) in fields {
                    match object.get(name) {
                        Some(value) => field.schema.check_value(value, depth + 1)?,
                        None if field.optional => {}
                        None => return Err(format!("the object is missing `{name}`")),
                    }
                }
                Ok(())
            }
            Self::Array { items } => {
                let array = value
                    .as_array()
                    .ok_or_else(|| "expected an array".to_string())?;
                if array.len() > MAX_ITEMS {
                    return Err(format!("an array holds at most {MAX_ITEMS} items"));
                }
                for item in array {
                    items.check_value(item, depth + 1)?;
                }
                Ok(())
            }
            Self::Map { values } => {
                let object = value
                    .as_object()
                    .ok_or_else(|| "expected an object".to_string())?;
                if object.len() > MAX_ITEMS {
                    return Err(format!("a map holds at most {MAX_ITEMS} items"));
                }
                for item in object.values() {
                    values.check_value(item, depth + 1)?;
                }
                Ok(())
            }
            Self::Union { variants } => {
                let object = value
                    .as_object()
                    .ok_or_else(|| "expected a tagged object".to_string())?;
                let tag = object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "a union value needs a string `type`".to_string())?;
                let variant = variants
                    .get(tag)
                    .ok_or_else(|| format!("no union variant `{tag}`"))?;
                // The tag selects the variant; it is not repeated inside the
                // variant record, which would force every schema to list it.
                let mut payload = object.clone();
                payload.remove("type");
                variant.check_value(&serde_json::Value::Object(payload), depth + 1)
            }
            Self::Nullable { value: inner } => {
                if value.is_null() {
                    Ok(())
                } else {
                    inner.check_value(value, depth + 1)
                }
            }
        }
    }
}

/// One field of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    #[serde(flatten)]
    pub schema: Type,
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
}

impl Field {
    pub fn required(schema: Type) -> Self {
        Self {
            schema,
            optional: false,
        }
    }

    pub fn optional(schema: Type) -> Self {
        Self {
            schema,
            optional: true,
        }
    }
}

/// Format carried by a string value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StringFormat {
    #[default]
    Plain,
    Enum {
        values: Vec<String>,
    },
    Uuid,
    Path {
        scope: PathScope,
    },
    Id {
        domain: String,
    },
}

impl StringFormat {
    pub fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }
}

/// Where a path value is rooted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathScope {
    /// Relative to the app's own area.
    App,
    /// A host path whose access the grant must declare.
    Host,
}

fn check_string(format: &StringFormat, text: &str) -> Result<(), String> {
    if text.len() > 4096 {
        return Err("a string is at most 4096 bytes".to_string());
    }
    if text.chars().any(char::is_control) {
        return Err("a string has a control character".to_string());
    }
    match format {
        StringFormat::Plain => Ok(()),
        StringFormat::Enum { values } => {
            if values.iter().any(|value| value == text) {
                Ok(())
            } else {
                Err(format!("`{text}` is not one of the enum values"))
            }
        }
        StringFormat::Uuid => check_uuid(text),
        StringFormat::Path { .. } => Ok(()),
        StringFormat::Id { .. } => {
            if text.is_empty() {
                Err("an id is not empty".to_string())
            } else {
                Ok(())
            }
        }
    }
}

fn check_uuid(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return Err("a uuid is 36 characters".to_string());
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if hyphen {
            if *byte != b'-' {
                return Err("a uuid has hyphens at 8, 13, 18 and 23".to_string());
            }
        } else if !byte.is_ascii_hexdigit() {
            return Err("a uuid is hexadecimal".to_string());
        }
    }
    Ok(())
}

/// One exact capability signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub id: String,
    pub version: u32,
    pub input: Type,
    pub output: Type,
}

impl Signature {
    pub fn input_hash(&self) -> Result<String, String> {
        self.input.hash()
    }

    pub fn output_hash(&self) -> Result<String, String> {
        self.output.hash()
    }

    /// Hash of the whole contract. A caller and a provider can supply the same
    /// `id` and still differ here, which is what makes the match exact.
    pub fn hash(&self) -> Result<String, String> {
        let message = format!(
            "{}@{}:{}:{}",
            self.id,
            self.version,
            self.input_hash()?,
            self.output_hash()?
        );
        Ok(hex(&Sha256::digest(message.as_bytes())))
    }

    pub fn matches(&self, other: &Self) -> Result<bool, String> {
        if self.id != other.id || self.version != other.version {
            return Ok(false);
        }
        Ok(self.hash()? == other.hash()?)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.input.validate()?;
        self.output.validate()?;
        Ok(())
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identifier() -> Type {
        Type::record([("id", Type::id("process"))])
    }

    #[test]
    fn field_order_does_not_change_the_hash() {
        let first = Type::record([("a", Type::string()), ("b", Type::int(None, None))]);
        let second = Type::record([("b", Type::int(None, None)), ("a", Type::string())]);
        assert_eq!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn a_different_shape_changes_the_hash() {
        let first = Type::record([("name", Type::string())]);
        let second = Type::record([("name", Type::int(None, None))]);
        assert_ne!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn exact_signatures_match_and_versions_do_not() {
        let input = Type::record([("name", Type::string())]);
        let output = Type::record([("result", Type::int(Some(0), Some(100)))]);
        let first = Signature {
            id: "org.example.program.action".to_string(),
            version: 1,
            input: input.clone(),
            output: output.clone(),
        };
        let same = Signature {
            id: "org.example.program.action".to_string(),
            version: 1,
            input,
            output,
        };
        assert!(first.matches(&same).unwrap());

        let next = Signature {
            id: "org.example.program.action".to_string(),
            version: 2,
            input: first.input.clone(),
            output: first.output.clone(),
        };
        assert!(!first.matches(&next).unwrap());
    }

    #[test]
    fn records_reject_unknown_fields_and_validate_optionals() {
        let schema = Type::Record {
            fields: BTreeMap::from([
                ("id".to_string(), Field::required(Type::int(Some(1), None))),
                ("label".to_string(), Field::optional(Type::string())),
            ]),
        };
        assert!(
            schema
                .validate_value(&serde_json::json!({ "id": 7 }))
                .is_ok()
        );
        assert!(
            schema
                .validate_value(&serde_json::json!({ "id": 7, "label": "x" }))
                .is_ok()
        );
        assert!(
            schema
                .validate_value(&serde_json::json!({ "id": 7, "extra": true }))
                .is_err()
        );
        assert!(
            schema
                .validate_value(&serde_json::json!({ "id": 0 }))
                .is_err()
        );
    }

    #[test]
    fn unions_are_discriminated_by_type() {
        let schema = Type::Union {
            variants: BTreeMap::from([
                (
                    "start".to_string(),
                    Type::record([("id", Type::id("container"))]),
                ),
                (
                    "stop".to_string(),
                    Type::record([("id", Type::id("container"))]),
                ),
            ]),
        };
        assert!(
            schema
                .validate_value(&serde_json::json!({ "type": "start", "id": "abc" }))
                .is_ok()
        );
        // The variant record is closed, so `type` itself is an unknown field.
        assert!(
            schema
                .validate_value(&serde_json::json!({ "type": "other", "id": "abc" }))
                .is_err()
        );
    }

    #[test]
    fn schemas_are_bounded() {
        let mut nested = Type::Bool;
        for _ in 0..=MAX_DEPTH {
            nested = Type::Array {
                items: Box::new(nested),
            };
        }
        assert!(nested.validate().is_err());

        let fields = (0..=MAX_FIELDS)
            .map(|index| (format!("f{index}"), Field::required(Type::Bool)))
            .collect();
        assert!(Type::Record { fields }.validate().is_err());
    }

    #[test]
    fn base64_and_uuid_values_are_checked() {
        assert!(
            Type::Bytes
                .validate_value(&serde_json::json!("aGVsbG8="))
                .is_ok()
        );
        assert!(
            Type::Bytes
                .validate_value(&serde_json::json!("no!"))
                .is_err()
        );
        assert!(
            Type::String {
                format: StringFormat::Uuid
            }
            .validate_value(&serde_json::json!("123e4567-e89b-12d3-a456-426614174000"))
            .is_ok()
        );
    }

    #[test]
    fn the_same_shape_in_another_namespace_has_a_different_signature() {
        let input = Type::record([("name", Type::string())]);
        let output = Type::record([("result", Type::int(None, None))]);
        let first = Signature {
            id: "org.anth.host.network.read".to_string(),
            version: 1,
            input: input.clone(),
            output: output.clone(),
        };
        let second = Signature {
            id: "org.example.dashboard.network.read".to_string(),
            version: 1,
            input,
            output,
        };
        assert_eq!(first.input_hash().unwrap(), second.input_hash().unwrap());
        assert_eq!(first.output_hash().unwrap(), second.output_hash().unwrap());
        assert_ne!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn signatures_round_trip_through_json() {
        let signature = Signature {
            id: "org.example.program.action".to_string(),
            version: 1,
            input: identifier(),
            output: Type::record([("result", Type::int(None, None))]),
        };
        assert!(signature.validate().is_ok());
        let text = serde_json::to_string(&signature).unwrap();
        let parsed: Signature = serde_json::from_str(&text).unwrap();
        assert_eq!(signature, parsed);
    }
}
