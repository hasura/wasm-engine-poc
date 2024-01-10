//! model_source.Schema, IR and execution logic for 'select_many' operation
//!
//! A 'select_many' operation fetches zero or one row from a model

use hasura_authn_core::SessionVariables;
use lang_graphql::ast::common as ast;
use lang_graphql::ast::common::Name;
use lang_graphql::normalized_ast;
use lang_graphql::schema as gql_schema;
use open_dds;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::HashMap;

use super::model_selection::generate_int_input_argument;
use super::{Error, InternalEngineError};
use crate::metadata::resolved;
use crate::metadata::resolved::subgraph::Qualified;
use crate::schema::model_tracking::count_model;
use crate::schema::model_tracking::UsagesCounts;
use crate::schema::operations::model_selection::{model_selection_ir, ModelSelection};
use crate::schema::operations::permissions;
use crate::schema::types::ModelInputAnnotation;
use crate::schema::types::{
    self, model_arguments, model_filter::get_where_expression_input_field,
    model_filter::resolve_filter_expression, model_order_by::build_ndc_order_by,
    model_order_by::get_order_by_expression_input_field, output_type::get_custom_output_type,
    Annotation,
};
use crate::schema::GDS;

/// IR for the 'select_many' operation on a model
#[derive(Debug, Serialize)]
pub struct ModelSelectMany<'s> {
    // The name of the field as published in the schema
    pub field_name: ast::Name,

    pub model_selection: ModelSelection<'s>,

    // All the models/commands used in this operation. This includes the models/commands
    // used via relationships. And in future, the models/commands used in the filter clause
    pub(crate) usage_counts: UsagesCounts,
}

/// Generates the schema for the arguments of a model selection, which includes
/// limit, offset, order_by and where.
pub(crate) fn generate_select_many_arguments(
    builder: &mut gql_schema::Builder<GDS>,
    model: &resolved::model::Model,
) -> Result<
    HashMap<Name, gql_schema::Namespaced<GDS, gql_schema::InputField<GDS>>>,
    crate::schema::Error,
> {
    let mut arguments = HashMap::new();
    // insert limit argument
    let limit_argument = generate_int_input_argument(
        "limit".to_string(),
        Annotation::Input(types::InputAnnotation::Model(
            ModelInputAnnotation::ModelLimitArgument,
        )),
    )?;
    arguments.insert(
        limit_argument.name.clone(),
        builder.allow_all_namespaced(limit_argument, None),
    );
    // insert offset argument
    let offset_argument = generate_int_input_argument(
        "offset".to_string(),
        Annotation::Input(types::InputAnnotation::Model(
            ModelInputAnnotation::ModelOffsetArgument,
        )),
    )?;
    arguments.insert(
        offset_argument.name.clone(),
        builder.allow_all_namespaced(offset_argument, None),
    );

    // generate and insert order_by argument
    if let Some(order_by_expression_info) = &model.graphql_api.order_by_expression {
        let order_by_argument = {
            get_order_by_expression_input_field(
                builder,
                model.name.clone(),
                order_by_expression_info,
            )
        };

        arguments.insert(
            order_by_argument.name.clone(),
            builder.allow_all_namespaced(order_by_argument, None),
        );
    }

    // generate and insert where argument
    if let Some(filter_expression_info) = &model.graphql_api.filter_expression {
        let where_argument =
            get_where_expression_input_field(builder, model.name.clone(), filter_expression_info);
        arguments.insert(
            where_argument.name.clone(),
            builder.allow_all_namespaced(where_argument, None),
        );
    }

    Ok(arguments)
}

/// Generates schema for a 'select_many' operation
pub(crate) fn select_many_field(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    model: &resolved::model::Model,
    select_many: &resolved::model::SelectManyGraphQlDefinition,
    parent_type: &ast::TypeName,
) -> Result<
    (
        ast::Name,
        gql_schema::Namespaced<GDS, gql_schema::Field<GDS>>,
    ),
    crate::schema::Error,
