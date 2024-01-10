//! model_source.Schema, IR and execution logic for 'select_one' operation
//!
//! A 'select_one' operation fetches zero or one row from a model

use hasura_authn_core::SessionVariables;
use lang_graphql::{ast::common as ast, normalized_ast, schema as gql_schema};
use open_dds::ndc_client as ndc;
use open_dds;
use serde::Serialize;
use std::collections::HashMap;

use super::model_selection::model_selection_ir;
use super::{Error, InternalEngineError};
use crate::metadata::resolved;
use crate::metadata::resolved::subgraph::Qualified;
use crate::metadata::resolved::types::mk_name;
use crate::schema::types::output_type::get_object_type_representation;
use crate::schema::GDS;
use crate::schema::{
    model_tracking::{count_model, UsagesCounts},
    operations::{model_selection::ModelSelection, permissions},
    types::{
        self, input_type::get_input_type, model_arguments, output_type::get_custom_output_type,
        Annotation, ModelInputAnnotation,
    },
};

/// IR for the 'select_one' operation on a model
#[derive(Serialize, Debug)]
pub struct ModelSelectOne<'s> {
    // The name of the field as published in the schema
    pub field_name: ast::Name,

    pub model_selection: ModelSelection<'s>,

    // All the models/commands used in this operation. This includes the models/commands
    // used via relationships. And in future, the models/commands used in the filter clause
    pub(crate) usage_counts: UsagesCounts,
}

/// Generates schema for a 'select_one' operation
pub(crate) fn select_one_field(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    model: &resolved::model::Model,
    select_unique: &resolved::model::SelectUniqueGraphQlDefinition,
    parent_type: &ast::TypeName,
) -> Result<
    (
        ast::Name,
        gql_schema::Namespaced<GDS, gql_schema::Field<GDS>>,
    ),
    crate::schema::Error,
> {
    let query_root_field = select_unique.query_root_field.clone();

    let mut arguments = HashMap::new();
    for (field_name, field_type) in select_unique.unique_identifier.iter() {
        let graphql_field_name = mk_name(field_name.0.as_str())?;
        let argument = gql_schema::InputField::new(
            graphql_field_name,
            None,
            Annotation::Input(types::InputAnnotation::Model(
                ModelInputAnnotation::ModelUniqueIdentifierArgument {
                    field_name: field_name.clone(),
                },
            )),
            get_input_type(gds, builder, field_type)?,
            None,
            gql_schema::DeprecationStatus::NotDeprecated,
        );
        arguments.insert(
            argument.name.clone(),
            builder.allow_all_namespaced(argument, None),
        );
    }

    for (argument_field_name, argument_field) in
        model_arguments::build_model_argument_fields(gds, builder, model)?
    {
        if arguments
            .insert(argument_field_name.clone(), argument_field)
            .is_some()
        {
            return Err(crate::schema::Error::GraphQlArgumentConflict {
                argument_name: argument_field_name,
                field_name: query_root_field,
                type_name: parent_type.clone(),
            });
        }
    }

    let object_type_representation = get_object_type_representation(gds, &model.data_type)?;
    let output_typename = get_custom_output_type(gds, builder, &model.data_type)?;

    let field = builder.conditional_namespaced(
        gql_schema::Field::new(
            query_root_field.clone(),
            None,
            Annotation::Output(types::OutputAnnotation::RootField(
                types::RootFieldAnnotation::Model {
                    data_type: model.data_type.clone(),
                    source: model.source.clone(),
                    kind: types::RootFieldKind::SelectOne,
                    name: model.name.clone(),
                },
            )),
            ast::TypeContainer::named_null(output_typename),
            arguments,
            gql_schema::DeprecationStatus::NotDeprecated,
        ),
        permissions::get_select_one_namespace_annotations(
            model,
            object_type_representation,
            select_unique,
        ),
    );
    Ok((query_root_field, field))
}

/// Generates the IR for a 'select_one' operation
// TODO: Remove once TypeMapping has more than one variant
#[allow(irrefutable_let_patterns)]
pub(crate) fn select_one_generate_ir<'s>(
    field: &normalized_ast::Field<'s, GDS>,
    field_call: &normalized_ast::FieldCall<'s, GDS>,
    data_type: &Qualified<open_dds::types::CustomTypeName>,
    model_source: &'s resolved::model::ModelSource,
    session_variables: &SessionVariables,
    model_name: &'s Qualified<open_dds::models::ModelName>,
) -> Result<ModelSelectOne<'s>, Error> {
    let field_mappings = model_source
        .type_mappings
        .get(data_type)
        .and_then(|type_mapping| {
            if let resolved::types::TypeMapping::Object { field_mappings } = type_mapping {
                Some(field_mappings)
            } else {
                None
            }
        })
        .ok_or_else(|| InternalEngineError::InternalGeneric {
            description: format!("type '{:}' not found in source type_mappings", data_type),
        })?;

    let mut filter_clause = vec![];
    let mut model_argument_fields = Vec::new();
    for argument in field_call.arguments.values() {
        match argument.info.generic {
            annotation @ Annotation::Input(types::InputAnnotation::Model(
                model_input_argument_annotation,
            )) => match model_input_argument_annotation {
                ModelInputAnnotation::ModelArgument { .. } => {
                    model_argument_fields.push(argument);
                }
                ModelInputAnnotation::ModelUniqueIdentifierArgument { field_name } => {
                    let field_mapping = &field_mappings.get(field_name).ok_or_else(|| {
                        InternalEngineError::InternalGeneric {
                            description: format!(
                                "invalid unique identifier field in annotation: {field_name:}"
                            ),
                        }
                    })?;
                    let ndc_expression = ndc::models::Expression::BinaryComparisonOperator {
                        column: ndc::models::ComparisonTarget::Column {
                            name: field_mapping.column.clone(),
                            path: vec![],
                        },
                        operator: ndc::models::BinaryComparisonOperator::Equal,
                        value: ndc::models::ComparisonValue::Scalar {
                            value: argument.value.as_json(),
                        },
                    };
                    filter_clause.push(ndc_expression);
                }
                _ => Err(InternalEngineError::UnexpectedAnnotation {
                    annotation: annotation.clone(),
                })?,
            },
            annotation => Err(InternalEngineError::UnexpectedAnnotation {
                annotation: annotation.clone(),
            })?,
        }
    }
    let model_arguments = model_arguments::build_ndc_model_arguments(
        &field_call.name,
        model_argument_fields.into_iter(),
        &model_source.type_mappings,
    )?;

    // Add the name of the root model
    let mut usage_counts = UsagesCounts::new();
    count_model(model_name.clone(), &mut usage_counts);

    let model_selection = model_selection_ir(
        &field.selection_set,
        data_type,
        model_source,
        model_arguments,
        filter_clause,
        permissions::get_select_filter_predicate(field_call)?,
        None, // limit
        None, // offset
        None, // order_by
        session_variables,
        // Get all the models/commands that were used as relationships
        &mut usage_counts,
    )?;

    Ok(ModelSelectOne {
        field_name: field_call.name.clone(),
        model_selection,
        usage_counts,
    })
}
