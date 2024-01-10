//! Schema, IR and execution logic for commands
//!
//! A 'command' executes a function/procedure and returns back the result of the execution.

use hasura_authn_core::SessionVariables;
use lang_graphql::ast::common as ast;
use lang_graphql::ast::common::TypeContainer;
use lang_graphql::ast::common::TypeName;
use lang_graphql::normalized_ast;
use lang_graphql::schema as gql_schema;
use lang_graphql::schema::InputField;
use lang_graphql::schema::Namespaced;
use open_dds::ndc_client as gdc;
use open_dds::commands::{self, DataConnectorCommand};
use serde::Serialize;
use serde_json as json;
use std::collections::BTreeMap;
use std::collections::HashMap;

use super::remote_joins::JoinLocations;
use super::remote_joins::MonotonicCounter;
use super::remote_joins::RemoteJoin;
use super::{Error, InternalEngineError};
use crate::metadata::resolved;
use crate::metadata::resolved::subgraph;
use crate::schema::model_tracking::count_command;
use crate::schema::model_tracking::UsagesCounts;
use crate::schema::operations::permissions;
use crate::schema::types::command_arguments;
use crate::schema::types::output_type::collect_relationships;
use crate::schema::types::output_type::process_selection_set_ir;
use crate::schema::types::{self, output_type::get_output_type, Annotation};
use crate::schema::GDS;

/// IR for the 'command' operations
#[derive(Serialize, Debug)]
pub struct CommandRepresentation<'s> {
    /// The name of the command
    pub command_name: subgraph::Qualified<commands::CommandName>,

    /// The name of the field as published in the schema
    pub field_name: ast::Name,

    /// The data connector backing this model.
    pub data_connector: resolved::data_connector::DataConnector,

    /// Source function/procedure in the data connector for this model
    pub ndc_source: DataConnectorCommand,

    /// Arguments for the NDC table
    pub(crate) arguments: BTreeMap<String, json::Value>,

    /// IR for the command result selection set
    pub(crate) selection: types::output_type::ResultSelectionSet<'s>,

    /// The Graphql base type for the output_type of command. Helps in deciding how
    /// the response from the NDC needs to be processed.
    pub type_container: TypeContainer<TypeName>,

    // All the models/commands used in the 'command' operation.
    pub(crate) usage_counts: UsagesCounts,
}

pub enum Response {
    QueryResponse {
        response: gdc::models::QueryResponse,
    },
    MutationResponse {
        response: gdc::models::MutationResponse,
    },
}

pub(crate) fn command_field(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    command: &resolved::command::Command,
    command_field_name: ast::Name,
) -> Result<
    (
        ast::Name,
        gql_schema::Namespaced<GDS, gql_schema::Field<GDS>>,
    ),
    crate::schema::Error,
> {
    let output_typename = get_output_type(gds, builder, &command.output_type)?;

    let mut arguments = HashMap::new();
    for (argument_name, argument_type) in &command.arguments {
        let field_name = ast::Name::new(argument_name.0.as_str())?;
        let input_type = types::input_type::get_input_type(gds, builder, argument_type)?;
        let input_field: Namespaced<GDS, InputField<GDS>> = builder.allow_all_namespaced(
            gql_schema::InputField::new(
                field_name.clone(),
                None,
                Annotation::Input(types::InputAnnotation::CommandArgument {
                    argument_type: argument_type.clone(),
                    ndc_func_proc_argument: command
                        .source
                        .as_ref()
                        .and_then(|command_source| {
                            command_source.argument_mappings.get(argument_name)
                        })
                        .cloned(),
                }),
                input_type,
                None,
                gql_schema::DeprecationStatus::NotDeprecated,
            ),
            None,
        );
        arguments.insert(field_name, input_field);
    }

    let field = builder.conditional_namespaced(
        gql_schema::Field::new(
            command_field_name.clone(),
            None,
            Annotation::Output(types::OutputAnnotation::RootField(
                types::RootFieldAnnotation::Command {
                    name: command.name.clone(),
                    source: command.source.clone(),
                    underlying_object_typename: command.underlying_object_typename.clone(),
                },
            )),
            output_typename,
            arguments,
            gql_schema::DeprecationStatus::NotDeprecated,
        ),
        permissions::get_command_namespace_annotations(command),
    );
    Ok((command_field_name, field))
}

