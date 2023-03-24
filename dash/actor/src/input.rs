use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt,
    net::IpAddr,
    str::{FromStr, Split},
};

use dash_api::{
    model::{
        ModelFieldDateTimeDefaultType, ModelFieldKindNativeSpec, ModelFieldNativeSpec,
        ModelFieldSpec, ModelFieldsNativeSpec, ModelFieldsSpec,
    },
    serde_json::{Map, Value},
};
use inflector::Inflector;
use ipis::{
    async_recursion::async_recursion,
    core::{
        anyhow::{anyhow, bail, Error, Result},
        chrono::{DateTime, Utc},
        uuid::Uuid,
    },
};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::storage::StorageClient;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTemplate {
    basemap: BTreeMap<String, InputModelFieldSpec>,
    map: Value,
}

impl InputTemplate {
    pub fn new_empty(original: &ModelFieldsSpec, parsed: ModelFieldsNativeSpec) -> Self {
        Self {
            basemap: parsed
                .into_iter()
                .map(|parsed| {
                    (
                        parsed.name.clone(),
                        InputModelFieldSpec {
                            original: original
                                .iter()
                                .find(|original| original.name == parsed.name)
                                .cloned(),
                            parsed,
                        },
                    )
                })
                .collect(),
            map: Default::default(),
        }
    }

    pub async fn update_field_string(
        &mut self,
        storage: &StorageClient<'_, '_>,
        input: InputFieldString,
    ) -> Result<()> {
        let InputField { name, value } = input;

        let (base_field, field) = self.get_field(&name)?;

        match &base_field.parsed.kind {
            // BEGIN primitive types
            ModelFieldKindNativeSpec::None {} => {
                *field = Value::Null;
                Ok(())
            }
            ModelFieldKindNativeSpec::Boolean { default: _ } => {
                *field = Value::Bool(value.parse()?);
                Ok(())
            }
            ModelFieldKindNativeSpec::Integer {
                default: _,
                minimum,
                maximum,
            } => {
                let value_i64: i64 = value.parse()?;
                assert_cmp(
                    &name,
                    &value_i64,
                    "minimum",
                    minimum,
                    "greater",
                    Ordering::Greater,
                )?;
                assert_cmp(
                    &name,
                    &value_i64,
                    "maximum",
                    maximum,
                    "less",
                    Ordering::Less,
                )?;

                *field = Value::Number(value_i64.into());
                Ok(())
            }
            ModelFieldKindNativeSpec::Number {
                default: _,
                minimum,
                maximum,
            } => {
                let value_f64: f64 = value.parse()?;
                assert_cmp(
                    &name,
                    &value_f64,
                    "minimum",
                    minimum,
                    "greater",
                    Ordering::Greater,
                )?;
                assert_cmp(
                    &name,
                    &value_f64,
                    "maximum",
                    maximum,
                    "less",
                    Ordering::Less,
                )?;

                *field = Value::Number(value.parse()?);
                Ok(())
            }
            ModelFieldKindNativeSpec::String { default: _, kind } => {
                crate::imp::assert_string(&name, &value, kind)?;
                *field = Value::String(value);
                Ok(())
            }
            ModelFieldKindNativeSpec::OneOfStrings {
                default: _,
                choices,
            } => {
                crate::imp::assert_contains(&name, "choices", choices, "value", Some(&value))?;
                *field = Value::String(value);
                Ok(())
            }
            // BEGIN string formats
            ModelFieldKindNativeSpec::DateTime { default: _ } => {
                let _: DateTime<Utc> = crate::imp::assert_type(&name, &value)?;
                *field = Value::String(value);
                Ok(())
            }
            ModelFieldKindNativeSpec::Ip {} => {
                let _: IpAddr = crate::imp::assert_type(&name, &value)?;
                *field = Value::String(value);
                Ok(())
            }
            ModelFieldKindNativeSpec::Uuid {} => {
                let _: Uuid = crate::imp::assert_type(&name, &value)?;
                *field = Value::String(value);
                Ok(())
            }
            // BEGIN aggregation types
            ModelFieldKindNativeSpec::Object { .. } => {
                let input = InputFieldValue {
                    name,
                    value: storage.get(base_field.original.as_ref(), &value).await?,
                };
                self.update_field_value(storage, input).await
            }
            ModelFieldKindNativeSpec::ObjectArray { .. } => {
                error_type_mismatch(&name, &value, &base_field.parsed)
            }
        }
    }

