use std::collections::{BTreeMap, HashMap};

use crate::metadata::resolved::subgraph::{
    Qualified, QualifiedBaseType, QualifiedTypeName, QualifiedTypeReference,
};

use crate::metadata::resolved::types::ObjectTypeRepresentation;
use crate::metadata::resolved::{
    self,
    types::{mk_name, TypeRepresentation},
};
use crate::schema::model_tracking::UsagesCounts;
use crate::schema::operations::model_selection::{self, ModelSelection};
use crate::schema::operations::remote_joins::{JoinLocations, MonotonicCounter};
use crate::schema::operations::remote_joins::{Location, RemoteJoin};
use crate::schema::operations::select_many::generate_select_many_arguments;
use crate::schema::operations::{permissions, InternalEngineError};
use crate::schema::{operations, Role, GDS};
use hasura_authn_core::SessionVariables;
use indexmap::IndexMap;
use lang_graphql::ast::common::{self as ast, Alias};
use lang_graphql::{normalized_ast, schema as gql_schema};
use open_dds::ndc_client as ndc;
use open_dds::{
    relationships,
    types::{CustomTypeName, FieldName, InbuiltType},
};
use serde::Serialize;

use self::relationship::{
    ModelRelationshipAnnotation, ModelTargetSource, RelationshipInfo, RemoteRelationshipInfo,
};

use super::inbuilt_type::base_type_container_for_inbuilt_type;
use super::{global_id_col_format, Annotation, OutputAnnotation, RootFieldAnnotation};

type Error = crate::schema::Error;

pub mod relationship;

pub(crate) const ID_TYPE_REFERENCE: QualifiedTypeReference = QualifiedTypeReference {
    underlying_type: QualifiedBaseType::Named(QualifiedTypeName::Inbuilt(InbuiltType::ID)),
    nullable: false,
};

pub fn get_base_type_container(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    type_name: &QualifiedTypeName,
) -> Result<ast::BaseTypeContainer<gql_schema::RegisteredTypeName>, Error> {
    match type_name {
        QualifiedTypeName::Inbuilt(inbuilt_type) => {
            Ok(base_type_container_for_inbuilt_type(inbuilt_type))
        }
        QualifiedTypeName::Custom(type_name) => Ok(ast::BaseTypeContainer::Named(
            get_custom_output_type(gds, builder, type_name)?,
        )),
    }
}

pub fn get_output_type(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    gds_type: &QualifiedTypeReference,
) -> Result<ast::TypeContainer<gql_schema::RegisteredTypeName>, Error> {
    if gds_type.nullable {
        match &gds_type.underlying_type {
            QualifiedBaseType::Named(type_name) => {
                let base = get_base_type_container(gds, builder, type_name)?;
                Ok(ast::TypeContainer {
                    base,
                    nullable: true,
                })
            }
            QualifiedBaseType::List(list_type) => {
                let output_type = get_output_type(gds, builder, list_type)?;
                Ok(ast::TypeContainer::list_null(output_type))
            }
        }
    } else {
        match &gds_type.underlying_type {
            QualifiedBaseType::Named(type_name) => {
                let base = get_base_type_container(gds, builder, type_name)?;
                Ok(ast::TypeContainer {
                    base,
                    nullable: false,
                })
            }
            QualifiedBaseType::List(list_type) => {
                let output_type = get_output_type(gds, builder, list_type)?;
                Ok(ast::TypeContainer::list_non_null(output_type))
            }
        }
    }
}

pub fn node_interface_type(
    builder: &mut gql_schema::Builder<GDS>,
) -> gql_schema::RegisteredTypeName {
    builder.register_type(super::TypeId::NodeRoot)
}

