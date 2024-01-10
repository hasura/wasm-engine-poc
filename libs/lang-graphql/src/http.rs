use crate::ast::common as ast;
use crate::ast::executable;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json;
// use serde_json::json;
use std::collections::HashMap;

/// The request as we receive it from the client, before we
/// parse the query string
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
pub struct RawRequest {
    pub operation_name: Option<ast::Name>,
    pub query: String,
    pub variables: Option<HashMap<ast::Name, serde_json::Value>>,
}

pub struct Request {
    pub operation_name: Option<ast::Name>,
    pub query: executable::ExecutableDocument,
    pub variables: HashMap<ast::Name, serde_json::Value>,
}

pub type VariableValues = HashMap<ast::Name, serde_json::Value>;

#[derive(Serialize, Debug)]
pub struct Extensions {
    /// Details of any error
    pub details: serde_json::Value,
}

#[derive(Serialize, Debug)]
pub struct GraphQLError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Extensions>,
}

#[derive(Serialize)]
pub struct Response {
    #[serde(skip_serializing)]
    pub status_code: http::status::StatusCode,
    pub data: Option<IndexMap<ast::Alias, serde_json::Value>>,
    pub errors: Option<Vec<GraphQLError>>,
}

impl Response {
    pub fn ok(data: IndexMap<ast::Alias, serde_json::Value>) -> Self {
        Self {
            status_code: http::status::StatusCode::OK,
            data: Some(data),
            errors: None,
        }
    }
    pub fn partial(
        data: IndexMap<ast::Alias, serde_json::Value>,
        errors: Vec<GraphQLError>,
    ) -> Self {
        Self {
            status_code: http::status::StatusCode::OK,
            data: Some(data),
            errors: Some(errors),
        }
    }

    pub fn error_with_status(status_code: http::status::StatusCode, error: GraphQLError) -> Self {
        Self {
            status_code,
            data: None,
            errors: Some(vec![error]),
        }
    }

    pub fn error_message_with_status(
        status_code: http::status::StatusCode,
        message: String,
    ) -> Self {
        Self {
            status_code,
            data: None,
            errors: Some(vec![GraphQLError {
                message,
                path: None,
                extensions: None,
            }]),
        }
    }

    pub fn error(error: GraphQLError) -> Self {
        Self {
            status_code: http::status::StatusCode::OK,
            data: None,
            errors: Some(vec![error]),
        }
    }

    pub fn errors_with_status(
        status_code: http::status::StatusCode,
        errors: Vec<GraphQLError>,
    ) -> Self {
        Self {
            status_code,
            data: None,
            errors: Some(errors),
        }
    }

    pub fn errors(errors: Vec<GraphQLError>) -> Self {
        Self {
            status_code: http::status::StatusCode::OK,
            data: None,
            errors: Some(errors),
        }
    }

    pub fn does_contains_error(&self) -> bool {
        self.errors.is_some()
    }
}

// impl axum::response::IntoResponse for Response {
//     fn into_response(self) -> axum::response::Response {
//         let response = match &self.errors {
//             None => {
//                 json!({ "data": self.data })
//             }
//             Some(errors) => {
//                 json!({ "data": self.data, "errors": errors })
//             }
//         };
//         (self.status_code, axum::Json(response)).into_response()
//     }
// }
