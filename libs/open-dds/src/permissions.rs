use std::collections::HashSet;

use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{
    commands::CommandName,
    models::{ModelName, OperatorName},
    relationships::RelationshipName,
    session_variables::SessionVariable,
    types::{CustomTypeName, FieldName},
};

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, Hash, Display)]
pub struct Role(pub String);

impl Role {
    pub fn new(str: &str) -> Role {
        Role(str.to_string())
    }
}

/// List of roles and their permissions for a type
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "TypePermissions")]
pub enum TypePermissions {
    V1(TypePermissionsV1),
}

impl TypePermissions {
    pub fn upgrade(self) -> TypePermissionsV1 {
        match self {
            TypePermissions::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "TypePermissionsV1")]
pub struct TypePermissionsV1 {
    pub type_name: CustomTypeName,
    pub permissions: Vec<TypePermission>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "TypePermission")]
pub struct TypePermission {
    pub role: Role,
    pub output: Option<TypeOutputPermission>,
}

/// One unit of output permission
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "TypeOutputPermission")]
pub struct TypeOutputPermission {
    /// Fields of the type that are accessible for a role
    pub allowed_fields: HashSet<FieldName>,
    // TODO: Presets for field arguments
    // pub field_argument_presets: HashMap<FieldName, Vec<ParameterPreset>>,
}

/// Roles and their permissions for a model
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelPermissions")]
pub enum ModelPermissions {
    V1(ModelPermissionsV1),
}

impl ModelPermissions {
    pub fn upgrade(self) -> ModelPermissionsV1 {
        match self {
            ModelPermissions::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelPermissionsV1")]
pub struct ModelPermissionsV1 {
    pub model_name: ModelName,
    pub permissions: Vec<ModelPermission>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelPermission")]
pub struct ModelPermission {
    pub role: Role,
    pub select: Option<SelectPermission>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "SelectPermission")]
pub struct SelectPermission {
    /// Filter expression when selecting rows for this model.
    /// Null filter implies all rows are selectable.
    pub filter: NullableModelPredicate,
    //TODO: Implement the following when aggregate queries are introduced
    // #[serde(default)]
    // pub allow_aggregations: bool,
}

// We use this instead of an Option, so that we can make the filter field in
// SelectPermission required, but still accept an explicit null value.
// This is why we also need to use serde untagged.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum NullableModelPredicate {
    Null(()),
    NotNull(ModelPredicate),
}

impl NullableModelPredicate {
    pub fn as_option_ref(&self) -> Option<&ModelPredicate> {
        match self {
            NullableModelPredicate::Null(_) => None,
            NullableModelPredicate::NotNull(p) => Some(p),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandPermission")]
pub struct CommandPermission {
    pub role: Role,
    // TODO: Implement predicates and presets
    pub allow_execution: bool,
}

/// Role-Permission map for a command
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandPermissions")]
pub enum CommandPermissions {
    V1(CommandPermissionsV1),
}

impl CommandPermissions {
    pub fn upgrade(self) -> CommandPermissionsV1 {
        match self {
            CommandPermissions::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandPermissionsV1")]
pub struct CommandPermissionsV1 {
    pub command_name: CommandName,
    pub permissions: Vec<CommandPermission>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "FieldComparisonPredicate")]
pub struct FieldComparisonPredicate {
    pub field: FieldName,
    pub operator: OperatorName,
    // Optional to support unary operators
    pub value: Option<ValueExpression>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "RelationshipPredicate")]
pub struct RelationshipPredicate {
    pub name: RelationshipName,
    pub predicate: Option<Box<ModelPredicate>>,
}

// Predicates that use NDC operators pushed down to NDC. `ValueExpressions` are
// evaluated on the server.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ModelPredicate {
    FieldComparison(FieldComparisonPredicate),
    // TODO: Remote relationships are disallowed for now
    Relationship(RelationshipPredicate),
    #[schemars(title = "And")]
    And(Vec<ModelPredicate>),
    #[schemars(title = "Or")]
    Or(Vec<ModelPredicate>),
    #[schemars(title = "Not")]
    Not(Box<ModelPredicate>),
    // TODO: Figure out the story with _ceq
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ValueExpression {
    #[schemars(title = "Literal")]
    Literal(JsonValue),
    #[schemars(title = "SessionVariable")]
    SessionVariable(SessionVariable),
    // TODO: Uncomment the below, once commands are supported.
    // Command {
    //     name: CommandName,
    //     arguments: HashMap<ArgumentName, ValueExpression>,
    // },
}
