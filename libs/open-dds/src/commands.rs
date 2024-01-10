use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    arguments::{ArgumentDefinition, ArgumentName},
    data_connector::DataConnectorName,
    types::{CustomTypeName, FieldName, GraphQlFieldName, TypeReference},
};

/// The name of a command.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, derive_more::Display, JsonSchema,
)]
pub struct CommandName(pub String);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DataConnectorCommand {
    #[schemars(title = "Function")]
    Function(String),
    #[schemars(title = "Procedure")]
    Procedure(String),
}

/// The definition of a command.
/// A command is a user-defined operation which can take arguments and returns an output.
/// The semantics of a command are opaque to the Open DD specification.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "Command")]
pub enum Command {
    V1(CommandV1),
}

impl Command {
    pub fn upgrade(self) -> CommandV1 {
        match self {
            Command::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandV1")]
pub struct CommandV1 {
    pub name: CommandName,
    /// The type of the objects which is returned as the output.
    pub output_type: TypeReference,
    #[serde(default)]
    pub arguments: Vec<ArgumentDefinition>,
    pub source: Option<CommandSource>,
    pub graphql: Option<CommandGraphQlDefinition>,
}

/// Description of how a command maps to a particular data connector
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandSource")]
pub struct CommandSource {
    /// The name of the data connector backing this command.
    pub data_connector_name: DataConnectorName,

    /// The function/procedure in the data connector that backs this command.
    pub data_connector_command: DataConnectorCommand,

    /// How the various types used in this command correspond to
    /// entities in the data connector.
    #[serde(default)]
    pub type_mapping: HashMap<CustomTypeName, TypeMapping>,

    /// Mapping from command argument names to data connector table argument names.
    #[serde(default)]
    pub argument_mapping: HashMap<ArgumentName, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
pub enum GraphQlRootFieldKind {
    Query,
    Mutation,
}

/// The definition of the GraphQL API component specific to a command.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "CommandGraphQlDefinition")]
pub struct CommandGraphQlDefinition {
    /// The name of the graphql root field to use for this command.
    pub root_field_name: GraphQlFieldName,
    /// Whether to put this command in the Query or Mutation root of the GraphQL API.
    pub root_field_kind: GraphQlRootFieldKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "TypeMapping")]
pub struct TypeMapping {
    pub field_mapping: HashMap<FieldName, FieldMapping>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ObjectFieldMapping")]
pub struct FieldMapping {
    pub column: String,
    // TODO: Map field arguments
}
