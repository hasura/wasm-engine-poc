pub mod metadata;
pub mod schema;
pub mod utils;
use execute::query_plan::NodeQueryPlan;
use wasm_bindgen::prelude::*;
use lang_graphql;
use std::str::FromStr;
pub mod execute;


#[wasm_bindgen(module="/www/utils/log.js")]
extern "C" {
    fn log(message: &str);
}

// #[wasm_bindgen(module="/www/connector/query.ts")]
// extern "C" {
//     fn handle_query_request(query: &str);
// }

#[wasm_bindgen]
pub fn greet(name: &str) {
    let formatted_message = format!("Hello, {}!", name);
    log(&formatted_message);
}


pub fn generate_ir<'n, 's>(
    schema: &'s lang_graphql::schema::Schema<schema::GDS>,
    session: &hasura_authn_core::Session,
    normalized_request: &'s lang_graphql::normalized_ast::Operation<'s, schema::GDS>,
) -> Result<indexmap::IndexMap<lang_graphql::ast::common::Alias, schema::types::root_field::RootField<'n, 's>>, schema::operations::Error> {
    let ir = match &normalized_request.ty {
        lang_graphql::ast::common::OperationType::Query => {
            schema::types::query_root::generate_ir(schema, session, &normalized_request.selection_set)?
        }
        lang_graphql::ast::common::OperationType::Mutation => schema::types::mutation_root::generate_ir(
            &normalized_request.selection_set,
            &session.variables,
        )?,
        lang_graphql::ast::common::OperationType::Subscription => {
            Err(schema::operations::InternalEngineError::SubscriptionsNotSupported)?
        }
    };
    Ok(ir)
}

pub fn execute_query_plan(query_plan: execute::query_plan::QueryPlan) -> indexmap::IndexMap<lang_graphql::ast::common::Alias, serde_json::Value> {
    let mut response = indexmap::IndexMap::new();
    for (alias, field_plan) in query_plan.into_iter() {
        let field_response: serde_json::Value = match field_plan {
            NodeQueryPlan::TypeName { type_name } => {
                log(&"Processing TypeName");
                let res = serde_json::to_value(type_name);
                match res {
                    Ok(r) => {
                        r
                    },
                    Err(_) => {
                        serde_json::Value::Null
                    }
                }
            },
            NodeQueryPlan::TypeField { selection_set, schema, type_name, role: namespace } => {
                log(&"Processing TypeField");
                match schema.get_type(&type_name) {
                    Some(type_info) => {
                        let named_type = lang_graphql::introspection::named_type(
                            schema,
                            &namespace,
                            type_info,
                            selection_set,
                        );
                        match named_type {
                            Ok(named_type) => {
                                match serde_json::to_value(named_type) {
                                    Ok(named_type) => named_type,
                                    _ => serde_json::Value::Null
                                }
                            },
                            _ => {
                                serde_json::Value::Null
                            }
                        }
                    }
                    None => serde_json::Value::Null,
                }
            },
            NodeQueryPlan::SchemaField { role: namespace, selection_set, schema } => {
                log(&"Processing SchemaField");
                match lang_graphql::introspection::schema_type(schema, &namespace, selection_set) {
                    Ok(schema_type) => {
                        match serde_json::to_value(schema_type) {
                            Ok(schema_type) => schema_type,
                            _ => serde_json::Value::Null
                        }
                    },
                    _ => serde_json::Value::Null
                }
            },
            NodeQueryPlan::RelayNodeSelect(optional_query) => {
                log(&format!("optional query: {:?}", optional_query));
                serde_json::Value::Null
            },
            NodeQueryPlan::NDCQueryExecution(ndc_query) => {
                let execute::query_plan::NDCQueryExecution {
                    execution_tree,
                    selection_set,
                    execution_span_attribute,
                    field_span_attribute,
                    process_response_as,
                } = ndc_query;
                log(&format!("execution tree: {:?}", execution_tree));
                log(&format!("selection set: {:?}", selection_set));
                log(&format!("exectuion span attribute: {:?}", execution_span_attribute));
                log(&format!("field span attributes {:?}", field_span_attribute));
                log(&format!("Process response as: {:?}", process_response_as));
                log(&format!("Query: {:?}", execution_tree.root_node.query));
                match serde_json::to_string(&execution_tree.root_node.query) {
                    Ok(res) => {
                        log(&format!("Res: {:?}", res));
                        // handle_query_request(res.as_str());
                        serde_json::json!({"type": "query", "queryRequest": res})
                    },
                    _ => {
                        log(&"Error");
                        serde_json::Value::Null
                    }
                }
            },
            NodeQueryPlan::NDCMutationExecution(ndc_query) => {
                log(&format!("Query: {:?}", ndc_query));
                serde_json::Value::Null
            }
        };
        response.insert(alias.clone(), field_response);
    }
    log(&format!("Response: {:?}", response));
    response
}

