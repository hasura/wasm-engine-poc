//! Schema of the relay according to https://relay.dev/graphql/objectidentification.htm

use crate::metadata::resolved;
use crate::schema::model_tracking::UsagesCounts;
use crate::schema::operations::model_selection::model_selection_ir;
use crate::schema::types::{self, GlobalID};
use crate::schema::types::{output_type::node_interface_type, Annotation};
use crate::schema::{mk_typename, Role, GDS};
use base64::{engine::general_purpose, Engine};
use hasura_authn_core::SessionVariables;
use lang_graphql::{ast::common as ast, normalized_ast, schema as gql_schema};
use open_dds::ndc_client as ndc;
use open_dds::types::FieldName;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

use crate::schema::operations;
use crate::schema::types::{
    input_type::get_input_type,
    output_type::{
        get_custom_output_type, get_object_type_representation, get_output_type, ID_TYPE_REFERENCE,
    },
    NodeFieldTypeNameMapping, OutputAnnotation, RootFieldAnnotation,
};

use super::permissions;

/// IR for the 'select_one' operation on a model
#[derive(Serialize, Debug)]
pub struct NodeSelect<'n, 's> {
    // The name of the field as published in the schema
    pub field_name: &'n ast::Name,

    /// Model Selection IR fragment
    pub model_selection: operations::model_selection::ModelSelection<'s>,

    // We need this to post process the response for `__typename` fields and for
    // validating the response from the data connector. This is not a reference
    // as it is constructed from the original selection set by filtering fields
    // that are relevant.
    pub selection_set: normalized_ast::SelectionSet<'s, GDS>,

    // All the models/commands used in this operation. This includes the models/commands
    // used via relationships. And in future, the models/commands used in the filter clause
    pub(crate) usage_counts: UsagesCounts,
}

/// Generate the NDC IR for the node root field.

/// This function, decodes the value of the `id`
/// argument and then looks the `typename` up in the
/// `typename_mappings`. A successful lookup will yield the
/// `data_specification::TypeName` and the `ModelSource`
/// associated with the typename and a Hashset of roles that
/// can access the Object coresponding to the type name.
/// If the role, doesn't have model select permissions
/// to the model that is the global ID source for the
/// object type that was decoded, then this function
/// returns `None`.
pub(crate) fn relay_node_ir<'n, 's>(
    field: &'n normalized_ast::Field<'s, GDS>,
    field_call: &'n normalized_ast::FieldCall<'s, GDS>,
    typename_mappings: &'s HashMap<ast::TypeName, NodeFieldTypeNameMapping>,
    role: &Role,
    session_variables: &SessionVariables,
) -> Result<Option<NodeSelect<'n, 's>>, operations::Error> {
    let id_arg_value = field_call
        .expected_argument(&lang_graphql::mk_name!("id"))?
        .value
        .as_id()?;
    let decoded_id_value = general_purpose::STANDARD
        .decode(id_arg_value.clone())
        .map_err(|e| operations::Error::ErrorInDecodingGlobalId {
            encoded_value: id_arg_value.clone(),
            decoding_error: e.to_string(),
        })?;
    let global_id: GlobalID = serde_json::from_slice(decoded_id_value.as_slice())?;
    let typename_mapping = typename_mappings.get(&global_id.typename).ok_or(
        operations::InternalDeveloperError::GlobalIDTypenameMappingNotFound {
            type_name: global_id.typename.clone(),
        },
    )?;
    let role_model_select_permission = typename_mapping.model_select_permissions.get(role);
    match role_model_select_permission {
        // When a role doesn't have any model select permissions on the model
        // that is the Global ID source for the object type, we just return `null`.
        None => Ok(None),
        Some(role_model_select_permission) => {
            let model_source = typename_mapping.model_source.as_ref().ok_or(
                operations::InternalDeveloperError::NoSourceDataConnector {
                    type_name: global_id.typename.clone(),
                    field_name: lang_graphql::mk_name!("node"),
                },
            )?;

            let field_mappings = model_source
                .type_mappings
                .get(&typename_mapping.type_name)
                .map(|type_mapping| match type_mapping {
                    resolved::types::TypeMapping::Object { field_mappings } => field_mappings,
                })
                .ok_or_else(|| operations::InternalEngineError::InternalGeneric {
                    description: format!(
                        "type '{}' not found in model source type_mappings",
                        typename_mapping.type_name
                    ),
                })?;
            let filter_clauses = global_id
                .id
                .iter()
                .map(|(field_name, val)| {
                    let field_mapping = &field_mappings.get(field_name).ok_or_else(|| {
                        operations::InternalEngineError::InternalGeneric {
                            description: format!("invalid field in annotation: {field_name:}"),
                        }
                    })?;
                    Ok(ndc::models::Expression::BinaryComparisonOperator {
                        column: ndc::models::ComparisonTarget::Column {
                            name: field_mapping.column.clone(),
                            path: vec![],
                        },
                        operator: ndc::models::BinaryComparisonOperator::Equal,
                        value: ndc::models::ComparisonValue::Scalar { value: val.clone() },
                    })
                })
                .collect::<Result<_, operations::Error>>()?;

            let new_selection_set = field
                .selection_set
                .filter_field_calls_by_typename(global_id.typename);

            let mut usage_counts = UsagesCounts::new();
            let model_selection = model_selection_ir(
                &new_selection_set,
                &typename_mapping.type_name,
                model_source,
                BTreeMap::new(),
                filter_clauses,
                &role_model_select_permission.filter,
                None, // limit
                None, // offset
                None, // order_by
                session_variables,
                // Get all the models/commands that were used as relationships
                &mut usage_counts,
            )?;
            Ok(Some(NodeSelect {
                field_name: &field_call.name,
                model_selection,
                selection_set: new_selection_set,
                usage_counts,
            }))
        }
    }
}

