//! Schema of the query root type

use hasura_authn_core::{Session, SessionVariables};
use indexmap::IndexMap;
use lang_graphql as gql;
use lang_graphql::ast::common as ast;
use lang_graphql::schema as gql_schema;
use open_dds::{
    commands::{CommandName, GraphQlRootFieldKind},
    models,
    types::CustomTypeName,
};

use std::collections::HashMap;

use super::{Annotation, OutputAnnotation, RootFieldAnnotation};
use crate::schema::operations::relay::relay_node_ir;
use crate::schema::operations::{
    commands, relay, select_many, select_one, Error, InternalDeveloperError, InternalEngineError,
};
use crate::schema::types::root_field;
use crate::schema::types::RootFieldKind;
use crate::schema::{mk_typename, GDS};
use crate::{
    metadata::resolved::{self, subgraph},
    schema::operations::relay::RelayNodeFieldOutput,
};

/// Generates schema for the query root type
pub fn query_root_schema(
    builder: &mut gql_schema::Builder<GDS>,
    gds: &GDS,
) -> Result<gql_schema::Object<GDS>, crate::schema::Error> {
    let type_name = mk_typename("Query")?;
    let mut fields = HashMap::new();
    for model in gds.metadata.models.values() {
        for select_unique in model.graphql_api.select_uniques.iter() {
            let (field_name, field) =
                select_one::select_one_field(gds, builder, model, select_unique, &type_name)?;
            fields.insert(field_name, field);
        }
        for select_many in model.graphql_api.select_many.iter() {
            let (field_name, field) =
                select_many::select_many_field(gds, builder, model, select_many, &type_name)?;
            fields.insert(field_name, field);
        }
    }

    // Add node field for only the commands which have a query root field
    // defined, that is, they are based on functions.
    for command in gds.metadata.commands.values() {
        if let Some(command_graphql_api) = &command.graphql_api {
            if matches!(
                command_graphql_api.root_field_kind,
                GraphQlRootFieldKind::Query
            ) {
                let command_field_name = command_graphql_api.root_field_name.clone();
                let (field_name, field) =
                    commands::command_field(gds, builder, command, command_field_name)?;
                fields.insert(field_name, field);
            }
        }
    }

    let RelayNodeFieldOutput {
        relay_node_gql_field: node_field,
        relay_node_permissions: roles_implementing_node_interface,
    } = relay::relay_node_field(gds, builder)?;
    if fields
        .insert(
            node_field.name.clone(),
            // Instead of allowing all, here we should conditionally
            // allow roles whose atleast one object implement the
            // global ID.
            builder.conditional_namespaced(node_field.clone(), roles_implementing_node_interface),
        )
        .is_some()
    {
        return Err(
            crate::schema::Error::DuplicateFieldNameGeneratedInObjectType {
                field_name: node_field.name,
                type_name: subgraph::Qualified::new(
                    "-".to_string(),
                    CustomTypeName("Query".to_string()),
                ),
            },
        );
    };
    Ok(gql_schema::Object::new(
        builder,
        type_name,
        None,
        fields,
        HashMap::new(),
    ))
}

/// Generates IR for the selection set of type 'query root'
pub fn generate_ir<'n, 's>(
    schema: &'s gql::schema::Schema<GDS>,
    session: &Session,
    selection_set: &'s gql::normalized_ast::SelectionSet<'s, GDS>,
) -> Result<IndexMap<ast::Alias, root_field::RootField<'n, 's>>, Error> {
    let type_name = selection_set
        .type_name
        .clone()
        .ok_or_else(|| gql::normalized_ast::Error::NoTypenameFound)?;
    let mut ir = IndexMap::new();
    for (alias, field) in selection_set.fields.iter() {
        let field_call = field.field_call()?;
        let field_ir = match field_call.name.as_str() {
            "__typename" => Ok(root_field::QueryRootField::TypeName {
                type_name: type_name.clone(),
            }),
            "__schema" => Ok(root_field::QueryRootField::SchemaField {
                role: session.role.clone(),
                selection_set: &field.selection_set,
                schema,
            }),
            "__type" => {
                let ir = generate_type_field_ir(schema, &field.selection_set, field_call, session)?;
                Ok(ir)
            }
            _ => match field_call.info.generic {
                annotation @ Annotation::Output(field_annotation) => match field_annotation {
                    OutputAnnotation::RootField(root_field) => match root_field {
                        RootFieldAnnotation::Model {
                            data_type,
                            source,
                            kind,
                            name: model_name,
                        } => {
                            let ir = generate_model_rootfield_ir(
                                &type_name, source, data_type, kind, field, field_call, session,
                                model_name,
                            )?;
                            Ok(ir)
                        }
                        RootFieldAnnotation::Command {
                            name,
                            underlying_object_typename,
                            source,
                        } => {
                            let ir = generate_command_rootfield_ir(
                                name,
                                &type_name,
                                source,
                                underlying_object_typename,
                                field,
                                field_call,
                                &session.variables,
                            )?;
                            Ok(ir)
                        }
                        RootFieldAnnotation::RelayNode { typename_mappings } => {
                            let ir = generate_nodefield_ir(
                                field,
                                field_call,
                                typename_mappings,
                                session,
                            )?;
                            Ok(ir)
                        }
                        _ => Err(Error::from(InternalEngineError::UnexpectedAnnotation {
                            annotation: annotation.clone(),
                        })),
                    },
                    _ => Err(Error::from(InternalEngineError::UnexpectedAnnotation {
                        annotation: annotation.clone(),
                    })),
                },
                annotation => Err(Error::from(InternalEngineError::UnexpectedAnnotation {
                    annotation: annotation.clone(),
                })),
            },
        }?;
        ir.insert(
            alias.clone(),
            root_field::RootField::QueryRootField(field_ir),
        );
    }
    Ok(ir)
}

