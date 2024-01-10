use std::fmt::Display;

use schemars::JsonSchema;
use serde::{
    de::value::{StrDeserializer, StringDeserializer},
    Deserialize, Serialize,
};

use crate::data_connector::DataConnectorName;

#[derive(
    Serialize, Deserialize, Hash, Clone, Debug, PartialEq, Eq, JsonSchema, derive_more::Display,
)]
#[serde(untagged)]
// TODO: This serde untagged causes bad error messages when the type name is invalid.
// Either manually deserialize it or use a library to make the error messages better.
pub enum TypeName {
    Inbuilt(InbuiltType),
    Custom(CustomTypeName),
}

/// The name of a user-defined type.
#[derive(
    Serialize, Clone, Debug, PartialEq, Eq, Hash, derive_more::Display, JsonSchema, PartialOrd, Ord,
)]
pub struct CustomTypeName(pub String);
impl CustomTypeName {
    fn new(s: String) -> Result<CustomTypeName, String> {
        // First character should be alphabetic or underscore
        let first_char_valid =
            matches!(s.chars().next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
        // All characters should be alphanumeric or underscore
        let all_chars_valid = s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        // Should not be an inbuilt type
        let not_an_inbuilt_type =
            InbuiltType::deserialize(StrDeserializer::<serde::de::value::Error>::new(s.as_str()))
                .is_err();
        if first_char_valid && all_chars_valid && not_an_inbuilt_type {
            Ok(CustomTypeName(s))
        } else {
            Err(format!("invalid custom type name: {s}"))
        }
    }
}

impl<'de> Deserialize<'de> for CustomTypeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        CustomTypeName::new(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
/// Reference to an Open DD type including nullable values and arrays.
/// Suffix '!' to indicate a non-nullable reference, and wrap in '[]' to indicate an array.
/// Eg: '[String!]!' is a non-nullable array of non-nullable strings.
pub struct TypeReference {
    pub underlying_type: BaseType,
    pub nullable: bool,
}

impl Display for TypeReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            self.underlying_type,
            if self.nullable { "" } else { "!" }
        )
    }
}

impl Serialize for TypeReference {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for TypeReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let mut chars = s.chars();
        Ok(if chars.next_back() == Some('!') {
            TypeReference {
                underlying_type: BaseType::deserialize(StrDeserializer::new(chars.as_str()))?,
                nullable: false,
            }
        } else {
            TypeReference {
                underlying_type: BaseType::deserialize(StringDeserializer::new(s))?,
                nullable: true,
            }
        })
    }
}

const TYPE_REFERENCE_DESCRIPTION: &str = r#"A reference to an Open DD type including nullable values and arrays.
Suffix '!' to indicate a non-nullable reference, and wrap in '[]' to indicate an array.
Eg: '[String!]!' is a non-nullable array of non-nullable strings."#;

impl JsonSchema for TypeReference {
    fn schema_name() -> String {
        "TypeReference".into()
    }

    // TODO: Add description / examples to the json schema
    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        schemars::schema::Schema::Object(schemars::schema::SchemaObject {
            metadata: Some(Box::new(schemars::schema::Metadata {
                description: Some(TYPE_REFERENCE_DESCRIPTION.into()),
                ..Default::default()
            })),
            instance_type: Some(schemars::schema::SingleOrVec::Single(Box::new(
                schemars::schema::InstanceType::String,
            ))),
            ..Default::default()
        })
    }
}

#[derive(Hash, Clone, Debug, PartialEq, Eq, derive_more::Display)]
pub enum BaseType {
    #[display(fmt = "{_0}")]
    Named(TypeName),
    #[display(fmt = "[{_0}]")]
    List(Box<TypeReference>),
}

impl JsonSchema for BaseType {
    fn schema_name() -> String {
        "BaseType".into()
    }

    // TODO: Add description / examples to the json schema
    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        String::json_schema(gen)
    }
}

