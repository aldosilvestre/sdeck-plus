//! Plugin manifest model and validation. Parsing never touches the plugin executable.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

use super::PluginError;

/// Settings payload of one binding: field id -> JSON value.
pub type Settings = Map<String, Value>;

/// Wire protocol version spoken by this host.
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest accepted `manifest.json`.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Largest serialized settings payload, matching the stored-action bound.
pub const MAX_SETTINGS_BYTES: usize = 4096;
const MAX_ACTIONS: usize = 32;
const MAX_FIELDS: usize = 16;
const MAX_TEXT_LENGTH: usize = 1024;
const DEFAULT_TEXT_LENGTH: usize = 256;

/// Physical control kinds an action can be bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginControl {
    Key,
    DialTurn,
    DialPress,
    Touch,
}

/// One configuration form field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SettingField {
    Text {
        id: String,
        label: String,
        #[serde(default)]
        required: bool,
        #[serde(default = "default_text_length")]
        max_length: usize,
        #[serde(default)]
        default: Option<String>,
    },
    Integer {
        id: String,
        label: String,
        #[serde(default)]
        required: bool,
        min: i64,
        max: i64,
        #[serde(default)]
        default: Option<i64>,
    },
    Boolean {
        id: String,
        label: String,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        default: Option<bool>,
    },
}

fn default_text_length() -> usize {
    DEFAULT_TEXT_LENGTH
}

impl SettingField {
    /// Key of the field inside the settings payload.
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } | Self::Integer { id, .. } | Self::Boolean { id, .. } => id,
        }
    }

    /// Human readable label for forms.
    pub fn label(&self) -> &str {
        match self {
            Self::Text { label, .. }
            | Self::Integer { label, .. }
            | Self::Boolean { label, .. } => label,
        }
    }

    fn required(&self) -> bool {
        match self {
            Self::Text { required, .. }
            | Self::Integer { required, .. }
            | Self::Boolean { required, .. } => *required,
        }
    }

    fn default_value(&self) -> Option<Value> {
        match self {
            Self::Text { default, .. } => default.clone().map(Value::String),
            Self::Integer { default, .. } => default.map(Value::from),
            Self::Boolean { default, .. } => default.map(Value::Bool),
        }
    }

    fn validate_definition(&self) -> Result<(), String> {
        valid_id(self.id())?;
        valid_label(self.label())?;
        match self {
            Self::Text {
                max_length,
                default,
                ..
            } => {
                if *max_length == 0 || *max_length > MAX_TEXT_LENGTH {
                    return Err(format!(
                        "field `{}` max_length must be 1..={MAX_TEXT_LENGTH}",
                        self.id()
                    ));
                }
                if let Some(value) = default {
                    self.check_value(&Value::String(value.clone()))?;
                }
            }
            Self::Integer {
                min, max, default, ..
            } => {
                if min > max {
                    return Err(format!("field `{}` has min greater than max", self.id()));
                }
                if let Some(value) = default {
                    self.check_value(&Value::from(*value))?;
                }
            }
            Self::Boolean { .. } => {}
        }
        Ok(())
    }

    fn check_value(&self, value: &Value) -> Result<(), String> {
        let id = self.id();
        match (self, value) {
            (
                Self::Text {
                    required,
                    max_length,
                    ..
                },
                Value::String(text),
            ) => {
                if text.chars().count() > *max_length {
                    return Err(format!("field `{id}` exceeds {max_length} characters"));
                }
                if text.chars().any(char::is_control) {
                    return Err(format!("field `{id}` contains control characters"));
                }
                if *required && text.trim().is_empty() {
                    return Err(format!("field `{id}` is required"));
                }
                Ok(())
            }
            (Self::Integer { min, max, .. }, Value::Number(number)) => match number.as_i64() {
                Some(n) if (*min..=*max).contains(&n) => Ok(()),
                Some(_) => Err(format!("field `{id}` must be between {min} and {max}")),
                None => Err(format!("field `{id}` must be an integer")),
            },
            (Self::Boolean { .. }, Value::Bool(_)) => Ok(()),
            _ => Err(format!("field `{id}` has the wrong type")),
        }
    }
}

/// One action a plugin offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginAction {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub controls: Vec<PluginControl>,
    #[serde(default)]
    pub settings: Vec<SettingField>,
}

