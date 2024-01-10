use std::collections::BTreeMap;

use lang_graphql::ast::common::Name;
use lang_graphql::normalized_ast::Value;
use open_dds::types::{CustomTypeName, InbuiltType};

use crate::metadata::resolved::subgraph::{
    Qualified, QualifiedBaseType, QualifiedTypeName, QualifiedTypeReference,
};
use crate::metadata::resolved::types::TypeMapping;
use crate::schema::operations;
use crate::schema::types::Annotation;
use crate::schema::GDS;

use super::InputAnnotation;

pub fn map_argument_value_to_ndc_type(
    argument_name: &Name,
    value_type: &QualifiedTypeReference,
    value: &Value<GDS>,
    type_mappings: &BTreeMap<Qualified<CustomTypeName>, TypeMapping>,
) -> Result<serde_json::Value, operations::Error> {
    if value.is_null() {
        return Ok(serde_json::Value::Null);
    }

    match &value_type.underlying_type {
        QualifiedBaseType::List(element_type) => {
            let mapped_elements = value
                .as_list()?
                .iter()
                .map(|element_value| {
                    map_argument_value_to_ndc_type(
                        argument_name,
                        element_type,
                        element_value,
                        type_mappings,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::Value::from(mapped_elements))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::String)) => {
            Ok(serde_json::Value::from(value.as_string()?))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::Float)) => {
            Ok(serde_json::Value::from(value.as_float()?))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::Int)) => {
            Ok(serde_json::Value::from(value.as_int_i64()?))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::ID)) => {
            Ok(serde_json::Value::from(value.as_id()?))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::Boolean)) => {
            Ok(serde_json::Value::from(value.as_boolean()?))
        }
        QualifiedBaseType::Named(QualifiedTypeName::Custom(custom_type_name)) => {
            let TypeMapping::Object { field_mappings } =
                type_mappings.get(custom_type_name).ok_or_else(|| {
                    operations::InternalDeveloperError::TypeMappingNotFoundForArgument {
                        type_name: custom_type_name.clone(),
                        argument_name: argument_name.clone(),
                    }
                })?;
            let object_value = value.as_object()?;
            let mapped_fields = object_value
                .iter()
                .map(|(_gql_field_name, field_value)| {
                    let (field_name, field_type) = match field_value.info.generic {
                        Annotation::Input(InputAnnotation::InputObjectField {
                            field_name,
                            field_type,
                        }) => Ok((field_name, field_type)),
                        annotation => Err(operations::InternalEngineError::UnexpectedAnnotation {
                            annotation: annotation.clone(),
                        }),
                    }?;

                    let field_mapping = field_mappings.get(field_name).ok_or_else(|| {
                        operations::InternalEngineError::InternalGeneric {
                            description: format!("unable to find mapping for field {field_name:}"),
                        }
                    })?;

                    let mapped_field_value = map_argument_value_to_ndc_type(
                        argument_name,
                        field_type,
                        &field_value.value,
                        type_mappings,
                    )?;
                    Ok((field_mapping.column.to_string(), mapped_field_value))
                })
                .collect::<Result<serde_json::Map<String, serde_json::Value>, operations::Error>>(
                )?;

            Ok(serde_json::Value::Object(mapped_fields))
        }
    }
}