pub fn get_custom_output_type(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    gds_type: &Qualified<CustomTypeName>,
) -> Result<gql_schema::RegisteredTypeName, Error> {
    let type_representation = gds.metadata.types.get(gds_type).ok_or_else(|| {
        crate::schema::Error::InternalTypeNotFound {
            type_name: gds_type.clone(),
        }
    })?;
    match type_representation {
        TypeRepresentation::Object(object_type_representation) => {
            Ok(builder.register_type(super::TypeId::OutputType {
                gds_type_name: gds_type.clone(),
                graphql_type_name: object_type_representation
                    .graphql_output_type_name
                    .as_ref()
                    .ok_or_else(|| Error::NoGraphQlOutputTypeNameForObject {
                        type_name: gds_type.clone(),
                    })?
                    .clone(),
            }))
        }
        TypeRepresentation::ScalarType { graphql_type_name } => {
            Ok(builder.register_type(super::TypeId::ScalarType {
                gds_type_name: gds_type.clone(),
                graphql_type_name: graphql_type_name
                    .as_ref()
                    .ok_or_else(|| Error::NoGraphQlTypeNameForScalar {
                        type_name: gds_type.clone(),
                    })?
                    .clone(),
            }))
        }
    }
}

/// generate graphql schema for object type fields
fn object_type_fields(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    object_type_representation: &ObjectTypeRepresentation,
) -> Result<HashMap<ast::Name, gql_schema::Namespaced<GDS, gql_schema::Field<GDS>>>, Error> {
    let mut graphql_fields = object_type_representation
        .fields
        .iter()
        .map(|(field_name, field_definition)| -> Result<_, Error> {
            let graphql_field_name = mk_name(field_name.0.as_str())?;
            let field = gql_schema::Field::<GDS>::new(
                graphql_field_name.clone(),
                None,
                Annotation::Output(super::OutputAnnotation::Field {
                    name: field_name.clone(),
                }),
                get_output_type(gds, builder, &field_definition.field_type)?,
                HashMap::new(),
                gql_schema::DeprecationStatus::NotDeprecated,
            );
            // if output permissions are defined for this type, we conditionally
            // include fields
            let namespaced_field = {
                let mut role_map = HashMap::new();
                for (role, perms) in &object_type_representation.type_permissions {
                    if perms.allowed_fields.contains(field_name) {
                        role_map.insert(Role(role.0.clone()), None);
                    }
                }
                builder.conditional_namespaced(field, role_map)
            };
            Ok((graphql_field_name, namespaced_field))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    let graphql_relationship_fields = object_type_representation
        .relationships
        .iter()
        .map(
            |(relationship_field_name, relationship)| -> Result<_, Error> {
                let graphql_field_name = relationship_field_name.clone();
                let relationship_base_output_type =
                    get_custom_output_type(gds, builder, &relationship.target_typename)?;

                // TODO: Replace with if let when we support relationships to commands.
                let resolved::relationship::RelationshipTarget::Model {
                    model_name,
                    relationship_type,
                } = &relationship.target;
                let relationship_output_type = match relationship_type {
                    relationships::RelationshipType::Array => {
                        let non_nullable_relationship_base_type =
                            ast::TypeContainer::named_non_null(relationship_base_output_type);
                        ast::TypeContainer::list_null(non_nullable_relationship_base_type)
                    }
                    relationships::RelationshipType::Object => {
                        ast::TypeContainer::named_null(relationship_base_output_type)
                    }
                };

                let model = gds.metadata.models.get(model_name).ok_or_else(|| {
                    Error::InternalModelNotFound {
                        model_name: model_name.clone(),
                    }
                })?;
                if !model.arguments.is_empty() {
                    return Err(Error::InternalUnsupported {
                        summary: "Relationships to models with arguments aren't supported".into(),
                    });
                }

                let arguments = match relationship_type {
                    relationships::RelationshipType::Array => {
                        generate_select_many_arguments(builder, model)?
                    }
                    relationships::RelationshipType::Object => HashMap::new(),
                };

                let target_object_type_representation =
                    get_object_type_representation(gds, &model.data_type)?;

                let relationship_field = builder.conditional_namespaced(
                    gql_schema::Field::<GDS>::new(
                        graphql_field_name.clone(),
                        None,
                        Annotation::Output(super::OutputAnnotation::RelationshipToModel(
                            ModelRelationshipAnnotation {
                                source_type: relationship.source.clone(),
                                relationship_name: relationship.name.clone(),
                                model_name: model_name.clone(),
                                target_source: ModelTargetSource::new(model, relationship)?,
                                target_type: relationship.target_typename.clone(),
                                relationship_type: relationship_type.clone(),
                                mappings: relationship.mappings.clone(),
                            },
                        )),
                        relationship_output_type,
                        arguments,
                        gql_schema::DeprecationStatus::NotDeprecated,
                    ),
                    permissions::get_relationship_namespace_annotations(
                        model,
                        object_type_representation,
                        target_object_type_representation,
                        &relationship.mappings,
                    ),
                );
                Ok((graphql_field_name, relationship_field))
            },
        )
        .collect::<Result<HashMap<_, _>, _>>()?;
    graphql_fields.extend(graphql_relationship_fields);
    Ok(graphql_fields)
}

pub fn output_type_schema(
    gds: &GDS,
    builder: &mut gql_schema::Builder<GDS>,
    type_name: &Qualified<CustomTypeName>,
    graphql_type_name: &ast::TypeName,
) -> Result<gql_schema::TypeInfo<GDS>, Error> {
    let type_representation =
        gds.metadata
            .types
            .get(type_name)
            .ok_or_else(|| Error::InternalTypeNotFound {
                type_name: type_name.clone(),
            })?;

    let graphql_type_name = graphql_type_name.clone();

    match &type_representation {
        resolved::types::TypeRepresentation::Object(object_type_representation) => {
            let mut object_type_fields =
                object_type_fields(gds, builder, object_type_representation)?;
            if object_type_representation.global_id_fields.is_empty() {
                Ok(gql_schema::TypeInfo::Object(gql_schema::Object::new(
                    builder,
                    graphql_type_name,
                    None,
                    object_type_fields,
                    HashMap::new(),
                )))
            } else {
                // Generate the Global object `id` field and insert it
                // into the `object_type_fields`.
                let mut interfaces = HashMap::new();
                let global_id_field_name = lang_graphql::mk_name!("id");
                let global_id_field = gql_schema::Field::<GDS>::new(
                    global_id_field_name.clone(),
                    None,
                    Annotation::Output(super::OutputAnnotation::GlobalIDField {
                        global_id_fields: object_type_representation.global_id_fields.to_vec(),
                    }),
                    get_output_type(gds, builder, &ID_TYPE_REFERENCE)?,
                    HashMap::new(),
                    gql_schema::DeprecationStatus::NotDeprecated,
                );
                if object_type_fields
                    .insert(
                        global_id_field_name.clone(),
                        builder.conditional_namespaced(
                            global_id_field,
                            permissions::get_node_interface_annotations(object_type_representation),
                        ),
                    )
                    .is_some()
                {
                    return Err(Error::DuplicateFieldNameGeneratedInObjectType {
                        field_name: global_id_field_name,
                        type_name: type_name.clone(),
                    });
                }
                interfaces.insert(
                    node_interface_type(builder),
                    builder.conditional_namespaced(
                        (),
                        permissions::get_node_interface_annotations(object_type_representation),
                    ),
                );
                Ok(gql_schema::TypeInfo::Object(gql_schema::Object::new(
                    builder,
                    graphql_type_name,
                    None,
                    object_type_fields,
                    interfaces,
                )))
            }
        }
        resolved::types::TypeRepresentation::ScalarType { .. } => Err(Error::InternalUnsupported {
            summary: format!(
                "a scalar type {} mapping to non-scalar GraphQL types",
                type_name.clone()
            ),
        }),
    }
}

#[derive(Debug, Serialize)]
pub(crate) enum FieldSelection<'s> {
    Column {
        column: String,
    },
    LocalRelationship {
        query: ModelSelection<'s>,
        /// Relationship names needs to be unique across the IR. This field contains
        /// the uniquely generated relationship name. `ModelRelationshipAnnotation`
        /// contains a relationship name but that is the name from the metadata.
        name: String,
        relationship_info: RelationshipInfo<'s>,
    },
    RemoteRelationship {
        ir: ModelSelection<'s>,
        relationship_info: RemoteRelationshipInfo<'s>,
    },
}

/// IR that represents the selected fields of an output type.
#[derive(Debug, Serialize)]
pub(crate) struct ResultSelectionSet<'s> {
    // The fields in the selection set. They are stored in the form that would
    // be converted and sent over the wire. Serialized the map as ordered to
    // produce deterministic golden files.
    pub(crate) fields: IndexMap<String, FieldSelection<'s>>,
}

