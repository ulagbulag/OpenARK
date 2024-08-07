mod decoder;

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Error, Result};
use arrow::datatypes::{DataType, Field, Fields, Schema as ArrowSchema, TimeUnit};
use dash_api::model::{
    ModelFieldAttributeSpec, ModelFieldKindNativeSpec, ModelFieldKindObjectSpec,
    ModelFieldNativeSpec,
};
use deltalake::kernel::{
    ArrayType, DataType as DeltaDataType, MapType, PrimitiveType,
    PrimitiveType as DeltaPrimitiveType, StructField, StructType,
};
use schemars::schema::{
    ArrayValidation, InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject, SingleOrVec,
    SubschemaValidation,
};

use super::arrow::ToDataType;

pub trait ToField {
    fn to_field(&self) -> Result<Field>;
}

impl ToField for StructField {
    fn to_field(&self) -> Result<Field> {
        self.data_type().to_field(self.name(), self.is_nullable())
    }
}

trait ToFieldByDataType {
    fn to_field(&self, name: &str, nullable: bool) -> Result<Field>;
}

impl ToFieldByDataType for DeltaDataType {
    fn to_field(&self, name: &str, nullable: bool) -> Result<Field> {
        match self {
            DeltaDataType::Primitive(type_) => type_.to_field(name, nullable),
            DeltaDataType::Struct(type_) => type_.to_field(name, nullable),
            DeltaDataType::Array(type_) => type_.to_field(name, nullable),
            DeltaDataType::Map(type_) => type_.to_field(name, nullable),
        }
    }
}

impl ToFieldByDataType for ArrayType {
    fn to_field(&self, name: &str, nullable: bool) -> Result<Field> {
        Ok(Field::new_list(
            name,
            self.element_type().to_field(name, self.contains_null())?,
            nullable,
        ))
    }
}

impl<T> ToFieldByDataType for T
where
    T: ToDataType,
{
    fn to_field(&self, name: &str, nullable: bool) -> Result<Field> {
        self.to_data_type()
            .map(|data_type| Field::new(name, data_type, nullable))
    }
}

impl ToDataType for PrimitiveType {
    fn to_data_type(&self) -> Result<DataType> {
        Ok(match self {
            Self::Boolean => DataType::Boolean,
            Self::Byte => DataType::Int8,
            Self::Short => DataType::Int16,
            Self::Integer => DataType::Int32,
            Self::Long => DataType::Int64,
            Self::Float => DataType::Float32,
            Self::Double => DataType::Float64,
            Self::Binary => DataType::Binary,
            Self::String => DataType::Utf8,
            Self::Date => DataType::Date32,
            Self::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, None),
            Self::TimestampNtz => DataType::Timestamp(TimeUnit::Microsecond, None),
            // Self::Decimal(precision, scale) => DataType::Decimal128(*precision, *scale),
            Self::Decimal(_, _) => bail!("unsupported schema data type: {self}"),
        })
    }
}

impl ToDataType for StructType {
    fn to_data_type(&self) -> Result<DataType> {
        self.fields()
            .map(|field| field.to_field())
            .collect::<Result<Fields>>()
            .map(DataType::Struct)
    }
}

impl ToDataType for MapType {
    fn to_data_type(&self) -> Result<DataType> {
        bail!("unsupported schema data type: Map")
    }
}

pub trait FieldColumns {
    fn to_data_columns(&self) -> Result<Vec<StructField>>;
}

impl FieldColumns for ArrowSchema {
    fn to_data_columns(&self) -> Result<Vec<StructField>> {
        self.fields().to_data_columns()
    }
}

