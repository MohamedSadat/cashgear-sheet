//! The entity's schema as the ERP sends it (`GET {entityRoute}/Schema`, contract C.2), and the
//! fields a binding keeps from it, resolved for the session's language.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Ar,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Localized {
    pub en: String,
    pub ar: String,
}

impl Localized {
    pub fn get(&self, language: Language) -> &str {
        match language {
            Language::En => &self.en,
            Language::Ar => &self.ar,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSchema {
    pub name: String,
    pub label: Localized,
    #[serde(rename = "type")]
    pub kind: String,
    pub editable: bool,
    #[serde(default)]
    pub edit_reason: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub scale: Option<u32>,
    /// A number, or a decimal's text as decimals travel (contract, `min?: number | string`).
    #[serde(default, deserialize_with = "number_or_text")]
    pub min: Option<f64>,
    #[serde(default, deserialize_with = "number_or_text")]
    pub max: Option<f64>,
    #[serde(default)]
    pub lookup: Option<String>,
}

/// A limit given as a number or as a decimal's text, such as `"0"`.
fn number_or_text<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Limit {
        Number(f64),
        Text(String),
    }
    match Option::<Limit>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Limit::Number(n)) => Ok(Some(n)),
        Some(Limit::Text(text)) => {
            text.trim().parse::<f64>().map(Some).map_err(|_| {
                serde::de::Error::custom(format!("a limit that isn't a number: {text}"))
            })
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupItem {
    pub code: String,
    pub label: Localized,
    #[serde(default)]
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySchema {
    pub entity: String,
    pub schema_version: String,
    pub label: Localized,
    pub key_field: String,
    pub id_field: String,
    pub max_page_size: u32,
    pub max_records: u32,
    pub max_commit_records: u32,
    pub fields: Vec<FieldSchema>,
    #[serde(default)]
    pub lookups: BTreeMap<String, Vec<LookupItem>>,
}

/// What a field holds, and so how a cell shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Integer,
    /// Digits after the point.
    Decimal(u32),
    Boolean,
    Date,
    DateTime,
    /// The name of its list in the schema's lookups. Cells hold the codes.
    Lookup(String),
}

impl FieldKind {
    /// The number format a bound column's cells get, as Excel's codes write it. Text and codes are
    /// `@`, so a code such as `001` stays text if it is ever typed again.
    pub fn number_format(&self) -> Option<String> {
        match self {
            Self::Text | Self::Lookup(_) => Some("@".to_string()),
            Self::Integer => Some("0".to_string()),
            Self::Decimal(0) => Some("#,##0".to_string()),
            Self::Decimal(scale) => Some(format!("#,##0.{}", "0".repeat(*scale as usize))),
            Self::Boolean => None,
            Self::Date => Some("yyyy-mm-dd".to_string()),
            Self::DateTime => Some("yyyy-mm-dd hh:mm".to_string()),
        }
    }
}

/// One item of a lookup list, as a binding keeps it: its code, its label in the session's
/// language, and whether a change may choose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupEntry {
    pub code: String,
    pub label: String,
    pub active: bool,
}

/// A field as a binding keeps it: its column, in the schema's order.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundField {
    pub name: String,
    /// The heading, in the session's language.
    pub label: String,
    pub kind: FieldKind,
    /// What the schema says for this caller: whether the sheet lets its cells be edited.
    pub editable: bool,
    pub edit_reason: Option<String>,
    /// The constraints local rules check (contract, "Schema").
    pub required: bool,
    pub max_length: Option<u32>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl EntitySchema {
    /// Reads and checks a schema the sheet can bind: a key field among the fields, known types,
    /// unique names, decimals with a scale and lookups whose list is there.
    pub fn parse(json: &str) -> Result<Self, String> {
        let schema: Self =
            serde_json::from_str(json).map_err(|error| format!("The ERP's schema: {error}"))?;
        schema.check()?;
        Ok(schema)
    }

    fn check(&self) -> Result<(), String> {
        if self.fields.is_empty() {
            return Err("The ERP's schema has no fields.".to_string());
        }
        if self.fields.len() > 256 {
            return Err("The ERP's schema has more fields than a sheet binds.".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for field in &self.fields {
            if !seen.insert(field.name.as_str()) {
                return Err(format!("The ERP's schema lists {} twice.", field.name));
            }
            self.kind_of(field)?;
        }
        if !seen.contains(self.key_field.as_str()) {
            return Err("The ERP's schema has no key field.".to_string());
        }
        if seen.contains(self.id_field.as_str()) {
            return Err("The ERP's schema sends its id as a field.".to_string());
        }
        if self.max_page_size == 0 || self.max_records == 0 {
            return Err("The ERP's schema has no room for records.".to_string());
        }
        Ok(())
    }

    fn kind_of(&self, field: &FieldSchema) -> Result<FieldKind, String> {
        Ok(match field.kind.as_str() {
            "text" => FieldKind::Text,
            "integer" => FieldKind::Integer,
            "decimal" => match field.scale {
                Some(scale) if scale <= 10 => FieldKind::Decimal(scale),
                _ => return Err(format!("{} has no scale.", field.name)),
            },
            "boolean" => FieldKind::Boolean,
            "date" => FieldKind::Date,
            "dateTime" => FieldKind::DateTime,
            "lookup" => match &field.lookup {
                Some(list) if self.lookups.contains_key(list) => FieldKind::Lookup(list.clone()),
                _ => return Err(format!("{}'s list isn't in the schema.", field.name)),
            },
            other => {
                return Err(format!(
                    "{} has a type the sheet doesn't know: {other}.",
                    field.name
                ))
            }
        })
    }

    /// The fields in column order, with headings in `language`.
    pub fn bound_fields(&self, language: Language) -> Vec<BoundField> {
        self.fields
            .iter()
            .map(|field| BoundField {
                name: field.name.clone(),
                label: field.label.get(language).to_string(),
                kind: self.kind_of(field).expect("checked when parsed"),
                editable: field.editable,
                edit_reason: field.edit_reason.clone(),
                required: field.required.unwrap_or(false),
                max_length: field.max_length,
                min: field.min,
                max: field.max,
            })
            .collect()
    }

    /// Each lookup list, with labels in `language`, for the fields that name it.
    pub fn lookup_lists(&self, language: Language) -> BTreeMap<String, Vec<LookupEntry>> {
        self.lookups
            .iter()
            .map(|(name, items)| {
                let entries = items
                    .iter()
                    .map(|item| LookupEntry {
                        code: item.code.clone(),
                        label: item.label.get(language).to_string(),
                        active: item.active.unwrap_or(true),
                    })
                    .collect();
                (name.clone(), entries)
            })
            .collect()
    }

    pub fn key_index(&self) -> usize {
        self.fields
            .iter()
            .position(|field| field.name == self.key_field)
            .expect("checked when parsed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_limits(limits: &str) -> Result<EntitySchema, String> {
        EntitySchema::parse(&format!(
            r#"{{"entity":"vendors","schemaVersion":"vendors.1","label":{{"en":"Vendors","ar":"x"}},
            "keyField":"vendorAccount","idField":"recId","maxPageSize":1000,"maxRecords":10000,"maxCommitRecords":500,
            "fields":[
              {{"name":"vendorAccount","label":{{"en":"Account","ar":"x"}},"type":"text","editable":false}},
              {{"name":"debitLimit","label":{{"en":"Credit limit","ar":"x"}},"type":"decimal","scale":2,"editable":true{limits}}}],
            "lookups":{{}}}}"#
        ))
    }

    #[test]
    fn limits_come_as_numbers_or_as_a_decimals_text() {
        let schema = with_limits(r#","min":"0","max":"1000000.50""#).unwrap();
        let fields = schema.bound_fields(Language::En);
        assert_eq!(fields[1].min, Some(0.0));
        assert_eq!(fields[1].max, Some(1_000_000.5));
        let schema = with_limits(r#","min":-5,"max":null"#).unwrap();
        let fields = schema.bound_fields(Language::En);
        assert_eq!((fields[1].min, fields[1].max), (Some(-5.0), None));
        let schema = with_limits("").unwrap();
        assert_eq!(schema.bound_fields(Language::En)[1].min, None);
        assert!(with_limits(r#","min":"zero""#).is_err());
    }
}