fn build_global_id_fields(
    global_id_fields: &Vec<FieldName>,
    field_mappings: &BTreeMap<FieldName, resolved::types::FieldMapping>,
    field_alias: &Alias,
    fields: &mut IndexMap<String, FieldSelection>,
) -> Result<(), operations::Error> {
    for field_name in global_id_fields {
        let field_mapping =
            field_mappings
                .get(field_name)
                .ok_or_else(|| InternalEngineError::InternalGeneric {
                    description: format!("invalid global id field in annotation: {field_name:}"),
                })?;
        // Prefix the global column id with something that will be unlikely to be chosen
        // by the user,
        //  to not have any conflicts with any of the fields
        // in the selection set.
        let global_col_id_alias = global_id_col_format(field_alias, field_name);

        fields.insert(
            global_col_id_alias,
            FieldSelection::Column {
                column: field_mapping.column.clone(),
            },
        );
    }
    Ok(())
}

/// Builds the IR from a normalized selection set
/// `field_mappings` is needed separately during IR generation and cannot be embedded
/// into the annotation itself because the same GraphQL type may have different field
/// sources depending on the model being queried.
pub(crate) fn generate_selection_set_ir<'s>(
    selection_set: &normalized_ast::SelectionSet<'s, GDS>,
    data_connector: &'s resolved::data_connector::DataConnector,
    type_mappings: &'s BTreeMap<Qualified<CustomTypeName>, resolved::types::TypeMapping>,
    field_mappings: &BTreeMap<FieldName, resolved::types::FieldMapping>,
    session_variables: &SessionVariables,
    usage_counts: &mut UsagesCounts,
) -> Result<ResultSelectionSet<'s>, operations::Error> {
    let mut fields = IndexMap::new();
    for field in selection_set.fields.values() {
        let field_call = field.field_call()?;
        match field_call.info.generic {
            annotation @ Annotation::Output(annotated_field) => match annotated_field {
                OutputAnnotation::Field { name, .. } => {
                    let field_mapping = &field_mappings.get(name).ok_or_else(|| {
                        InternalEngineError::InternalGeneric {
                            description: format!("invalid field in annotation: {name:}"),
                        }
                    })?;
                    fields.insert(
                        field.alias.to_string(),
                        FieldSelection::Column {
                            column: field_mapping.column.clone(),
                        },
                    );
                }
                OutputAnnotation::RootField(RootFieldAnnotation::Introspection) => {}
                OutputAnnotation::GlobalIDField { global_id_fields } => {
                    build_global_id_fields(
                        global_id_fields,
                        field_mappings,
                        &field.alias,
                        &mut fields,
                    )?;
                }
                OutputAnnotation::RelayNodeInterfaceID { typename_mappings } => {
                    // Even though we already have the value of the global ID field
                    // here, we try to re-compute the value of the same ID by decoding the ID.
                    // We do this because it simplifies the code structure.
                    // If the NDC were to accept key-value pairs from the v3-engine that will
                    // then be outputted as it is, then we could avoid this computation.
                    let type_name = field.selection_set.type_name.clone().ok_or(
                        InternalEngineError::InternalGeneric {
                            description: "typename not found while resolving NodeInterfaceId"
                                .to_string(),
                        },
                    )?;
                    let global_id_fields = typename_mappings.get(&type_name).ok_or(
                        InternalEngineError::InternalGeneric {
                            description: format!(
                                "Global ID fields not found of the type {}",
                                type_name
                            ),
                        },
                    )?;

                    build_global_id_fields(
                        global_id_fields,
                        field_mappings,
                        &field.alias,
                        &mut fields,
                    )?;
                }
                OutputAnnotation::RelationshipToModel(relationship_annotation) => {
                    fields.insert(
                        field.alias.to_string(),
                        relationship::generate_relationship_ir(
                            field,
                            relationship_annotation,
                            data_connector,
                            type_mappings,
                            session_variables,
                            usage_counts,
                        )?,
                    );
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
    Ok(ResultSelectionSet { fields })
}

/// Convert selection set IR (`ResultSelectionSet`) into NDC fields
pub(crate) fn process_selection_set_ir<'s>(
    model_selection: &ResultSelectionSet<'s>,
    join_id_counter: &mut MonotonicCounter,
) -> Result<
    (
        IndexMap<String, ndc::models::Field>,
        JoinLocations<RemoteJoin<'s>>,
    ),
    operations::Error,
> {
    let mut ndc_fields = IndexMap::new();
    let mut join_locations = JoinLocations::new();
    for (alias, field) in &model_selection.fields {
        match field {
            FieldSelection::Column { column } => {
                ndc_fields.insert(
                    alias.to_string(),
                    ndc::models::Field::Column {
                        column: column.clone(),
                    },
                );
            }
            FieldSelection::LocalRelationship {
                query,
                name,
                relationship_info: _,
            } => {
                let (relationship_query, jl) =
                    model_selection::ir_to_ndc_query(query, join_id_counter)?;
                let ndc_field = ndc::models::Field::Relationship {
                    query: Box::new(relationship_query),
                    relationship: name.to_string(),
                    arguments: BTreeMap::new(),
                };
                if !jl.locations.is_empty() {
                    join_locations.locations.insert(
                        alias.clone(),
                        Location {
                            join_node: None,
                            rest: jl,
                        },
                    );
                }
                ndc_fields.insert(alias.to_string(), ndc_field);
            }
            FieldSelection::RemoteRelationship {
                ir,
                relationship_info,
            } => {
                // For all the left join fields, create an alias and inject
                // them into the NDC IR
                let mut join_columns = HashMap::new();
                for ((src_field_alias, src_field), target_field) in &relationship_info.join_mapping
                {
                    let lhs_alias = make_hasura_phantom_field(&src_field.column);
                    ndc_fields.insert(
                        lhs_alias.clone(),
                        ndc::models::Field::Column {
                            column: src_field.column.clone(),
                        },
                    );
                    join_columns.insert(
                        src_field_alias.clone(),
                        (lhs_alias.clone(), target_field.clone()),
                    );
                }
                // Construct the `JoinLocations` tree
                let (ndc_ir, sub_join_locations) =
                    model_selection::ir_to_ndc_ir(ir, join_id_counter)?;
                let rj_info = RemoteJoin {
                    target_ndc_ir: ndc_ir,
                    target_data_connector: ir.data_connector,
                    join_columns,
                };
                join_locations.locations.insert(
                    alias.clone(),
                    Location {
                        join_node: Some(rj_info),
                        rest: sub_join_locations,
                    },
                );
            }
        };
    }
    Ok((ndc_fields, join_locations))
}

fn make_hasura_phantom_field(field_name: &str) -> String {
    format!("__hasura_phantom_field__{}", field_name)
}

/// From the fields in `ResultSelectionSet`, collect relationships recursively
/// and create NDC relationship definitions
pub(crate) fn collect_relationships(
    selection: &ResultSelectionSet,
    relationships: &mut BTreeMap<String, ndc::models::Relationship>,
) -> Result<(), operations::Error> {
    for field in selection.fields.values() {
        match field {
            FieldSelection::Column { .. } => (),
            FieldSelection::LocalRelationship {
                query,
                name,
                relationship_info,
            } => {
                relationships.insert(
                    name.to_string(),
                    relationship::process_relationship_definition(relationship_info)?,
                );
                collect_relationships(&query.selection, relationships)?;
            }
            // we ignore remote relationships as we are generating relationship
            // definition for one data connector
            FieldSelection::RemoteRelationship { .. } => (),
        };
    }
    Ok(())
}

/// Gets the `ObjectTypeRepresentation` of the type
/// identified with the `gds_type`, it will throw
/// an error if the type is not found to be an object.
pub(crate) fn get_object_type_representation<'s>(
    gds: &'s GDS,
    gds_type: &Qualified<CustomTypeName>,
) -> Result<&'s ObjectTypeRepresentation, crate::schema::Error> {
    let type_representation = gds.metadata.types.get(gds_type).ok_or_else(|| {
        crate::schema::Error::InternalTypeNotFound {
            type_name: gds_type.clone(),
        }
    })?;
    match type_representation {
        TypeRepresentation::Object(object_type_representation) => Ok(object_type_representation),
        TypeRepresentation::ScalarType { .. } => {
            Err(crate::schema::Error::ExpectedTypeToBeObject {
                type_name: gds_type.clone(),
            })
        }
    }
}
