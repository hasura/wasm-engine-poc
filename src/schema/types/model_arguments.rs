use std::collections::{BTreeMap, HashMap};

use lang_graphql::ast::common as ast;
use lang_graphql::normalized_ast::InputField;

use crate::metadata::resolved;
use crate::metadata::resolved::subgraph::Qualified;
use crate::metadata::resolved::types::TypeMapping;
use crate::schema::operations;
use crate::schema::types::arguments::map_argument_value_to_ndc_type;
use crate::schema::GDS;
use lang_graphql::{mk_name, schema as gql_schema};
use open_dds::models::ModelName;
use open_dds::types::CustomTypeName;

use super::input_type::get_input_type;
use super::{Annotation, ModelInputAnnotation, TypeId};

/// Creates the `args` input object within which the model
/// arguments fields will live.
pub fn get_model_arguments_input_field(
    builder: &mut gql_schema::Builder<GDS>,
    model: &resolved::model::Model,
) -> Result<gql_schema::InputField<GDS>, crate::schema::Error> {
    model
        .graphql_api
        .arguments_input_type
        .as_ref()
        .ok_or(crate::schema::Error::NoArgumentsInputTypeForSelectMany {
            model_name: model.name.clone(),
        })
        .map(|arguments_input_type| {
            // This function call adds the model arguments to the
            // `args` input object
            builder.register_type(TypeId::ModelArgumentsInput {
                model_name: model.name.clone(),
                type_name: arguments_input_type.clone(),
            });

            gql_schema::InputField {
                name: mk_name!("args"),
                description: None,
                info: Annotation::Input(super::InputAnnotation::Model(
                    super::ModelInputAnnotation::ModelArgumentsExpression,
                )),
                field_type: ast::TypeContainer::named_non_null(arguments_input_type.clone()),
                default_value: None,
                deprecation_status: gql_schema::DeprecationStatus::NotDeprecated,
            }
        })
}

pub fn build_model_argument_fields(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    model: &resolved::model::Model,
) -> Result<
    HashMap<ast::Name, gql_schema::Namespaced<GDS, gql_schema::InputField<GDS>>>,
    crate::schema::Error,
> {
    model
        .arguments
        .iter()
        .map(|(argument_name, argument_type)| {
            let field_name = ast::Name::new(argument_name.0.as_str())?;
            let input_type = get_input_type(gds, builder, argument_type)?;
            let input_field = builder.allow_all_namespaced(
                gql_schema::InputField::new(
                    field_name.clone(),
                    None,
                    Annotation::Input(super::InputAnnotation::Model(
                        super::ModelInputAnnotation::ModelArgument {
                            argument_type: argument_type.clone(),
                            ndc_table_argument: model
                                .source
                                .as_ref()
                                .and_then(|model_source| {
                                    model_source.argument_mappings.get(argument_name)
                                })
                                .cloned(),
                        },
                    )),
                    input_type,
                    None,
                    gql_schema::DeprecationStatus::NotDeprecated,
                ),
                None,
            );
            Ok((field_name, input_field))
        })
        .collect()
}

pub fn build_model_arguments_input_schema(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    type_name: &ast::TypeName,
    model_name: &Qualified<ModelName>,
) -> Result<gql_schema::TypeInfo<GDS>, crate::schema::Error> {
    let model = gds.metadata.models.get(model_name).ok_or_else(|| {
        crate::schema::Error::InternalModelNotFound {
            model_name: model_name.clone(),
        }
    })?;

    Ok(gql_schema::TypeInfo::InputObject(
        gql_schema::InputObject::new(
            type_name.clone(),
            None,
            build_model_argument_fields(gds, builder, model)?,
        ),
    ))
}

pub fn build_ndc_model_arguments<'a, TInputFieldIter: Iterator<Item = &'a InputField<'a, GDS>>>(
    model_operation_field: &ast::Name,
    arguments: TInputFieldIter,
    model_type_mappings: &BTreeMap<Qualified<CustomTypeName>, TypeMapping>,
) -> Result<BTreeMap<String, open_dds::ndc_client::models::Argument>, operations::Error> {
    let mut ndc_arguments = BTreeMap::new();
    for argument in arguments {
        match argument.info.generic {
            Annotation::Input(super::InputAnnotation::Model(
                ModelInputAnnotation::ModelArgument {
                    argument_type,
                    ndc_table_argument,
                },
            )) => {
                let ndc_table_argument = ndc_table_argument.clone().ok_or_else(|| {
                    operations::InternalDeveloperError::NoArgumentSource {
                        field_name: model_operation_field.clone(),
                        argument_name: argument.name.clone(),
                    }
                })?;
                let mapped_argument_value = map_argument_value_to_ndc_type(
                    &argument.name,
                    argument_type,
                    &argument.value,
                    model_type_mappings,
                )?;
                ndc_arguments.insert(
                    ndc_table_argument,
                    open_dds::ndc_client::models::Argument::Literal {
                        value: mapped_argument_value,
                    },
                );
            }
            annotation => {
                return Err(operations::InternalEngineError::UnexpectedAnnotation {
                    annotation: annotation.clone(),
                })?;
            }
        }
    }
    Ok(ndc_arguments)
}
