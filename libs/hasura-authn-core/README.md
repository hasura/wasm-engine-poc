### Commenting out these sections let you remove axum

// impl IntoResponse for SessionError {
//     fn into_response(self) -> axum::response::Response {
//         match self {
//             SessionError::Unauthorized(role) => Response::error_message_with_status(
//                 StatusCode::UNAUTHORIZED,
//                 format!("cannot be authorized as role: {}", role),
//             )
//             .into_response(),
//             SessionError::InternalRoleNotFound(role) => Response::error_message_with_status(
//                 StatusCode::INTERNAL_SERVER_ERROR,
//                 format!("internal: RoleAuthorization of role: {} not found", role),
//             )
//             .into_response(),
//             SessionError::InvalidHeaderValue { header_name, error } => {
//                 Response::error_message_with_status(
//                     StatusCode::BAD_REQUEST,
//                     format!(
//                         "the value of the header '{}' isn't a valid string: '{}'",
//                         header_name, error
//                     ),
//                 )
//                 .into_response()
//             }
//         }
//     }
// }

// Using the x-hasura-* headers of the request and the identity set by the authn system,
// this layer resolves a 'session' which is then used by the execution engine
// pub async fn resolve_session<'a, B>(
//     Extension(identity): Extension<Identity>,
//     mut request: Request<B>,
//     next: Next<B>,
// ) -> axum::response::Result<axum::response::Response> {
//     let mut session_variables = HashMap::new();
//     let mut role = None;
//     // traverse through the headers and collect role and session variables
//     for (header_name, header_value) in request.headers() {
//         if let Ok(session_variable) = SessionVariable::from_str(header_name.as_str()) {
//             let variable_value = match header_value.to_str() {
//                 Err(e) => Err(SessionError::InvalidHeaderValue {
//                     header_name: header_name.to_string(),
//                     error: e.to_string(),
//                 })?,
//                 Ok(h) => SessionVariableValue::new(h),
//             };

//             if session_variable == SESSION_VARIABLE_ROLE.to_owned() {
//                 role = Some(Role::new(&variable_value.0))
//             } else {
//                 // TODO: Handle the duplicate case?
//                 session_variables.insert(session_variable, variable_value);
//             }
//         }
//     }
//     let session = identity
//         .get_role_authorization(role.as_ref())?
//         .build_session(session_variables);
//     request.extensions_mut().insert(session);
//     let response = next.run(request).await;
//     Ok(response)
// }