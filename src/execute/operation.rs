use indexmap::IndexMap;
use serde_json as json;

use crate::metadata::resolved;
use crate::schema::operations;
use crate::schema::operations::response_processing;
use crate::schema::GDS;
use gql::normalized_ast;
use lang_graphql as gql;
use lang_graphql::ast::common as ast;
use open_dds::ndc_client as ndc;

use tracing_util::{set_attribute_on_active_span, AttributeVisibility, SpanVisibility};

use super::process_response::process_response;
use super::query_plan::{
    NDCMutationExecution, NDCQueryExecution, NodeQueryPlan, ProcessResponseAs, QueryPlan,
};
use super::remote_joins::execute_join_locations;

pub async fn execute_query_plan<'n, 's>(
    http_client: &reqwest::Client,
    query_plan: QueryPlan<'n, 's>,
) -> Result<IndexMap<ast::Alias, json::Value>, operations::Error> {
    let mut response = IndexMap::new();
    // println!("Query Plan: {:?}", query_plan);
    for (alias, field_plan) in query_plan.into_iter() {
        let field_response: json::Value = match field_plan {
            NodeQueryPlan::TypeName { type_name } => {
                set_attribute_on_active_span(AttributeVisibility::Default, "field", "__typename");
                json::to_value(type_name)?
            }
            NodeQueryPlan::TypeField {
                selection_set,
                schema,
                type_name,
                role: namespace,
            } => {
                set_attribute_on_active_span(AttributeVisibility::Default, "field", "__type");
                match schema.get_type(&type_name) {
                    Some(type_info) => json::to_value(gql::introspection::named_type(
                        schema,
                        &namespace,
                        type_info,
                        selection_set,
                    )?)?,
                    None => json::Value::Null,
                }
            }
            NodeQueryPlan::SchemaField {
                role: namespace,
                selection_set,
                schema,
            } => {
                set_attribute_on_active_span(AttributeVisibility::Default, "field", "__schema");
                json::to_value(gql::introspection::schema_type(
                    schema,
                    &namespace,
                    selection_set,
                )?)?
            }
            NodeQueryPlan::NDCQueryExecution(ndc_query) => {
                let NDCQueryExecution {
                    execution_tree,
                    selection_set,
                    execution_span_attribute,
                    field_span_attribute,
                    process_response_as,
                } = ndc_query;
                let mut response = execute_ndc_query(
                    http_client,
                    execution_tree.root_node.query,
                    execution_tree.root_node.data_connector,
                    execution_span_attribute.clone(),
                    field_span_attribute.clone(),
                )
                .await?;
                execute_join_locations(
                    http_client,
                    execution_span_attribute,
                    field_span_attribute,
                    &mut response,
                    &process_response_as,
                    execution_tree.remote_executions,
                )
                .await?;
                let result = process_response(selection_set, response, process_response_as)?;
                json::to_value(result).map_err(operations::Error::from)?
            }
            NodeQueryPlan::NDCMutationExecution(ndc_query) => {
                let NDCMutationExecution {
                    query,
                    data_connector,
                    selection_set,
                    execution_span_attribute,
                    field_span_attribute,
                    process_response_as,
                    // TODO: remote joins are not handled for mutations
                    join_locations: _,
                } = ndc_query;
                let response = execute_ndc_mutation(
                    http_client,
                    query,
                    data_connector,
                    selection_set,
                    execution_span_attribute,
                    field_span_attribute,
                    process_response_as,
                )
                .await?;
                json::to_value(response).map_err(operations::Error::from)?
            }
            NodeQueryPlan::RelayNodeSelect(optional_query) => match optional_query {
                None => json::Value::Null,
                Some(ndc_query) => {
                    let NDCQueryExecution {
                        execution_tree,
                        selection_set,
                        execution_span_attribute,
                        field_span_attribute,
                        process_response_as,
                    } = ndc_query;
                    let mut response = execute_ndc_query(
                        http_client,
                        execution_tree.root_node.query,
                        execution_tree.root_node.data_connector,
                        execution_span_attribute.clone(),
                        field_span_attribute.clone(),
                    )
                    .await?;
                    execute_join_locations(
                        http_client,
                        execution_span_attribute,
                        field_span_attribute,
                        &mut response,
                        &process_response_as,
                        execution_tree.remote_executions,
                    )
                    .await?;
                    let result = process_response(selection_set, response, process_response_as)?;
                    json::to_value(result).map_err(operations::Error::from)?
                }
            },
        };
        response.insert(alias.clone(), field_response);
    }
    Ok(response)
}

