use serde_json as json;
use tracing_util::SpanVisibility;

use crate::schema::operations;
use crate::schema::operations::response_processing;
use crate::schema::GDS;
use gql::normalized_ast;
use lang_graphql as gql;
use open_dds::ndc_client as ndc;

use super::query_plan::ProcessResponseAs;

pub fn process_response<'s>(
    selection_set: &normalized_ast::SelectionSet<'s, GDS>,
    rows_sets: Vec<ndc::models::RowSet>,
    process_response_as: ProcessResponseAs<'s>,
) -> Result<json::Value, operations::Error> {
    let tracer = tracing_util::global_tracer();
    // Post process the response to add the `__typename` fields
    tracer.in_span("process_response", SpanVisibility::Internal, || {
        let row_set = get_single_rowset(rows_sets)?;
        match process_response_as {
            ProcessResponseAs::Array => {
                let result =
                    response_processing::process_selection_set_as_list(row_set, selection_set)?;
                json::to_value(result).map_err(operations::Error::from)
            }
            ProcessResponseAs::Object => {
                let result =
                    response_processing::process_selection_set_as_object(row_set, selection_set)?;
                json::to_value(result).map_err(operations::Error::from)
            }
            ProcessResponseAs::CommandResponse {
                command_name,
                type_container,
            } => {
                let result = response_processing::process_command_rows(
                    command_name,
                    row_set.rows,
                    selection_set,
                    type_container,
                )?;
                json::to_value(result).map_err(operations::Error::from)
            }
        }
    })
}

fn get_single_rowset(
    rows_sets: Vec<ndc::models::RowSet>,
) -> Result<ndc::models::RowSet, operations::Error> {
    Ok(rows_sets
        .into_iter()
        .next()
        .ok_or(operations::InternalDeveloperError::BadGDCResponse {
            summary: "missing rowset".into(),
        })?)
}
