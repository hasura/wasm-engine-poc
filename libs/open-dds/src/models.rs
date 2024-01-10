use std::collections::HashMap;

use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    arguments::{ArgumentDefinition, ArgumentName},
    commands::TypeMapping,
    data_connector::DataConnectorName,
    types::{CustomTypeName, FieldName, GraphQlFieldName, GraphQlTypeName},
};

/// The name of data model.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, derive_more::Display, JsonSchema,
)]
pub struct ModelName(pub String);

/// The definition of a data model.
/// A data model is a collection of objects of a particular type. Models can support one or more CRUD operations.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "Model")]
pub enum Model {
    V1(ModelV1),
}

impl Model {
    pub fn upgrade(self) -> ModelV1 {
        match self {
            Model::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelV1")]
pub struct ModelV1 {
    pub name: ModelName,
    /// The type of the objects of which this model is a collection.
    pub object_type: CustomTypeName,
    #[serde(default)]
    pub global_id_source: bool,
    #[serde(default)]
    pub arguments: Vec<ArgumentDefinition>,
    pub source: Option<ModelSource>,
    pub filterable_fields: Vec<FilterableField>,
    pub orderable_fields: Vec<OrderableField>,
    pub graphql: Option<ModelGraphQlDefinition>,
}

/// Description of how a model maps to a particular data connector
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelSource")]
pub struct ModelSource {
    /// The name of the data connector backing this model.
    pub data_connector_name: DataConnectorName,

    /// The collection in the data connector that backs this model.
    pub collection: String,

    /// How the various types used in this model correspond to
    /// entities in the data connector.
    #[serde(default)]
    pub type_mapping: HashMap<CustomTypeName, TypeMapping>,

    // Mapping from model argument names to data connector table argument names.
    #[serde(default)]
    pub argument_mapping: HashMap<ArgumentName, String>,
}

/// The definition of the GraphQL API component specific to a model.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ModelGraphQlDefinition")]
pub struct ModelGraphQlDefinition {
    pub select_uniques: Vec<SelectUniqueGraphQlDefinition>,
    pub select_many: Option<SelectManyGraphQlDefinition>,
    pub arguments_input_type: Option<GraphQlTypeName>,
    /// The type name of the filter boolean expression.
    pub filter_expression_type: Option<GraphQlTypeName>,
    pub order_by_expression_type: Option<GraphQlTypeName>,
}

/// The definition of the GraphQL API for selecting a unique row/object from a model.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "SelectUniqueGraphQlDefinition")]
pub struct SelectUniqueGraphQlDefinition {
    /// The name of the query root field for this API.
    pub query_root_field: GraphQlFieldName,
    /// A set of fields which can uniquely identify a row/object in the model.
    pub unique_identifier: Vec<FieldName>,
}

/// The definition of the GraphQL API for selecting rows from a model.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "SelectManyGraphQlDefinition")]
pub struct SelectManyGraphQlDefinition {
    /// The name of the query root field for this API.
    pub query_root_field: GraphQlFieldName,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "FilterableField")]
pub struct FilterableField {
    pub field_name: FieldName,
    pub operators: EnableAllOrSpecific<OperatorName>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "OrderableField")]
pub struct OrderableField {
    pub field_name: FieldName,
    pub order_by_directions: EnableAllOrSpecific<OrderByDirection>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(title = "EnableAllOrSpecific")]
pub enum EnableAllOrSpecific<T> {
    EnableAll(bool),
    EnableSpecific(Vec<T>),
}

#[derive(
    Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, PartialOrd, Ord, Hash,
)]
#[schemars(title = "OrderByDirection")]
pub enum OrderByDirection {
    Asc,
    Desc,
}

#[derive(
    Display, Serialize, Deserialize, Clone, Debug, Eq, PartialEq, JsonSchema, PartialOrd, Ord, Hash,
)]
pub struct OperatorName(pub String);