/// Executes a NDC operation
pub async fn execute_ndc_query<'n, 's>(
    http_client: &reqwest::Client,
    query: ndc::models::QueryRequest,
    data_connector: &resolved::data_connector::DataConnector,
    execution_span_attribute: String,
    field_span_attribute: String,
) -> Result<Vec<ndc::models::RowSet>, operations::Error> {
    println!("Query Request: {:?}", query);
    let tracer = tracing_util::global_tracer();
    tracer
        .in_span_async("execute_ndc_query", SpanVisibility::User, || {
            Box::pin(async {
                set_attribute_on_active_span(
                    AttributeVisibility::Default,
                    "operation",
                    execution_span_attribute,
                );
                set_attribute_on_active_span(
                    AttributeVisibility::Default,
                    "field",
                    field_span_attribute,
                );
                let connector_response =
                    fetch_from_data_connector(http_client, query, data_connector).await?;
                Ok(connector_response.0)
            })
        })
        .await
}

pub(crate) async fn fetch_from_data_connector<'s>(
    http_client: &reqwest::Client,
    query_request: ndc::models::QueryRequest,
    data_connector: &resolved::data_connector::DataConnector,
) -> Result<ndc::models::QueryResponse, operations::Error> {
    let tracer = tracing_util::global_tracer();
    tracer
        .in_span_async(
            "fetch_from_data_connector",
            SpanVisibility::Internal,
            || {
                Box::pin(async {
                    let ndc_config = ndc::apis::configuration::Configuration {
                        base_path: data_connector.url.get_url(ast::OperationType::Query),
                        user_agent: None,
                        // This is isn't expensive, reqwest::Client is behind an Arc
                        client: http_client.clone(),
                        headers: data_connector.headers.0.clone(),
                    };
                    ndc::apis::default_api::query_post(&ndc_config, query_request)
                        .await
                        .map_err(operations::Error::from) // ndc_client::apis::Error -> InternalError -> Error
                })
            },
        )
        .await
}

pub(crate) async fn execute_ndc_mutation<'n, 's>(
    http_client: &reqwest::Client,
    query: ndc::models::MutationRequest,
    data_connector: &resolved::data_connector::DataConnector,
    selection_set: &'n normalized_ast::SelectionSet<'s, GDS>,
    execution_span_attribute: String,
    field_span_attribute: String,
    process_response_as: ProcessResponseAs<'s>,
) -> Result<json::Value, operations::Error> {
    let tracer = tracing_util::global_tracer();
    tracer
        .in_span_async("execute_ndc_mutation", SpanVisibility::User, || {
            Box::pin(async {
                set_attribute_on_active_span(
                    AttributeVisibility::Default,
                    "operation",
                    execution_span_attribute,
                );
                set_attribute_on_active_span(
                    AttributeVisibility::Default,
                    "field",
                    field_span_attribute,
                );
                let connector_response =
                    fetch_from_data_connector_mutation(http_client, query, data_connector).await?;
                // Post process the response to add the `__typename` fields
                tracer.in_span("process_response", SpanVisibility::Internal, || {
                    // NOTE: NDC returns a `Vec<RowSet>` (to account for
                    // variables). We don't use variables in NDC queries yet,
                    // hence we always pick the first `RowSet`.
                    let mutation_results = connector_response
                        .operation_results
                        .into_iter()
                        .next()
                        .ok_or(operations::InternalDeveloperError::BadGDCResponse {
                            summary: "missing rowset".into(),
                        })?;
                    match process_response_as {
                        ProcessResponseAs::CommandResponse {
                            command_name,
                            type_container,
                        } => {
                            let result = response_processing::process_command_rows(
                                command_name,
                                mutation_results.returning,
                                selection_set,
                                type_container,
                            )?;
                            Ok(json::to_value(result).map_err(operations::Error::from))
                        }
                        _ => Err(operations::Error::from(
                            operations::InternalEngineError::InternalGeneric {
                                description: "mutations without commands are not supported yet"
                                    .into(),
                            },
                        )),
                    }?
                })
            })
        })
        .await
}

pub(crate) async fn fetch_from_data_connector_mutation<'s>(
    http_client: &reqwest::Client,
    query_request: ndc::models::MutationRequest,
    data_connector: &resolved::data_connector::DataConnector,
) -> Result<ndc::models::MutationResponse, operations::Error> {
    let tracer = tracing_util::global_tracer();
    tracer
        .in_span_async(
            "fetch_from_data_connector",
            SpanVisibility::Internal,
            || {
                Box::pin(async {
                    let gdc_config = ndc::apis::configuration::Configuration {
                        base_path: data_connector.url.get_url(ast::OperationType::Mutation),
                        user_agent: None,
                        // This is isn't expensive, reqwest::Client is behind an Arc
                        client: http_client.clone(),
                        headers: data_connector.headers.0.clone(),
                    };
                    ndc::apis::default_api::mutation_post(&gdc_config, query_request)
                        .await
                        .map_err(operations::Error::from) // ndc_client::apis::Error -> InternalError -> Error
                })
            },
        )
        .await
}