impl PluginAction {
    /// Validates `settings` against the form, rejecting unknown keys, wrong types and
    /// out-of-bound values, and returns the payload with defaults filled in.
    pub fn validate_settings(&self, settings: &Settings) -> Result<Settings, PluginError> {
        let invalid = |message: String| PluginError::InvalidSettings(message);
        if let Some(key) = settings.keys().find(|key| self.field(key).is_none()) {
            return Err(invalid(format!("unknown setting `{key}`")));
        }
        let mut normalized = Settings::new();
        for field in &self.settings {
            match settings
                .get(field.id())
                .cloned()
                .or_else(|| field.default_value())
            {
                Some(value) => {
                    field.check_value(&value).map_err(invalid)?;
                    normalized.insert(field.id().to_owned(), value);
                }
                None if field.required() => {
                    return Err(invalid(format!("field `{}` is required", field.id())));
                }
                None => {}
            }
        }
        let size = serde_json::to_vec(&normalized).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_SETTINGS_BYTES {
            return Err(invalid(format!(
                "settings exceed the {MAX_SETTINGS_BYTES}-byte limit"
            )));
        }
        Ok(normalized)
    }

    fn field(&self, id: &str) -> Option<&SettingField> {
        self.settings.iter().find(|field| field.id() == id)
    }
}

/// A validated `plugins/<id>/manifest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub protocol_version: u32,
    pub name: String,
    /// Path relative to the plugin directory.
    pub executable: String,
    pub actions: Vec<PluginAction>,
}

impl PluginManifest {
    /// Parses and validates manifest bytes. Never launches anything.
    pub fn parse(bytes: &[u8]) -> Result<Self, PluginError> {
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PluginError::InvalidManifest(format!(
                "manifest exceeds {MAX_MANIFEST_BYTES} bytes"
            )));
        }
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| PluginError::InvalidManifest(error.to_string()))?;
        manifest.validate().map_err(PluginError::InvalidManifest)?;
        Ok(manifest)
    }

    /// Looks an action up by id.
    pub fn action(&self, id: &str) -> Option<&PluginAction> {
        self.actions.iter().find(|action| action.id == id)
    }

    fn validate(&self) -> Result<(), String> {
        valid_id(&self.id)?;
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported protocol_version {}",
                self.protocol_version
            ));
        }
        if self.version.is_empty()
            || self.version.len() > 32
            || self.version.chars().any(char::is_control)
        {
            return Err("version must be 1-32 printable characters".into());
        }
        valid_label(&self.name)?;
        valid_relative_path(&self.executable)?;
        if self.actions.is_empty() || self.actions.len() > MAX_ACTIONS {
            return Err(format!("a plugin needs 1..={MAX_ACTIONS} actions"));
        }
        let mut action_ids = BTreeSet::new();
        for action in &self.actions {
            valid_id(&action.id)?;
            valid_label(&action.name)?;
            if action
                .description
                .as_ref()
                .is_some_and(|text| text.chars().count() > 256)
            {
                return Err(format!(
                    "action `{}` description exceeds 256 characters",
                    action.id
                ));
            }
            if action.controls.is_empty() {
                return Err(format!("action `{}` supports no controls", action.id));
            }
            if action.settings.len() > MAX_FIELDS {
                return Err(format!(
                    "action `{}` has more than {MAX_FIELDS} fields",
                    action.id
                ));
            }
            if !action_ids.insert(action.id.as_str()) {
                return Err(format!("duplicate action id `{}`", action.id));
            }
            let mut field_ids = BTreeSet::new();
            for field in &action.settings {
                field.validate_definition()?;
                if !field_ids.insert(field.id()) {
                    return Err(format!(
                        "duplicate field id `{}` in `{}`",
                        field.id(),
                        action.id
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Plugin, action and field ids: 1-48 chars of `[a-z0-9_-]`, starting alphanumeric.
pub fn valid_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if first_ok && rest_ok && id.len() <= 48 {
        Ok(())
    } else {
        Err(format!("invalid id `{id}`"))
    }
}

fn valid_label(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > 64 || text.chars().any(char::is_control) {
        Err("names and labels must be 1-64 printable characters".into())
    } else {
        Ok(())
    }
}

/// Lexical check: only plain relative components, no `.`/`..`/absolute/NUL.
pub(super) fn valid_relative_path(path: &str) -> Result<(), String> {
    use std::path::{Component, Path};
    let plain = !path.is_empty()
        && path.len() <= 128
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    if plain {
        Ok(())
    } else {
        Err(format!("unsafe executable path `{path}`"))
    }
}
