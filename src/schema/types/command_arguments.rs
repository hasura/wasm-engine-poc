use std::collections::{BTreeMap, HashMap};

use lang_graphql::ast::common as ast;
use lang_graphql::normalized_ast::InputField;
use open_dds::types::CustomTypeName;

use crate::metadata::resolved::subgraph::Qualified;
use crate::metadata::resolved::types::TypeMapping;
use crate::schema::operations;
use crate::schema::types::arguments::map_argument_value_to_ndc_type;
use crate::schema::types::Annotation;
use crate::schema::GDS;

use super::InputAnnotation;

pub fn build_ndc_command_arguments(
    command_field: &ast::Name,
    argument: &InputField<GDS>,
    command_type_mappings: &BTreeMap<Qualified<CustomTypeName>, TypeMapping>,
) -> Result<HashMap<String, serde_json::Value>, operations::Error> {
    let mut ndc_arguments = HashMap::new();

    match argument.info.generic {
        Annotation::Input(InputAnnotation::CommandArgument {
            argument_type,
            ndc_func_proc_argument,
        }) => {
            let ndc_func_proc_argument = ndc_func_proc_argument.clone().ok_or_else(|| {
                operations::InternalDeveloperError::NoArgumentSource {
                    field_name: command_field.clone(),
                    argument_name: argument.name.clone(),
                }
            })?;
            let mapped_argument_value = map_argument_value_to_ndc_type(
                &argument.name,
                argument_type,
                &argument.value,
                command_type_mappings,
            )?;
            ndc_arguments.insert(ndc_func_proc_argument, mapped_argument_value);
            Ok(())
        }
        annotation => Err(operations::InternalEngineError::UnexpectedAnnotation {
            annotation: annotation.clone(),
        }),
    }?;
    Ok(ndc_arguments)
}