/// Generates the IR for a 'command' operation
#[allow(irrefutable_let_patterns)]
pub(crate) fn command_generate_ir<'s>(
    command_name: &subgraph::Qualified<commands::CommandName>,
    field: &normalized_ast::Field<'s, GDS>,
    field_call: &normalized_ast::FieldCall<'s, GDS>,
    underlying_object_typename: &Option<subgraph::Qualified<open_dds::types::CustomTypeName>>,
    command_source: &'s resolved::command::CommandSource,
    session_variables: &SessionVariables,
) -> Result<CommandRepresentation<'s>, Error> {
    let empty_field_mappings = BTreeMap::new();
    // No field mappings should exists if the resolved output type of command is
    // not a custom object type
    let field_mappings = match underlying_object_typename {
        None => &empty_field_mappings,
        Some(typename) => command_source
            .type_mappings
            .get(typename)
            .and_then(|type_mapping| {
                if let resolved::types::TypeMapping::Object { field_mappings } = type_mapping {
                    Some(field_mappings)
                } else {
                    None
                }
            })
            .ok_or_else(|| InternalEngineError::InternalGeneric {
                description: format!(
                    "type '{}' not found in command source type_mappings",
                    typename
                ),
            })?,
    };

    let mut command_arguments = BTreeMap::new();
    for argument in field_call.arguments.values() {
        command_arguments.extend(
            command_arguments::build_ndc_command_arguments(
                &field_call.name,
                argument,
                &command_source.type_mappings,
            )?
            .into_iter(),
        );
    }

    // Add the name of the root command
    let mut usage_counts = UsagesCounts::new();
    count_command(command_name.clone(), &mut usage_counts);

    let selection = types::output_type::generate_selection_set_ir(
        &field.selection_set,
        &command_source.data_connector,
        &command_source.type_mappings,
        field_mappings,
        session_variables,
        &mut usage_counts,
    )?;

    Ok(CommandRepresentation {
        command_name: command_name.clone(),
        field_name: field_call.name.clone(),
        data_connector: command_source.data_connector.clone(),
        ndc_source: command_source.source.clone(),
        arguments: command_arguments,
        selection,
        type_container: field.type_container.clone(),
        // selection_set: &field.selection_set,
        usage_counts,
    })
}

pub fn ir_to_ndc_query_ir<'s>(
    function_name: &String,
    ir: &CommandRepresentation<'s>,
    join_id_counter: &mut MonotonicCounter,
) -> Result<(gdc::models::QueryRequest, JoinLocations<RemoteJoin<'s>>), Error> {
    let (ndc_fields, jl) = process_selection_set_ir(&ir.selection, join_id_counter)?;
    let query = gdc::models::Query {
        aggregates: None,
        fields: Some(ndc_fields),
        limit: None,
        offset: None,
        order_by: None,
        predicate: None,
    };
    let mut collection_relationships = BTreeMap::new();
    collect_relationships(&ir.selection, &mut collection_relationships)?;
    let arguments: BTreeMap<String, gdc::models::Argument> = ir
        .arguments
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                gdc::models::Argument::Literal { value: v.clone() },
            )
        })
        .collect();
    let query_request = gdc::models::QueryRequest {
        query,
        collection: function_name.to_string(),
        arguments,
        collection_relationships,
        variables: None,
    };
    Ok((query_request, jl))
}

pub fn ir_to_ndc_mutation_ir<'s>(
    procedure_name: &String,
    ir: &CommandRepresentation<'s>,
    join_id_counter: &mut MonotonicCounter,
) -> Result<(gdc::models::MutationRequest, JoinLocations<RemoteJoin<'s>>), Error> {
    let arguments = ir
        .arguments
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<BTreeMap<String, serde_json::Value>>();

    let (ndc_fields, jl) = process_selection_set_ir(&ir.selection, join_id_counter)?;
    let mutation_operation = gdc::models::MutationOperation::Procedure {
        name: procedure_name.to_string(),
        arguments,
        fields: Some(ndc_fields),
    };
    let mut collection_relationships = BTreeMap::new();
    collect_relationships(&ir.selection, &mut collection_relationships)?;
    let mutation_request = gdc::models::MutationRequest {
        operations: vec![mutation_operation],
        collection_relationships,
    };
    Ok((mutation_request, jl))
}