// Who needs a standard library? pfffft. We don't need em. 
#[wasm_bindgen]
pub fn handle_request(raw_request: String, schema: String) -> String {
    // log(&raw_request);
    // log(&schema);

    let user_role = hasura_authn_core::Role::new("admin");
    let mut session_variables_map = std::collections::HashMap::new();
    session_variables_map.insert(
        hasura_authn_core::SessionVariable::from_str("x-hasura-user-id").unwrap(),
        hasura_authn_core::SessionVariableValue::new("123"),
    );
    // For instance, if we are dealing with a user that should have a default session with 'user_role'
    let default_role_authorization = hasura_authn_core::RoleAuthorization {
        role: user_role.clone(),
        session_variables: session_variables_map,
        allowed_session_variables_from_request: hasura_authn_core::SessionVariableList::All,
    };

    // Assemble the session
    let dummy_client_provided_variables = std::collections::HashMap::new();
    let session = default_role_authorization.build_session(dummy_client_provided_variables);

    // log(&format!("Session: {:?}", user_session));

    let gql_schema: Option<lang_graphql::schema::Schema<schema::GDS>> = match schema::GDS::new(&schema) {
        Ok(graphql_schema) => {
            match graphql_schema.build_schema() {
                Ok(graphql_schema) => {
                    Some(graphql_schema)
                },
                Err(_) => {
                    log("Bad schema");
                    None
                }
            }
        },
        Err(_) => {
            log("Bad schema");
            None
        }
    };

    if let Some(schema) = gql_schema {
        match serde_json::from_str::<lang_graphql::http::RawRequest>(&raw_request) {
            Ok(raw_request) => {
                log(&format!("Parsed Request: {:?}", raw_request));
                let parse_result = lang_graphql::parser::Parser::new(&raw_request.query).parse_executable_document();
                match parse_result {
                    Ok(executable_document) => {
                        // log(&format!("Executable Document: {:?}", executable_document));
                        let request = lang_graphql::http::Request {
                            operation_name: raw_request.operation_name,
                            query: executable_document,
                            variables: raw_request.variables.unwrap_or_default(),
                        };

                        let normalized_request = lang_graphql::validation::normalize_request(&session.role, &schema, &request);

                        match normalized_request {
                            Ok(request) => {
                                let ir = generate_ir(&schema, &session, &request);
                                match ir {
                                    Ok(ir) => {
                                        log(&format!("IR: {:?}", ir));
                                        let query_plan = execute::query_plan::generate_query_plan(&ir);
                                        match query_plan {
                                            Ok(query_plan) => {
                                                log(&format!("Query Plan: {:?}", query_plan));
                                                let query_response = execute_query_plan(query_plan);
                                                let json_response = serde_json::to_value(&query_response);

                                                match json_response {
                                                    Ok(json_response) => {
                                                        json_response.to_string()
                                                    },
                                                    Err(_) => {
                                                        log(&"Error");
                                                        "{}".to_string()
                                                    }
                                                }
                                            }, 
                                            Err(_) => {
                                                log(&"Error!");
                                                "{}".to_string()
                                            }
                                        }
                                    }, 
                                    Err(_) => {
                                        log("Error generating ir");
                                        "{}".to_string()
                                    }
                                }
                            }, 
                            Err(_) => {
                                log("Bad request");
                                "{}".to_string()
                            }
                        }

                    }
                    Err(parse_error) => {
                        log(&format!("Parsing Error: {:?}", parse_error));
                        "{}".to_string()
                    }
                }
            }
            Err(e) => {
                // Handle the error, perhaps log it
                log(&format!("Failed to parse request: {}", e));
                "{}".to_string()
            }
        }
    } else {
        log("Schema missing");
        "{}".to_string()
    }
}

//     raw_request: gql::http::RawRequest,