    #[async_recursion]
    pub async fn update_field_value(
        &mut self,
        storage: &StorageClient<'_, '_>,
        input: InputFieldValue,
    ) -> Result<()> {
        let InputField { name, value } = input;

        let (base_field, field) = self.get_field(&name)?;

        match &base_field.parsed.kind {
            // BEGIN primitive types
            ModelFieldKindNativeSpec::None {} => {
                *field = Value::Null;
                Ok(())
            }
            ModelFieldKindNativeSpec::Boolean { default: _ } => {
                if value.is_boolean() {
                    *field = value;
                    Ok(())
                } else {
                    error_type_mismatch(&name, &value, &base_field.parsed)
                }
            }
            ModelFieldKindNativeSpec::Integer {
                default: _,
                minimum,
                maximum,
            } => match value.as_i64() {
                Some(value_number) => {
                    assert_cmp(
                        &name,
                        &value_number,
                        "minimum",
                        minimum,
                        "greater",
                        Ordering::Greater,
                    )?;
                    assert_cmp(
                        &name,
                        &value_number,
                        "maximum",
                        maximum,
                        "less",
                        Ordering::Less,
                    )?;

                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::Number {
                default: _,
                minimum,
                maximum,
            } => match value.as_f64() {
                Some(value_number) => {
                    assert_cmp(
                        &name,
                        &value_number,
                        "minimum",
                        minimum,
                        "greater",
                        Ordering::Greater,
                    )?;
                    assert_cmp(
                        &name,
                        &value_number,
                        "maximum",
                        maximum,
                        "less",
                        Ordering::Less,
                    )?;

                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::String { default: _, kind } => match value.as_str() {
                Some(value_str) => {
                    crate::imp::assert_string(&name, value_str, kind)?;
                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::OneOfStrings {
                default: _,
                choices,
            } => match value.as_str() {
                Some(value_string) => {
                    crate::imp::assert_contains(
                        &name,
                        "choices",
                        choices,
                        "value",
                        Some(value_string),
                    )?;
                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            // BEGIN string formats
            ModelFieldKindNativeSpec::DateTime { default: _ } => match value.as_str() {
                Some(value_string) => {
                    let _: DateTime<Utc> = crate::imp::assert_type(&name, value_string)?;
                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::Ip {} => match value.as_str() {
                Some(value_string) => {
                    let _: IpAddr = crate::imp::assert_type(&name, value_string)?;
                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::Uuid {} => match value.as_str() {
                Some(value_string) => {
                    let _: Uuid = crate::imp::assert_type(&name, value_string)?;
                    *field = value;
                    Ok(())
                }
                None => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            // BEGIN aggregation types
            ModelFieldKindNativeSpec::Object {
                children: _,
                dynamic: _,
            } => match value {
                Value::String(ref_name) => {
                    let input = InputFieldValue {
                        name,
                        value: storage.get(base_field.original.as_ref(), &ref_name).await?,
                    };
                    self.update_field_value(storage, input).await
                }
                Value::Object(children) => {
                    for (child, value) in children.into_iter() {
                        let child = InputField::sub_object(&name, &child, value);
                        self.update_field_value(storage, child).await?;
                    }
                    Ok(())
                }
                value => error_type_mismatch(&name, &value, &base_field.parsed),
            },
            ModelFieldKindNativeSpec::ObjectArray { .. } => match value {
                Value::Array(children) => {
                    for (index, value) in children.into_iter().enumerate() {
                        let child = InputField::sub_array(&name, index, value);
                        self.update_field_value(storage, child).await?;
                    }
                    Ok(())
                }
                value => error_type_mismatch(&name, &value, &base_field.parsed),
            },
        }
    }

    fn get_field(&mut self, name: &str) -> Result<(&InputModelFieldSpec, &mut Value)> {
        let mut base_field = match self.basemap.get("/") {
            Some(field) => field,
            None => bail!("no root field"),
        };
        let mut field = &mut self.map;

        for entry in CursorIterator::from_name(name) {
            field = match entry {
                CursorEntry::EnterArray { basename, index } => {
                    base_field = match self.basemap.get(&basename) {
                        Some(field) => field,
                        None => bail!("no such Array field: {name:?}"),
                    };

                    match field {
                        Value::Null => {
                            *field = Value::Array(vec![Default::default(); index]);
                            &mut field[index]
                        }
                        Value::Array(children) => {
                            if children.len() <= index {
                                children.resize(index + 1, Default::default());
                            }
                            &mut children[index]
                        }
                        _ => {
                            let type_ = base_field.parsed.kind.to_type();
                            bail!("cannot access to {type_} by Array index {index:?}: {name:?}")
                        }
                    }
                }
                CursorEntry::EnterObject { basename, child } => {
                    if child.is_empty() {
                        field
                    } else {
                        base_field = match self.basemap.get(&basename) {
                            Some(field) => field,
                            None => bail!("no such Object field: {name:?}"),
                        };

                        match field {
                            Value::Null => {
                                let mut children: Map<_, _> = Default::default();
                                children.insert(child.to_string(), Default::default());

                                *field = Value::Object(children);
                                &mut field[child]
                            }
                            Value::Object(children) => {
                                children.entry(child).or_insert(Default::default())
                            }
                            _ => {
                                let type_ = base_field.parsed.kind.to_type();
                                bail!(
                                    "cannot access to {type_} by Object field {child:?}: {name:?}"
                                )
                            }
                        }
                    }
                }
            }
        }
        Ok((base_field, field))
    }

    fn fill_default_value(&mut self, name: &str, optional: bool, is_atom: bool) -> Result<()> {
        let (base_field, field) = self.get_field(name)?;
        let optional = optional || base_field.parsed.attribute.optional;

        fn assert_optional(name: &str, optional: bool, spec: &ModelFieldNativeSpec) -> Result<()> {
            if optional {
                Ok(())
            } else {
                let type_ = spec.kind.to_type();
                bail!("missing {type_} value: {name:?}")
            }
        }

        match &base_field.parsed.kind {
            // BEGIN primitive types
            ModelFieldKindNativeSpec::None {} => {
                *field = Value::Null;
                Ok(())
            }
            ModelFieldKindNativeSpec::Boolean { default } => {
                if field.is_null() {
                    match default {
                        Some(default) => {
                            *field = Value::Bool(*default);
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::Integer {
                default,
                minimum: _,
                maximum: _,
            } => {
                if field.is_null() {
                    match default {
                        Some(default) => {
                            *field = Value::Number((*default).into());
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::Number {
                default,
                minimum: _,
                maximum: _,
            } => {
                if field.is_null() {
                    match default {
                        Some(default) => {
                            *field = Value::Number(default.to_string().parse()?);
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::String { default, kind: _ } => {
                if field.is_null() {
                    match default {
                        Some(default) => {
                            *field = Value::String(default.clone());
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::OneOfStrings {
                default,
                choices: _,
            } => {
                if field.is_null() {
                    match default {
                        Some(default) => {
                            *field = Value::String(default.clone());
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            // BEGIN string formats
            ModelFieldKindNativeSpec::DateTime { default } => {
                if field.is_null() {
                    match default {
                        Some(ModelFieldDateTimeDefaultType::Now) => {
                            *field = Value::String(Utc::now().to_rfc3339());
                            Ok(())
                        }
                        None => assert_optional(name, optional, &base_field.parsed),
                    }
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::Ip {} => {
                if field.is_null() {
                    assert_optional(name, optional, &base_field.parsed)
                } else {
                    Ok(())
                }
            }
            ModelFieldKindNativeSpec::Uuid {} => {
                if field.is_null() {
                    assert_optional(name, optional, &base_field.parsed)
                } else {
                    Ok(())
                }
            }
            // BEGIN aggregation types
            ModelFieldKindNativeSpec::Object {
                children,
                dynamic: _,
            } => {
                if field.is_null() {
                    *field = Value::Object(Default::default());
                }

                for child in crate::imp::get_children_names(children) {
                    self.fill_default_value(&format!("{name}{child}/"), optional, true)?;
                }
                Ok(())
            }
            ModelFieldKindNativeSpec::ObjectArray { children } => {
                if is_atom {
                    if field.is_null() {
                        *field = Value::Array(Default::default());
                    }

                    if let Some(children) = field.as_array() {
                        let children = 0..children.len();
                        for child in children {
                            self.fill_default_value(&format!("{name}{child}/"), optional, false)?;
                        }
                    }
                    Ok(())
                } else {
                    if field.is_null() {
                        *field = Value::Object(Default::default());
                    }

                    for child in crate::imp::get_children_names(children) {
                        self.fill_default_value(&format!("{name}{child}/"), optional, true)?;
                    }
                    Ok(())
                }
            }
        }
    }

    pub fn finalize(mut self) -> Result<Value> {
        self.fill_default_value("/", false, true).map(|()| self.map)
    }
}

pub type InputFieldString = InputField<String>;
pub type InputFieldValue = InputField<Value>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputField<Value> {
    pub name: String,
    pub value: Value,
}

impl FromStr for InputFieldString {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let re = Regex::new(crate::name::RE_SET)?;
        re.captures(s)
            .and_then(|captures| captures.iter().flatten().last())
            .map(|m| Self {
                name: s[..m.start()].to_string(),
                value: s[m.start()..m.end()].to_string(),
            })
            .ok_or_else(|| anyhow!("field name is invalid: {s} {s:?}"))
    }
}

impl<Value> InputField<Value> {
    fn sub_array(parent: &str, index: usize, value: Value) -> Self {
        Self {
            name: format!("{parent}{index}/"),
            value,
        }
    }

    fn sub_object(parent: &str, child: &str, value: Value) -> Self {
        Self {
            name: format!("{parent}{}/", child.to_snake_case()),
            value,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputModelFieldSpec {
    original: Option<ModelFieldSpec>,
    parsed: ModelFieldNativeSpec,
}

struct CursorIterator<'a> {
    basename: String,
    split: Split<'a, char>,
}

impl<'a> CursorIterator<'a> {
    fn from_name(name: &'a str) -> Self {
        CursorIterator {
            basename: '/'.to_string(),
            split: name.split('/'),
        }
    }
}

impl<'a> Iterator for CursorIterator<'a> {
    type Item = CursorEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.split.next().map(|child| match child.parse::<usize>() {
            Ok(index) => CursorEntry::EnterArray {
                basename: self.basename.clone(),
                index,
            },
            Err(_) => {
                if !child.is_empty() {
                    self.basename = format!("{}{child}/", self.basename);
                };
                CursorEntry::EnterObject {
                    basename: self.basename.clone(),
                    child,
                }
            }
        })
    }
}

enum CursorEntry<'a> {
    EnterArray { basename: String, index: usize },
    EnterObject { basename: String, child: &'a str },
}

fn assert_cmp<T>(
    name: &str,
    subject: &T,
    object_label: &str,
    object: &Option<T>,
    ordering_label: &str,
    ordering: Ordering,
) -> Result<()>
where
    T: Copy + fmt::Debug + PartialOrd,
{
    match object {
        Some(object) =>  match subject.partial_cmp(object) {
            Some(Ordering::Equal) => Ok(()),
            Some(result) if result == ordering => Ok(()),
            _ => bail!("value {subject:?} should be {ordering_label} than {object_label} value {object:?}: {name:?}"),
        }
        _ => Ok(()),
    }
}

fn error_type_mismatch<Value>(name: &str, value: Value, spec: &ModelFieldNativeSpec) -> Result<()>
where
    Value: fmt::Debug,
{
    let type_ = spec.kind.to_type();
    bail!("type mismatch; expected {type_}, but given {value:?}: {name:?}")
}