impl FieldColumns for RootSchema {
    fn to_data_columns(&self) -> Result<Vec<StructField>> {
        type Definitions = ::schemars::Map<String, Schema>;

        fn find_schema_definition<'a>(
            definitions: &'a Definitions,
            schema: &'a Schema,
        ) -> Result<&'a Schema> {
            match schema {
                Schema::Object(value) => find_schema_object_definition(definitions, value)
                    .map(|result| result.unwrap_or(schema)),
                schema => Ok(schema),
            }
        }

        fn find_schema_object_definition<'a>(
            definitions: &'a Definitions,
            value: &'a SchemaObject,
        ) -> Result<Option<&'a Schema>> {
            const REFERENCE_ROOT: &str = "#/definitions/";
            match value.reference.as_ref() {
                Some(reference) if reference.starts_with(REFERENCE_ROOT) => {
                    match definitions.get(&reference[REFERENCE_ROOT.len()..]) {
                        Some(schema) => Ok(Some(schema)),
                        None => bail!("no such json schema reference: {reference:?}"),
                    }
                }
                Some(reference) => {
                    bail!("relative json schema reference is not supported yet: {reference:?}")
                }
                None => Ok(None),
            }
        }

        fn find_instance_type_none(instance_types: &[InstanceType]) -> Option<usize> {
            instance_types
                .iter()
                .enumerate()
                .find(|(_, instance_type)| matches!(instance_type, InstanceType::Null))
                .map(|(index, _)| index)
        }

        fn find_schema_none(schemas: &[Schema]) -> Option<usize> {
            schemas
                .iter()
                .enumerate()
                .find(|(_, schema)| match schema {
                    Schema::Object(value) => match &value.instance_type {
                        Some(SingleOrVec::Single(instance_type)) => {
                            matches!(**instance_type, InstanceType::Null)
                        }
                        _ => false,
                    },
                    _ => false,
                })
                .map(|(index, _)| index)
        }

        struct Context<'a> {
            definitions: &'a Definitions,
            name: &'a str,
        }

        trait JsonFieldColumn {
            fn to_data_column(
                &self,
                definitions: &Definitions,
                name: &str,
                nullable: bool,
            ) -> Result<Option<StructField>>;
        }

        impl JsonFieldColumn for Schema {
            fn to_data_column(
                &self,
                definitions: &Definitions,
                name: &str,
                nullable: bool,
            ) -> Result<Option<StructField>> {
                fn parse_instance_type(
                    Context { definitions, name }: Context,
                    value: &SchemaObject,
                    instance_type: &InstanceType,
                    nullable: bool,
                ) -> Result<Option<StructField>> {
                    Ok(match instance_type {
                        InstanceType::Null => None,
                        InstanceType::Boolean => Some(StructField::new(
                            name,
                            DeltaDataType::Primitive(DeltaPrimitiveType::Boolean),
                            nullable,
                        )),
                        InstanceType::Integer => Some(StructField::new(
                            name,
                            DeltaDataType::Primitive(DeltaPrimitiveType::Long),
                            nullable,
                        )),
                        InstanceType::Number => Some(StructField::new(
                            name,
                            DeltaDataType::Primitive(DeltaPrimitiveType::Double),
                            nullable,
                        )),
                        InstanceType::String => Some(StructField::new(
                            name,
                            DeltaDataType::Primitive(DeltaPrimitiveType::String),
                            nullable,
                        )),
                        InstanceType::Array => value
                            .array
                            .to_array_data_type(definitions)?
                            .map(Box::new)
                            .map(|type_| {
                                StructField::new(name, DeltaDataType::Array(type_), nullable)
                            }),
                        InstanceType::Object => Some(StructField::new(
                            name,
                            DeltaDataType::Struct(Box::new(StructType::new(
                                value.object.to_data_columns(definitions)?,
                            ))),
                            nullable,
                        )),
                    })
                }

                match self {
                    Schema::Bool(true) => bail!("dynamic object is not supported yet"),
                    Schema::Bool(false) => Ok(None),
                    Schema::Object(value) => {
                        let instance_type = match find_schema_object_definition(definitions, value)?
                        {
                            Some(schema) => {
                                return schema.to_data_column(definitions, name, nullable);
                            }
                            None => value.instance_type.as_ref(),
                        };

                        let ctx = Context { definitions, name };
                        Ok(match instance_type {
                            Some(SingleOrVec::Single(instance_type)) => {
                                parse_instance_type(ctx, value, instance_type, nullable)?
                            }
                            Some(SingleOrVec::Vec(instance_types)) => match instance_types.len() {
                                0 => None,
                                1 => parse_instance_type(ctx, value, &instance_types[0], nullable)?,
                                2 => match find_instance_type_none(instance_types) {
                                    Some(index) => parse_instance_type(
                                        ctx,
                                        value,
                                        &instance_types[1 - index],
                                        true,
                                    )?,
                                    None => bail!("union object is not supported"),
                                },
                                _ => bail!("union object is not supported"),
                            },
                            None => {
                                if let Some(subschemas) = value.subschemas.as_ref() {
                                    subschemas.to_enum_data_type(ctx, nullable)?
                                } else {
                                    None
                                }
                            }
                        })
                    }
                }
            }
        }

        trait JsonFieldColumnEnum {
            fn to_enum_data_type(
                &self,
                ctx: Context,
                nullable: bool,
            ) -> Result<Option<StructField>>;
        }

        impl JsonFieldColumnEnum for SubschemaValidation {
            fn to_enum_data_type(
                &self,
                ctx: Context,
                nullable: bool,
            ) -> Result<Option<StructField>> {
                if let Some(schemas) = self.any_of.as_ref() {
                    match schemas.len() {
                        0 => Ok(None),
                        1 => find_schema_definition(ctx.definitions, &schemas[0])?
                            .to_enum_data_type(ctx, nullable),
                        2 => match find_schema_none(schemas) {
                            Some(index) => {
                                find_schema_definition(ctx.definitions, &schemas[1 - index])?
                                    .to_enum_data_type(ctx, nullable)
                            }
                            None => bail!("union enum is not supported"),
                        },
                        _ => bail!("union enum is not supported"),
                    }
                } else {
                    Ok(None)
                }
            }
        }

        impl JsonFieldColumnEnum for Schema {
            fn to_enum_data_type(
                &self,
                Context { definitions, name }: Context,
                nullable: bool,
            ) -> Result<Option<StructField>> {
                self.to_data_column(definitions, name, nullable)
            }
        }

        trait JsonFieldColumnArray {
            fn to_array_data_type(&self, definitions: &Definitions) -> Result<Option<ArrayType>>;
        }

        impl JsonFieldColumnArray for Schema {
            fn to_array_data_type(&self, definitions: &Definitions) -> Result<Option<ArrayType>> {
                fn parse_instance_type(
                    definitions: &Definitions,
                    instance_type: &InstanceType,
                    nullable: bool,
                    value: &SchemaObject,
                ) -> Result<Option<ArrayType>> {
                    Ok(match instance_type {
                        InstanceType::Null => None,
                        InstanceType::Boolean => Some(ArrayType::new(
                            DeltaDataType::Primitive(DeltaPrimitiveType::Boolean),
                            nullable,
                        )),
                        InstanceType::Integer => Some(ArrayType::new(
                            DeltaDataType::Primitive(DeltaPrimitiveType::Long),
                            nullable,
                        )),
                        InstanceType::Number => Some(ArrayType::new(
                            DeltaDataType::Primitive(DeltaPrimitiveType::Double),
                            nullable,
                        )),
                        InstanceType::String => Some(ArrayType::new(
                            DeltaDataType::Primitive(DeltaPrimitiveType::String),
                            nullable,
                        )),
                        InstanceType::Array => value
                            .array
                            .to_array_data_type(definitions)?
                            .map(Box::new)
                            .map(|type_| ArrayType::new(DeltaDataType::Array(type_), nullable)),
                        InstanceType::Object => Some(ArrayType::new(
                            DeltaDataType::Struct(Box::new(StructType::new(
                                value.object.to_data_columns(definitions)?,
                            ))),
                            nullable,
                        )),
                    })
                }

                let nullable = false;
                match self {
                    Schema::Bool(true) => {
                        bail!("dynamic array is not supported yet")
                    }
                    Schema::Bool(false) => Ok(None),
                    Schema::Object(value) => {
                        match find_schema_object_definition(definitions, value)? {
                            Some(schema) => schema.to_array_data_type(definitions),
                            None => match &value.instance_type {
                                Some(SingleOrVec::Single(instance_type)) => {
                                    parse_instance_type(definitions, instance_type, nullable, value)
                                }
                                Some(SingleOrVec::Vec(instance_types)) => {
                                    match instance_types.len() {
                                        0 => Ok(None),
                                        1 => parse_instance_type(
                                            definitions,
                                            &instance_types[0],
                                            nullable,
                                            value,
                                        ),
                                        2 => match find_instance_type_none(instance_types) {
                                            Some(index) => parse_instance_type(
                                                definitions,
                                                &instance_types[1 - index],
                                                true,
                                                value,
                                            ),
                                            None => bail!("union array is not supported"),
                                        },
                                        _ => bail!("union array is not supported"),
                                    }
                                }
                                None => Ok(None),
                            },
                        }
                    }
                }
            }
        }

        impl JsonFieldColumnArray for Option<&SingleOrVec<Schema>> {
            fn to_array_data_type(&self, definitions: &Definitions) -> Result<Option<ArrayType>> {
                match self {
                    Some(SingleOrVec::Single(value)) => value.to_array_data_type(definitions),
                    Some(SingleOrVec::Vec(_)) => {
                        bail!("union array is not supported")
                    }
                    None => Ok(None),
                }
            }
        }

        impl JsonFieldColumnArray for Option<Box<ArrayValidation>> {
            fn to_array_data_type(&self, definitions: &Definitions) -> Result<Option<ArrayType>> {
                self.as_ref()
                    .and_then(|value| value.items.as_ref())
                    .to_array_data_type(definitions)
            }
        }

        trait JsonFieldColumns {
            fn to_data_columns(&self, definitions: &Definitions) -> Result<Vec<StructField>>;
        }

        impl JsonFieldColumns for Box<ObjectValidation> {
            fn to_data_columns(&self, definitions: &Definitions) -> Result<Vec<StructField>> {
                self.properties
                    .iter()
                    .filter_map(|(child_name, child)| {
                        let nullable = !self.required.contains(child_name);
                        child
                            .to_data_column(definitions, child_name, nullable)
                            .transpose()
                    })
                    .collect()
            }
        }

        impl JsonFieldColumns for Option<Box<ObjectValidation>> {
            fn to_data_columns(&self, definitions: &Definitions) -> Result<Vec<StructField>> {
                match self {
                    Some(value) => value.to_data_columns(definitions),
                    None => Ok(Default::default()),
                }
            }
        }

        let definitions = &self.definitions;

        // is metadta value dynamic?
        if self
            .schema
            .object
            .as_ref()
            .and_then(|object| object.properties.get("value"))
            .map(|value| matches!(value, Schema::Bool(true)))
            .unwrap_or_default()
        {
            Ok(Default::default())
        } else {
            self.schema.object.to_data_columns(definitions)
        }
    }
}

