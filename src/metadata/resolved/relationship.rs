use super::data_connector::DataConnectorContext;
use super::error::Error;
use super::model::Model;
use super::subgraph::Qualified;
use super::types::mk_name;
use super::types::ObjectTypeRepresentation;
use indexmap::IndexMap;
use lang_graphql::ast::common as ast;
use open_dds::data_connector::DataConnectorName;
use open_dds::models::ModelName;
use open_dds::relationships::{
    self, FieldAccess, RelationshipName, RelationshipType, RelationshipV1,
};
use open_dds::types::CustomTypeName;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum RelationshipTarget {
    Model {
        // TODO(Abhinav): Refactor resolved types to contain denormalized data (eg: actual resolved model)
        model_name: Qualified<ModelName>,
        relationship_type: RelationshipType,
    },
    // TODO: Add support for relationships with Commands.
    // Command {
    //     command: CommandName,
    // },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RelationshipMapping {
    pub source_field: FieldAccess,
    pub target_field: FieldAccess,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    pub name: RelationshipName,
    pub field_name: ast::Name,
    pub source: Qualified<CustomTypeName>,
    pub target: RelationshipTarget,
    pub target_typename: Qualified<CustomTypeName>,
    pub mappings: Vec<RelationshipMapping>,
    pub target_capabilities: Option<RelationshipCapabilities>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RelationshipCapabilities {
    // TODO: We don't handle relationships without foreach.
    // Change this to a bool, when we support that
    pub foreach: (),
    pub relationships: bool,
}

fn resolve_relationship_mappings(
    relationship: &RelationshipV1,
    source_type_name: &Qualified<CustomTypeName>,
    source_type: &ObjectTypeRepresentation,
    target_model: &Model,
) -> Result<Vec<RelationshipMapping>, Error> {
    let mut resolved_relationship_mappings = Vec::new();
    let mut field_mapping_btree_for_validation: HashSet<&String> = HashSet::new();
    for relationship_mapping in &relationship.mapping {
        let resolved_relationship_source_mapping = match &relationship_mapping.source {
            relationships::RelationshipMappingSource::Value(_v) => {
                return Err(Error::NotSupported {
                    reason: "Relationship mappings from value expressions are not supported yet."
                        .to_string(),
                })
            }
            relationships::RelationshipMappingSource::FieldPath(field_path) => {
                match &field_path[..] {
                    [] => {
                        return Err(Error::EmptyFieldPath {
                            location: "source".to_string(),
                            type_name: source_type_name.clone(),
                            relationship_name: relationship.name.clone(),
                        })
                    }
                    [t] => t,
                    _ => {
                        return Err(Error::NotSupported {
                            reason: "Relationships with nested field paths are not supported yet."
                                .to_string(),
                        })
                    }
                }
            }
        };
        let resolved_relationship_target_mapping = match &relationship_mapping.target {
            relationships::RelationshipMappingTarget::Argument(_argument_name) => {
                return Err(Error::NotSupported {
                    reason: "Relationship mappings to arguments expressions are not supported yet."
                        .to_string(),
                })
            }
            relationships::RelationshipMappingTarget::ModelField(field_path) => {
                match &field_path[..] {
                    [] => {
                        return Err(Error::EmptyFieldPath {
                            location: "target".to_string(),
                            type_name: source_type_name.clone(),
                            relationship_name: relationship.name.clone(),
                        })
                    }
                    [t] => t,
                    _ => {
                        return Err(Error::NotSupported {
                            reason: "Relationships with nested field paths are not supported yet."
                                .to_string(),
                        })
                    }
                }
            }
        };
        let source_field = resolved_relationship_source_mapping.clone();
        let target_field = resolved_relationship_target_mapping.clone();
        if !source_type.fields.contains_key(&source_field.field_name) {
            return Err(Error::UnknownSourceFieldInRelationshipMapping {
                relationship_name: relationship.name.clone(),
                source_type: source_type_name.clone(),
                field_name: source_field.field_name,
            });
        }
        if !target_model
            .type_fields
            .contains_key(&target_field.field_name)
        {
            return Err(Error::UnknownTargetFieldInRelationshipMapping {
                relationship_name: relationship.name.clone(),
                source_type: source_type_name.clone(),
                model_name: target_model.name.clone(),
                field_name: source_field.field_name,
            });
        }
        let resolved_relationship_mapping = {
            if field_mapping_btree_for_validation
                .insert(&resolved_relationship_source_mapping.field_name.0)
            {
                Ok(RelationshipMapping {
                    source_field,
                    target_field,
                })
            } else {
                Err(Error::MappingExistsInRelationship {
                    type_name: source_type_name.clone(),
                    field_name: source_field.field_name,
                    relationship_name: relationship.name.clone(),
                })
            }
        }?;
        resolved_relationship_mappings.push(resolved_relationship_mapping);
    }

    Ok(resolved_relationship_mappings)
}

fn get_relationship_capabilities(
    type_name: &Qualified<CustomTypeName>,
    relationship_name: &RelationshipName,
    target_model: &Model,
    data_connectors: &HashMap<Qualified<DataConnectorName>, DataConnectorContext<'_>>,
) -> Result<Option<RelationshipCapabilities>, Error> {
    let source = if let Some(source) = &target_model.source {
        source
    } else {
        return Ok(None);
    };

    let data_connector = data_connectors
        .get(&source.data_connector.name)
        .ok_or_else(|| Error::UnknownModelDataConnector {
            model_name: target_model.name.clone(),
            data_connector: source.data_connector.name.clone(),
        })?;
    let capabilities = &data_connector.capabilities.capabilities;

    if capabilities.query.variables.is_none() {
        return Err(Error::RelationshipTargetDoesNotSupportForEach {
            type_name: type_name.clone(),
            relationship_name: relationship_name.clone(),
            data_connector_name: source.data_connector.name.clone(),
        });
    };

    let relationships = capabilities.relationships.is_some();

    Ok(Some(RelationshipCapabilities {
        foreach: (),
        relationships,
    }))
}

pub fn resolve_relationship(
    relationship: &RelationshipV1,
    subgraph: &str,
    models: &IndexMap<Qualified<ModelName>, Model>,
    data_connectors: &HashMap<Qualified<DataConnectorName>, DataConnectorContext<'_>>,
    source_type: &ObjectTypeRepresentation,
) -> Result<Relationship, Error> {
    let source_type_name = Qualified::new(subgraph.to_string(), relationship.source.clone());
    let (relationship_target, target_model) = match &relationship.target {
        relationships::RelationshipTarget::Model(target_model) => {
            let qualified_target_model_name = Qualified::new(
                target_model
                    .subgraph()
                    .to_owned()
                    .unwrap_or(subgraph)
                    .to_string(),
                target_model.name.to_owned(),
            );
            let resolved_target_model =
                models.get(&qualified_target_model_name).ok_or_else(|| {
                    Error::UnknownTargetModelUsedInRelationship {
                        type_name: source_type_name.clone(),
                        relationship_name: relationship.name.clone(),
                        model_name: qualified_target_model_name.clone(),
                    }
                })?;
            (
                RelationshipTarget::Model {
                    model_name: qualified_target_model_name,
                    relationship_type: target_model.relationship_type.clone(),
                },
                resolved_target_model,
            )
        }
    };

    let target_capabilities = get_relationship_capabilities(
        &source_type_name,
        &relationship.name,
        target_model,
        data_connectors,
    )?;
    let resolved_relationship_mappings =
        resolve_relationship_mappings(relationship, &source_type_name, source_type, target_model)?;

    let field_name = mk_name(&relationship.name.0)?;
    Ok(Relationship {
        name: relationship.name.clone(),
        field_name,
        source: source_type_name,
        target: relationship_target,
        mappings: resolved_relationship_mappings,
        target_typename: target_model.data_type.clone(),
        target_capabilities,
    })
}