> {
    let query_root_field = select_many.query_root_field.clone();
    let mut arguments = generate_select_many_arguments(builder, model)?;

    // Generate the `args` input object and add the model
    // arguments within it.
    if !model.arguments.is_empty() {
        let model_arguments_input =
            model_arguments::get_model_arguments_input_field(builder, model)?;

        let name = model_arguments_input.name.clone();
        if arguments
            .insert(
                name.clone(),
                builder.allow_all_namespaced(model_arguments_input, None),
            )
            .is_some()
        {
            return Err(crate::schema::Error::GraphQlArgumentConflict {
                argument_name: name,
                field_name: query_root_field,
                type_name: parent_type.clone(),
            });
        }
    }

    let field_type = ast::TypeContainer::list_null(ast::TypeContainer::named_non_null(
        get_custom_output_type(gds, builder, &model.data_type)?,
    ));

    let field = builder.conditional_namespaced(
        gql_schema::Field::new(
            query_root_field.clone(),
            None,
            Annotation::Output(types::OutputAnnotation::RootField(
                types::RootFieldAnnotation::Model {
                    data_type: model.data_type.clone(),
                    source: model.source.clone(),
                    kind: types::RootFieldKind::SelectMany,
                    name: model.name.clone(),
                },
            )),
            field_type,
            arguments,
            gql_schema::DeprecationStatus::NotDeprecated,
        ),
        permissions::get_select_permissions_namespace_annotations(model),
    );
    Ok((query_root_field, field))
}

/// Generates the IR for a 'select_many' operation
#[allow(irrefutable_let_patterns)]
pub(crate) fn select_many_generate_ir<'n, 's>(
    field: &'n normalized_ast::Field<'s, GDS>,
    field_call: &'n normalized_ast::FieldCall<'s, GDS>,
    data_type: &Qualified<open_dds::types::CustomTypeName>,
    model_source: &'s resolved::model::ModelSource,
    session_variables: &SessionVariables,
    model_name: &'s Qualified<open_dds::models::ModelName>,
) -> Result<ModelSelectMany<'s>, Error> {
    let mut limit = None;
    let mut offset = None;
    let mut filter_clause = Vec::new();
    let mut order_by = None;
    let mut model_arguments = BTreeMap::new();

    for argument in field_call.arguments.values() {
        match argument.info.generic {
            annotation @ Annotation::Input(types::InputAnnotation::Model(
                model_argument_annotation,
            )) => match model_argument_annotation {
                ModelInputAnnotation::ModelLimitArgument => {
                    limit = Some(argument.value.as_int_u32()?)
                }
                ModelInputAnnotation::ModelOffsetArgument => {
                    offset = Some(argument.value.as_int_u32()?)
                }
                ModelInputAnnotation::ModelFilterExpression => {
                    filter_clause = resolve_filter_expression(argument.value.as_object()?)?
                }
                ModelInputAnnotation::ModelArgumentsExpression => match &argument.value {
                    normalized_ast::Value::Object(arguments) => {
                        model_arguments.extend(
                            model_arguments::build_ndc_model_arguments(
                                &field_call.name,
                                arguments.values(),
                                &model_source.type_mappings,
                            )?
                            .into_iter(),
                        );
                    }
                    _ => Err(InternalEngineError::InternalGeneric {
                        description: "Expected object value for model arguments".into(),
                    })?,
                },
                ModelInputAnnotation::ModelOrderByExpression => {
                    order_by = Some(build_ndc_order_by(argument)?)
                }
                _ => {
                    return Err(InternalEngineError::UnexpectedAnnotation {
                        annotation: annotation.clone(),
                    })?
                }
            },

            annotation => {
                return Err(InternalEngineError::UnexpectedAnnotation {
                    annotation: annotation.clone(),
                })?
            }
        }
    }

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
        limit,
        offset,
        order_by,
        session_variables,
        // Get all the models/commands that were used as relationships
        &mut usage_counts,
    )?;

    Ok(ModelSelectMany {
        field_name: field_call.name.clone(),
        model_selection,
        usage_counts,
    })
}