impl FieldColumns for [ModelFieldNativeSpec] {
    fn to_data_columns(&self) -> Result<Vec<StructField>> {
        struct FieldBuilder {
            name: String,
            type_: FieldBuilderType,
            attributes: ModelFieldAttributeSpec,
        }

        impl FieldBuilder {
            fn push<'a>(
                &mut self,
                mut child_names: impl Iterator<Item = &'a str>,
                name: &'a str,
                field: &'a ModelFieldNativeSpec,
            ) -> Result<()> {
                match &mut self.type_ {
                    FieldBuilderType::Object(children) => match child_names.next() {
                        Some(child_name) => children
                            .entry(name.into())
                            .or_insert(Self {
                                name: name.into(),
                                type_: FieldBuilderType::Object(Default::default()),
                                attributes: field.attribute,
                            })
                            .push(child_names, child_name, field),
                        None => match &field.kind {
                            // BEGIN primitive types
                            ModelFieldKindNativeSpec::None {} => Ok(()),
                            ModelFieldKindNativeSpec::Boolean { default: _ } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::Boolean,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::Integer {
                                default: _,
                                minimum: _,
                                maximum: _,
                            } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::Integer,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::Number {
                                default: _,
                                minimum: _,
                                maximum: _,
                            } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::Number,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::String {
                                default: _,
                                kind: _,
                            } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::String,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::OneOfStrings {
                                default: _,
                                choices: _,
                            } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::String,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            // BEGIN string formats
                            ModelFieldKindNativeSpec::DateTime { default: _ } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::DateTime,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::Ip {} => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::String,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::Uuid {} => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Primitive(
                                            FieldBuilderPrimitiveType::String,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            // BEGIN aggregation types
                            ModelFieldKindNativeSpec::StringArray {} => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Array(
                                            FieldBuilderArrayType::Primitive(
                                                FieldBuilderPrimitiveType::String,
                                            ),
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                            ModelFieldKindNativeSpec::Object { children: _, kind } => match kind {
                                ModelFieldKindObjectSpec::Dynamic {} => {
                                    children.insert(
                                        name.into(),
                                        Self {
                                            name: name.into(),
                                            type_: FieldBuilderType::Dynamic,
                                            attributes: field.attribute,
                                        },
                                    );
                                    Ok(())
                                }
                                ModelFieldKindObjectSpec::Enumerate { choices: _ }
                                | ModelFieldKindObjectSpec::Static {} => {
                                    children.insert(
                                        name.into(),
                                        Self {
                                            name: name.into(),
                                            type_: FieldBuilderType::Object(Default::default()),
                                            attributes: field.attribute,
                                        },
                                    );
                                    Ok(())
                                }
                            },
                            ModelFieldKindNativeSpec::ObjectArray { children: _ } => {
                                children.insert(
                                    name.into(),
                                    Self {
                                        name: name.into(),
                                        type_: FieldBuilderType::Array(
                                            FieldBuilderArrayType::Object,
                                        ),
                                        attributes: field.attribute,
                                    },
                                );
                                Ok(())
                            }
                        },
                    },
                    _ => bail!("the parent field should be Object"),
                }
            }

            fn try_into_children(self) -> Result<BTreeMap<String, Self>> {
                match self.type_ {
                    FieldBuilderType::Object(children) => Ok(children),
                    _ => bail!("cannot convert field builder to object"),
                }
            }
        }

        impl TryFrom<FieldBuilder> for StructField {
            type Error = Error;

            fn try_from(field: FieldBuilder) -> Result<Self> {
                let FieldBuilder {
                    name,
                    type_,
                    attributes: ModelFieldAttributeSpec { optional: nullable },
                } = field;

                Ok(Self::new(
                    name,
                    match type_ {
                        FieldBuilderType::Primitive(type_) => type_.into(),
                        FieldBuilderType::Array(type_) => {
                            DeltaDataType::Array(Box::new(ArrayType::new(
                                match type_ {
                                    FieldBuilderArrayType::Primitive(type_) => type_.into(),
                                    FieldBuilderArrayType::Object => {
                                        bail!("object array is not supported yet")
                                    }
                                },
                                nullable,
                            )))
                        }
                        FieldBuilderType::Object(children) => {
                            DeltaDataType::Struct(Box::new(StructType::new(
                                children
                                    .into_values()
                                    .map(TryInto::try_into)
                                    .collect::<Result<_>>()?,
                            )))
                        }
                        FieldBuilderType::Dynamic => bail!("dynamic array is not supported yet"),
                    },
                    nullable,
                ))
            }
        }

        enum FieldBuilderType {
            Primitive(FieldBuilderPrimitiveType),
            Array(FieldBuilderArrayType),
            Object(BTreeMap<String, FieldBuilder>),
            Dynamic,
        }

        enum FieldBuilderArrayType {
            Primitive(FieldBuilderPrimitiveType),
            Object,
        }

        enum FieldBuilderPrimitiveType {
            Boolean,
            Integer,
            Number,
            String,
            DateTime,
        }

        impl From<FieldBuilderPrimitiveType> for DeltaDataType {
            fn from(value: FieldBuilderPrimitiveType) -> Self {
                match value {
                    FieldBuilderPrimitiveType::Boolean => {
                        DeltaDataType::Primitive(DeltaPrimitiveType::Boolean)
                    }
                    FieldBuilderPrimitiveType::Integer => {
                        DeltaDataType::Primitive(DeltaPrimitiveType::Long)
                    }
                    FieldBuilderPrimitiveType::Number => {
                        DeltaDataType::Primitive(DeltaPrimitiveType::Double)
                    }
                    FieldBuilderPrimitiveType::String => {
                        DeltaDataType::Primitive(DeltaPrimitiveType::String)
                    }
                    FieldBuilderPrimitiveType::DateTime => {
                        DeltaDataType::Primitive(DeltaPrimitiveType::Timestamp)
                    }
                }
            }
        }

        let root = match self.first() {
            Some(root) => root,
            None => return Ok(Default::default()),
        };
        let mut root = FieldBuilder {
            name: Default::default(),
            type_: FieldBuilderType::Object(Default::default()),
            attributes: root.attribute,
        };

        for field in &self[1..] {
            let mut field_child_names = field.name[1..field.name.len() - 1].split('/');
            let field_name = field_child_names
                .next()
                .ok_or_else(|| anyhow!("fields are not ordered"))?;
            root.push(field_child_names, field_name, field)?;
        }
        root.try_into_children()
            .and_then(|children| children.into_values().map(TryInto::try_into).collect())
    }
}

impl FieldColumns for Vec<ModelFieldNativeSpec> {
    fn to_data_columns(&self) -> Result<Vec<StructField>> {
        self.as_slice().to_data_columns()
    }
}

trait FieldChildren {
    fn to_data_columns(&self) -> Result<Vec<StructField>>;
}

impl FieldChildren for Fields {
    fn to_data_columns(&self) -> Result<Vec<StructField>> {
        self.iter()
            .filter_map(|field| field.to_data_column().transpose())
            .collect()
    }
}

trait FieldChild {
    fn to_data_column(&self) -> Result<Option<StructField>>;
}

impl FieldChild for Field {
    fn to_data_column(&self) -> Result<Option<StructField>> {
        self.data_type().to_data_type().map(|type_| {
            type_.map(|type_| StructField::new(self.name().clone(), type_, self.is_nullable()))
        })
    }
}

trait FieldSchema {
    fn to_data_type(&self) -> Result<Option<DeltaDataType>>;
}

impl FieldSchema for Field {
    fn to_data_type(&self) -> Result<Option<DeltaDataType>> {
        self.data_type().to_data_type()
    }
}

impl FieldSchema for DataType {
    fn to_data_type(&self) -> Result<Option<DeltaDataType>> {
        Ok(match self {
            // BEGIN primitive types
            DataType::Null => None,
            DataType::Boolean => Some(DeltaDataType::Primitive(DeltaPrimitiveType::Boolean)),
            DataType::Int8 | DataType::UInt8 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Byte))
            }
            DataType::Int16 | DataType::UInt16 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Short))
            }
            DataType::Int32 | DataType::UInt32 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Integer))
            }
            DataType::Int64 | DataType::UInt64 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Long))
            }
            // DataType::Float16 => todo!(),
            DataType::Float32 => Some(DeltaDataType::Primitive(DeltaPrimitiveType::Float)),
            DataType::Float64 => Some(DeltaDataType::Primitive(DeltaPrimitiveType::Double)),
            DataType::Decimal128(precision, scale) | DataType::Decimal256(precision, scale) => {
                Some(DeltaDataType::decimal(*precision, (*scale).try_into()?)?)
            }
            // BEGIN binary formats
            DataType::Binary | DataType::FixedSizeBinary(_) | DataType::LargeBinary => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Binary))
            }
            // BEGIN string formats
            DataType::Utf8 | DataType::LargeUtf8 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::String))
            }
            DataType::Date32 | DataType::Date64 => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Date))
            }
            // DataType::Duration(_) => todo!(),
            // DataType::Interval(_) => todo!(),
            // DataType::Time32(_) => todo!(),
            // DataType::Time64(_) => todo!(),
            DataType::Timestamp(_, _) => {
                Some(DeltaDataType::Primitive(DeltaPrimitiveType::Timestamp))
            }
            // BEGIN aggregation types
            DataType::Union(_, _) => bail!("union data type is not supported"),
            DataType::FixedSizeList(field, _)
            | DataType::List(field)
            | DataType::LargeList(field) => field
                .to_data_type()?
                .map(Into::into)
                .map(|type_| ArrayType::new(type_, field.is_nullable()))
                .map(Box::new)
                .map(DeltaDataType::Array),
            DataType::Struct(fields) => Some(DeltaDataType::Struct(Box::new(StructType::new(
                fields.to_data_columns()?,
            )))),
            // DataType::Dictionary(_, _) => todo!(),
            // DataType::Map(_, _) => todo!(),
            type_ => bail!("unsupported data type: {type_:?}"),
            // DataType::RunEndEncoded(_, _) => todo!(),
        })
    }
}