pub(crate) struct RelayNodeFieldOutput {
    pub relay_node_gql_field: gql_schema::Field<GDS>,
    /// Roles having access to the `node` field.
    pub relay_node_permissions: HashMap<Role, Option<types::NamespaceAnnotation>>,
}

/// Calculates the relay `node` field and also returns the
/// list of roles that have access to the `node` field,
/// for the `node` field to be accessible to a role, the role
/// needs to atleast have access to the global ID of any one
/// object that implements the Node interface.
pub(crate) fn relay_node_field(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
) -> Result<RelayNodeFieldOutput, crate::schema::Error> {
    let mut arguments = HashMap::new();
    let mut typename_mappings = HashMap::new();
    // The node field should only be accessible to a role, if
    // atleast one object implements the global `id` field.
    //
    // For example,
    //
    // Let's say the `user` role doesn't have access to the global
    // ID fields to the types it has access to.
    //
    // Then, the node interface generated would not have any
    // object types because the object types don't expose the
    // global `id` field. GraphQL interfaces expect that the
    // objects implementing the interface define all the fields
    // defined by the interface.

    let mut roles_implementing_global_id: HashMap<Role, Option<types::NamespaceAnnotation>> =
        HashMap::new();
    for model in gds.metadata.models.values() {
        if model.global_id_source {
            let output_typename = get_custom_output_type(gds, builder, &model.data_type)?;

            let object_type_representation = get_object_type_representation(gds, &model.data_type)?;

            let node_interface_annotations =
                permissions::get_node_interface_annotations(object_type_representation);

            for role in node_interface_annotations.keys() {
                roles_implementing_global_id.insert(role.clone(), None);
            }

            if typename_mappings
                .insert(
                    output_typename.type_name().clone(),
                    NodeFieldTypeNameMapping {
                        type_name: model.data_type.clone(),
                        model_source: model.source.clone(),
                        model_select_permissions: model
                            .select_permissions
                            .clone()
                            .unwrap_or(HashMap::new()),
                    },
                )
                .is_some()
            {
                // This is declared as an internal error because this error should
                // never happen, because this is validated while resolving the metadata.
                return Err(
                    crate::schema::Error::InternalErrorDuplicateGlobalIdSourceFound {
                        type_name: output_typename.type_name().clone(),
                    },
                );
            };
        }
    }
    let id_argument: gql_schema::InputField<GDS> = gql_schema::InputField::new(
        lang_graphql::mk_name!("id"),
        None,
        Annotation::Output(types::OutputAnnotation::Field {
            name: FieldName("id".to_string()),
        }),
        get_input_type(gds, builder, &ID_TYPE_REFERENCE)?,
        None,
        gql_schema::DeprecationStatus::NotDeprecated,
    );
    arguments.insert(
        id_argument.name.clone(),
        builder.allow_all_namespaced(id_argument, None),
    );
    let relay_node_gql_field = gql_schema::Field::new(
        lang_graphql::mk_name!("node"),
        None,
        Annotation::Output(OutputAnnotation::RootField(
            RootFieldAnnotation::RelayNode { typename_mappings },
        )),
        ast::TypeContainer::named_null(node_interface_type(builder)),
        arguments,
        gql_schema::DeprecationStatus::NotDeprecated,
    );
    Ok(RelayNodeFieldOutput {
        relay_node_gql_field,
        relay_node_permissions: roles_implementing_global_id,
    })
}

pub fn node_interface_schema(
    builder: &mut gql_schema::Builder<GDS>,
    gds: &GDS,
) -> Result<gql_schema::Interface<GDS>, crate::schema::Error> {
    let mut fields = HashMap::new();
    let mut implemented_by = HashMap::new();
    let mut typename_global_id_mappings = HashMap::new();
    let mut roles_implementing_global_id: HashMap<Role, Option<types::NamespaceAnnotation>> =
        HashMap::new();
    for model in gds.metadata.models.values() {
        if model.global_id_source {
            let object_type_representation = get_object_type_representation(gds, &model.data_type)?;

            let object_typename = get_custom_output_type(gds, builder, &model.data_type)?;

            let node_interface_annotations =
                permissions::get_node_interface_annotations(object_type_representation);

            for role in node_interface_annotations.keys() {
                roles_implementing_global_id.insert(role.clone(), None);
            }

            implemented_by.insert(
                object_typename.clone(),
                builder.conditional_namespaced((), node_interface_annotations),
            );

            // Multiple models can be backed by the same type
            typename_global_id_mappings.insert(
                object_typename.type_name().clone(),
                model.global_id_fields.clone(),
            );
        }
    }
    let node_id_field = gql_schema::Field::new(
        lang_graphql::mk_name!("id"),
        None,
        Annotation::Output(OutputAnnotation::RelayNodeInterfaceID {
            typename_mappings: typename_global_id_mappings,
        }),
        get_output_type(gds, builder, &ID_TYPE_REFERENCE)?,
        HashMap::new(),
        gql_schema::DeprecationStatus::NotDeprecated,
    );
    fields.insert(
        node_id_field.name.clone(),
        builder.conditional_namespaced(node_id_field, roles_implementing_global_id),
    );
    let node_typename = mk_typename("Node")?;
    Ok(gql_schema::Interface::new(
        builder,
        node_typename,
        None,
        fields,
        HashMap::new(),
        implemented_by,
    ))
}