impl Serialize for BaseType {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for BaseType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let mut chars = s.chars();
        Ok(
            if chars.next() == Some('[') && chars.next_back() == Some(']') {
                BaseType::List(Box::new(TypeReference::deserialize(StrDeserializer::new(
                    chars.as_str(),
                ))?))
            } else {
                BaseType::Named(TypeName::deserialize(StringDeserializer::new(s))?)
            },
        )
    }
}

#[derive(
    Serialize, Deserialize, Hash, Clone, Debug, PartialEq, Eq, JsonSchema, derive_more::Display,
)]
pub enum InbuiltType {
    ID,
    Int,
    Float,
    Boolean,
    String,
}

#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, derive_more::Display, JsonSchema,
)]
pub struct GraphQlTypeName(pub String);

/// The name of a GraphQL object field.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, derive_more::Display, JsonSchema,
)]
pub struct GraphQlFieldName(pub String);

/// GraphQL configuration of an Open DD object type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ObjectTypeGraphQLConfiguration")]
pub struct ObjectTypeGraphQLConfiguration {
    /// The name to use for the GraphQL type representation of this object type.
    pub type_name: Option<GraphQlTypeName>,
    /// The name to use for the GraphQL input type representation of this object type.
    pub input_type_name: Option<GraphQlTypeName>,
    // TODO: Add type_kind if we want to allow making objects interfaces.
}

/// Definition of a user-defined Open DD object type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ObjectType")]
pub enum ObjectType {
    V1(ObjectTypeV1),
}

impl ObjectType {
    pub fn upgrade(self) -> ObjectTypeV1 {
        match self {
            ObjectType::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ObjectTypeV1")]
pub struct ObjectTypeV1 {
    pub name: CustomTypeName,
    pub fields: Vec<FieldDefinition>,
    pub global_id_fields: Option<Vec<FieldName>>,
    /// GraphQl configuration for this object.
    pub graphql: Option<ObjectTypeGraphQLConfiguration>,
}

/// The name of a field in a user-defined object type.
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    derive_more::Display,
    JsonSchema,
    PartialOrd,
    Ord,
)]
pub struct FieldName(pub String);

/// The definition of a field in a user-defined object type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "ObjectFieldDefinition")]
pub struct FieldDefinition {
    pub name: FieldName,
    #[serde(rename = "type")]
    pub field_type: TypeReference,
}

/// GraphQL configuration of an Open DD scalar type
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ScalarTypeGraphQLConfiguration")]
pub struct ScalarTypeGraphQLConfiguration {
    /// The name of the GraphQl type to use for this scalar.
    pub type_name: GraphQlTypeName,
    // TODO: add a representation field if we want to give semantics to this
    // scalar type.
}

/// Definition of a user-defined scalar type that that has opaque semantics.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ScalarType")]
pub enum ScalarType {
    V1(ScalarTypeV1),
}

impl ScalarType {
    pub fn upgrade(self) -> ScalarTypeV1 {
        match self {
            ScalarType::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "ScalarTypeV1")]
pub struct ScalarTypeV1 {
    /// The OpenDD name of this type.
    pub name: CustomTypeName,
    /// The name of the GraphQl scalar type to use for
    pub graphql: Option<ScalarTypeGraphQLConfiguration>,
}

/// GraphQL configuration of a data connector scalar
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "DataConnectorScalarGraphQLConfiguration")]
pub struct DataConnectorScalarGraphQLConfiguration {
    pub comparison_expression_type_name: Option<GraphQlTypeName>,
}

/// The representation of a data connector scalar in terms of Open DD types
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(tag = "version", content = "definition")]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "DataConnectorScalarRepresentation")]
pub enum DataConnectorScalarRepresentation {
    V1(DataConnectorScalarRepresentationV1),
}

impl DataConnectorScalarRepresentation {
    pub fn upgrade(self) -> DataConnectorScalarRepresentationV1 {
        match self {
            DataConnectorScalarRepresentation::V1(v1) => v1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[schemars(title = "DataConnectorScalarRepresentationV1")]
pub struct DataConnectorScalarRepresentationV1 {
    pub data_connector_name: DataConnectorName,
    pub data_connector_scalar_type: String,
    pub representation: TypeName,
    pub graphql: Option<DataConnectorScalarGraphQLConfiguration>,
}
