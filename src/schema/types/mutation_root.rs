//! Schema of the query root type

use hasura_authn_core::SessionVariables;
use indexmap::IndexMap;
use lang_graphql as gql;
use lang_graphql::ast::common as ast;
use lang_graphql::schema as gql_schema;
// use tracing_util::SpanVisibility;

use std::collections::HashMap;

use crate::schema::operations::{commands, Error, InternalDeveloperError, InternalEngineError};
use crate::schema::types::root_field;
use crate::schema::types::Annotation;
use crate::schema::{mk_typename, GDS};

use super::{OutputAnnotation, RootFieldAnnotation};

/// Generates schema for the query root type
pub fn mutation_root_schema(
    builder: &mut gql_schema::Builder<GDS>,
    gds: &GDS,
) -> Result<gql_schema::Object<GDS>, crate::schema::Error> {
    let type_name = mk_typename("Mutation")?;
    let mut fields = HashMap::new();

    // Add node field for only the commands which have a mutation root field
    // defined, that is, they are based on procedures.
    for command in gds.metadata.commands.values() {
        if let Some(command_graphql_api) = &command.graphql_api {
            if matches!(
                command_graphql_api.root_field_kind,
                open_dds::commands::GraphQlRootFieldKind::Mutation
            ) {
                let command_field_name: ast::Name = command_graphql_api.root_field_name.clone();
                let (field_name, field) =
                    commands::command_field(gds, builder, command, command_field_name)?;
                fields.insert(field_name, field);
            }
        }
    }

    Ok(gql_schema::Object::new(
        builder,
        type_name,
        None,
        fields,
        HashMap::new(),
    ))
}

/// Generates IR for the selection set of type 'mutation root'
pub fn generate_ir<'n, 's>(
    selection_set: &'s gql::normalized_ast::SelectionSet<'s, GDS>,
    session_variables: &SessionVariables,
) -> Result<IndexMap<ast::Alias, root_field::RootField<'n, 's>>, Error> {
    let type_name = selection_set
        .type_name
        .clone()
        .ok_or_else(|| gql::normalized_ast::Error::NoTypenameFound)?;
    let mut root_fields = IndexMap::new();
    for (alias, field) in &selection_set.fields {
        let field_call = field.field_call()?;
        let field_response = match field_call.name.as_str() {
            "__typename" => Ok(root_field::MutationRootField::TypeName {
                type_name: type_name.clone(),
            }),
            _ => match field_call.info.generic {
                Annotation::Output(OutputAnnotation::RootField(RootFieldAnnotation::Command {
                    name,
                    underlying_object_typename,
                    source,
                })) => {
                    let source = source.as_ref().ok_or_else(|| {
                        InternalDeveloperError::NoSourceDataConnector {
                            type_name: type_name.clone(),
                            field_name: field_call.name.clone(),
                        }
                    })?;
                    Ok(root_field::MutationRootField::CommandRepresentation {
                        selection_set: &field.selection_set,
                        ir: commands::command_generate_ir(
                            name,
                            field,
                            field_call,
                            underlying_object_typename,
                            source,
                            session_variables,
                        )?,
                    })
                }
                annotation => Err(InternalEngineError::UnexpectedAnnotation {
                    annotation: annotation.clone(),
                }),
            },
        }?;
        root_fields.insert(
            alias.clone(),
            root_field::RootField::MutationRootField(field_response),
        );
    }
    Ok(root_fields)
}
