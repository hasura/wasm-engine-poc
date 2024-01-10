use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    arguments::ArgumentName,
    models::ModelName,
    permissions::ValueExpression,
    types::{CustomTypeName, FieldName},
};

/// The name of the GraphQL relationship field.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema, derive_more::Display, Hash,
)]
pub struct RelationshipName(pub String);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
// TODO: Should we serialize and deserialize to UPPERCASE?
/// Type of the relationship.
#[schemars(title = "RelationshipType")]
pub enum RelationshipType {
    /// Select one related object from the target.
    Object,
    /// Select multiple related objects from the target.
    Array,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelRelationshipTarget")]
pub struct ModelRelationshipTarget {
    pub name: ModelName,
    // Deprecated, this solely exits for backwards compatibility till all the
    // tooling moves to the subgraph terminology
    namespace: Option<String>,
    subgraph: Option<String>,
    pub relationship_type: RelationshipType,
}

impl ModelRelationshipTarget {
    pub fn subgraph(&self) -> Option<&str> {
        self.subgraph
            .as_ref()
            .or(self.namespace.as_ref())
            .map(|x| x.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RelationshipTarget {
    Model(ModelRelationshipTarget),
    // TODO: CommandRelationshipTarget
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "RelationshipSourceFieldAccess")]
pub struct FieldAccess {
    pub field_name: FieldName,
    // #[serde(default)]
    // pub arguments: HashMap<ArgumentName, ValueExpression>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(title = "RelationshipMappingSource")]
pub enum RelationshipMappingSource {
    #[schemars(title = "SourceValue")]
    Value(ValueExpression),
    #[schemars(title = "SourceField")]
    FieldPath(Vec<FieldAccess>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(title = "RelationshipMappingTarget")]
pub enum RelationshipMappingTarget {
    #[schemars(title = "TargetArgument")]
    Argument(ArgumentName),
    #[schemars(title = "TargetModelField")]
    ModelField(Vec<FieldAccess>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "RelationshipMapping")]
pub struct RelationshipMapping {
    pub source: RelationshipMappingSource,
    pub target: RelationshipMappingTarget,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "Relationship")]
pub enum Relationship {
    V1(RelationshipV1),
}

impl Relationship {
    pub fn upgrade(self) -> RelationshipV1 {
        match self {
            Relationship::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "RelationshipV1")]
pub struct RelationshipV1 {
    pub name: RelationshipName,
    pub source: CustomTypeName,
    pub target: RelationshipTarget,
    pub mapping: Vec<RelationshipMapping>,
}
