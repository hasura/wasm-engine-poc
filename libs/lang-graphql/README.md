# Porting lang-graphql to WASM

### The only dependency that can't ship to wasm is axum.

### There is only one place in the entire project that uses axum.

Commenting out this code in http.rs allows you to compile lang-graphql to WASM:

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