pub fn generate_type_field_ir<'n, 's>(
    schema: &'s gql::schema::Schema<GDS>,
    selection_set: &'s gql::normalized_ast::SelectionSet<GDS>,
    field_call: &gql::normalized_ast::FieldCall<GDS>,
    session: &Session,
) -> Result<root_field::QueryRootField<'n, 's>, Error> {
    let name = field_call
        .expected_argument(&lang_graphql::mk_name!("name"))?
        .value
        .as_string()?;
    let type_name = mk_typename(name).map_err(|_e| Error::TypeFieldInvalidGraphQlName {
        name: name.to_string(),
    })?;
    Ok(root_field::QueryRootField::TypeField {
        role: session.role.clone(),
        selection_set,
        schema,
        type_name,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn generate_model_rootfield_ir<'n, 's>(
    type_name: &ast::TypeName,
    source: &'s Option<resolved::model::ModelSource>,
    data_type: &subgraph::Qualified<CustomTypeName>,
    kind: &RootFieldKind,
    field: &'n gql::normalized_ast::Field<'s, GDS>,
    field_call: &'s gql::normalized_ast::FieldCall<'s, GDS>,
    session: &Session,
    model_name: &'s subgraph::Qualified<models::ModelName>,
) -> Result<root_field::QueryRootField<'n, 's>, Error> {
    let source = source
        .as_ref()
        .ok_or_else(|| InternalDeveloperError::NoSourceDataConnector {
            type_name: type_name.clone(),
            field_name: field_call.name.clone(),
        })?;
    let ir = match kind {
        RootFieldKind::SelectOne => root_field::QueryRootField::ModelSelectOne {
            selection_set: &field.selection_set,
            ir: select_one::select_one_generate_ir(
                field,
                field_call,
                data_type,
                source,
                &session.variables,
                model_name,
            )?,
        },
        RootFieldKind::SelectMany => root_field::QueryRootField::ModelSelectMany {
            selection_set: &field.selection_set,
            ir: select_many::select_many_generate_ir(
                field,
                field_call,
                data_type,
                source,
                &session.variables,
                model_name,
            )?,
        },
    };
    Ok(ir)
}

pub fn generate_command_rootfield_ir<'n, 's>(
    name: &'s subgraph::Qualified<CommandName>,
    type_name: &ast::TypeName,
    source: &'s Option<resolved::command::CommandSource>,
    underlying_object_typename: &'s Option<subgraph::Qualified<CustomTypeName>>,
    field: &'n gql::normalized_ast::Field<'s, GDS>,
    field_call: &'s gql::normalized_ast::FieldCall<'s, GDS>,
    session_variables: &SessionVariables,
) -> Result<root_field::QueryRootField<'n, 's>, Error> {
    let source = source
        .as_ref()
        .ok_or_else(|| InternalDeveloperError::NoSourceDataConnector {
            type_name: type_name.clone(),
            field_name: field_call.name.clone(),
        })?;
    let ir = root_field::QueryRootField::CommandRepresentation {
        selection_set: &field.selection_set,
        ir: commands::command_generate_ir(
            name,
            field,
            field_call,
            underlying_object_typename,
            source,
            session_variables,
        )?,
    };
    Ok(ir)
}

pub fn generate_nodefield_ir<'n, 's>(
    field: &'n gql::normalized_ast::Field<'s, GDS>,
    field_call: &'n gql::normalized_ast::FieldCall<'s, GDS>,
    typename_mappings: &'s HashMap<ast::TypeName, super::NodeFieldTypeNameMapping>,
    session: &Session,
) -> Result<root_field::QueryRootField<'n, 's>, Error> {
    let ir = root_field::QueryRootField::NodeSelect(relay_node_ir(
        field,
        field_call,
        typename_mappings,
        &session.role,
        &session.variables,
    )?);
    Ok(ir)
}
