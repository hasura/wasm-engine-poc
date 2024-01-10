use std::collections::HashMap;

use crate::schema::operations::model_selection::ModelSelection;
use crate::schema::operations::remote_joins::{
    JoinId, JoinLocations, Location, MonotonicCounter, RemoteJoin,
};
use indexmap::IndexMap;
use open_dds::commands;

use crate::metadata::resolved::{self, subgraph};
use crate::schema::operations::{self, InternalEngineError};
use crate::schema::types::root_field;
use crate::schema::GDS;
use gql::normalized_ast;
use hasura_authn_core::Role;
use lang_graphql as gql;
use lang_graphql::ast::common as ast;
use open_dds::ndc_client as ndc;

pub type QueryPlan<'n, 's> = IndexMap<ast::Alias, NodeQueryPlan<'n, 's>>;

/// Query plan of individual root field or node
#[derive(Debug)]
pub enum NodeQueryPlan<'n, 's> {
    /// __typename field on query root
    TypeName { type_name: ast::TypeName },
    /// __schema field
    SchemaField {
        role: Role,
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        schema: &'s gql::schema::Schema<GDS>,
    },
    /// __type field
    TypeField {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        schema: &'s gql::schema::Schema<GDS>,
        type_name: ast::TypeName,
        role: Role,
    },
    /// NDC query to be executed
    NDCQueryExecution(NDCQueryExecution<'n, 's>),
    /// NDC query for Relay 'node' to be executed
    RelayNodeSelect(Option<NDCQueryExecution<'n, 's>>),
    /// NDC mutation to be executed
    NDCMutationExecution(NDCMutationExecution<'n, 's>),
}

#[derive(Debug)]
pub struct NDCQueryExecution<'n, 's> {
    pub execution_tree: ExecutionTree<'s>,
    pub execution_span_attribute: String,
    pub field_span_attribute: String,
    pub process_response_as: ProcessResponseAs<'s>,
    pub selection_set: &'n normalized_ast::SelectionSet<'s, GDS>,
}

#[derive(Debug)]
pub struct NDCMutationExecution<'n, 's> {
    pub query: ndc::models::MutationRequest,
    pub join_locations: JoinLocations<(RemoteJoin<'s>, JoinId)>,
    pub data_connector: &'s resolved::data_connector::DataConnector,
    pub execution_span_attribute: String,
    pub field_span_attribute: String,
    pub process_response_as: ProcessResponseAs<'s>,
    pub selection_set: &'n normalized_ast::SelectionSet<'s, GDS>,
}

#[derive(Debug)]
pub struct ExecutionTree<'s> {
    pub root_node: ExecutionNode<'s>,
    pub remote_executions: JoinLocations<(RemoteJoin<'s>, JoinId)>,
}

#[derive(Debug)]
pub struct ExecutionNode<'s> {
    pub query: ndc::models::QueryRequest,
    pub data_connector: &'s resolved::data_connector::DataConnector,
}

#[derive(Debug)]
pub enum ProcessResponseAs<'s> {
    Object,
    Array,
    CommandResponse {
        command_name: &'s subgraph::Qualified<commands::CommandName>,
        type_container: &'s ast::TypeContainer<ast::TypeName>,
    },
}

pub fn generate_query_plan<'n, 's>(
    ir: &'s IndexMap<ast::Alias, root_field::RootField<'n, 's>>,
) -> Result<QueryPlan<'n, 's>, operations::Error> {
    let mut query_plan = IndexMap::new();
    for (alias, field) in ir.into_iter() {
        let field_plan = match field {
            root_field::RootField::QueryRootField(field_ir) => plan_query(field_ir),
            root_field::RootField::MutationRootField(field_ir) => plan_mutation(field_ir),
        }?;
        query_plan.insert(alias.clone(), field_plan);
    }
    Ok(query_plan)
}

fn plan_mutation<'n, 's>(
    ir: &'s root_field::MutationRootField<'n, 's>,
) -> Result<NodeQueryPlan<'n, 's>, operations::Error> {
    let plan = match ir {
        root_field::MutationRootField::TypeName { type_name } => NodeQueryPlan::TypeName {
            type_name: type_name.clone(),
        },
        root_field::MutationRootField::CommandRepresentation { ir, selection_set } => {
            let proc_name = match ir.ndc_source {
                commands::DataConnectorCommand::Procedure(ref proc_name) => Ok(proc_name),
                commands::DataConnectorCommand::Function(_) => {
                    Err(operations::InternalEngineError::InternalGeneric {
                        description: "unexpected function for command in Mutation root field"
                            .into(),
                    })
                }
            }?;
            let mut join_id_counter = MonotonicCounter::new();
            let (ndc_ir, join_locations) =
                operations::commands::ir_to_ndc_mutation_ir(proc_name, ir, &mut join_id_counter)?;
            let join_locations_ids = assign_with_join_ids(join_locations)?;
            NodeQueryPlan::NDCMutationExecution(NDCMutationExecution {
                query: ndc_ir,
                join_locations: join_locations_ids,
                data_connector: &ir.data_connector,
                selection_set,
                execution_span_attribute: "execute_command".into(),
                field_span_attribute: ir.field_name.to_string(),
                process_response_as: ProcessResponseAs::CommandResponse {
                    command_name: &ir.command_name,
                    type_container: &ir.type_container,
                },
            })
        }
    };
    Ok(plan)
}

fn plan_query<'n, 's>(
    ir: &'s root_field::QueryRootField<'n, 's>,
) -> Result<NodeQueryPlan<'n, 's>, operations::Error> {
    let mut counter = MonotonicCounter::new();
    let query_plan = match ir {
        root_field::QueryRootField::TypeName { type_name } => NodeQueryPlan::TypeName {
            type_name: type_name.clone(),
        },
        root_field::QueryRootField::TypeField {
            selection_set,
            schema,
            type_name,
            role: namespace,
        } => NodeQueryPlan::TypeField {
            selection_set,
            schema,
            type_name: type_name.clone(),
            role: namespace.clone(),
        },
        root_field::QueryRootField::SchemaField {
            role: namespace,
            selection_set,
            schema,
        } => NodeQueryPlan::SchemaField {
            role: namespace.clone(),
            selection_set,
            schema,
        },
        root_field::QueryRootField::ModelSelectOne { ir, selection_set } => {
            let execution_tree = generate_execution_tree(&ir.model_selection)?;
            NodeQueryPlan::NDCQueryExecution(NDCQueryExecution {
                execution_tree,
                selection_set,
                execution_span_attribute: "execute_model_select_one".into(),
                field_span_attribute: ir.field_name.to_string(),
                process_response_as: ProcessResponseAs::Object,
            })
        }

        root_field::QueryRootField::ModelSelectMany { ir, selection_set } => {
            let execution_tree = generate_execution_tree(&ir.model_selection)?;
            NodeQueryPlan::NDCQueryExecution(NDCQueryExecution {
                execution_tree,
                selection_set,
                execution_span_attribute: "execute_model_select_many".into(),
                field_span_attribute: ir.field_name.to_string(),
                process_response_as: ProcessResponseAs::Array,
            })
        }
        root_field::QueryRootField::NodeSelect(optional_ir) => match optional_ir {
            Some(ir) => {
                let execution_tree = generate_execution_tree(&ir.model_selection)?;
                NodeQueryPlan::RelayNodeSelect(Some(NDCQueryExecution {
                    execution_tree,
                    selection_set: &ir.selection_set,
                    execution_span_attribute: "execute_node".into(),
                    field_span_attribute: "node".into(),
                    process_response_as: ProcessResponseAs::Object,
                }))
            }
            None => NodeQueryPlan::RelayNodeSelect(None),
        },
        root_field::QueryRootField::CommandRepresentation { ir, selection_set } => {
            let function_name = match ir.ndc_source {
                commands::DataConnectorCommand::Function(ref function_name) => Ok(function_name),
                commands::DataConnectorCommand::Procedure(_) => {
                    Err(operations::InternalEngineError::InternalGeneric {
                        description: "unexpected procedure for command in Query root field".into(),
                    })
                }
            }?;
            let (ndc_ir, join_locations) =
                operations::commands::ir_to_ndc_query_ir(function_name, ir, &mut counter)?;
            let join_locations_ids = assign_with_join_ids(join_locations)?;
            let execution_tree = ExecutionTree {
                root_node: ExecutionNode {
                    query: ndc_ir,
                    data_connector: &ir.data_connector,
                },
                remote_executions: join_locations_ids,
            };
            NodeQueryPlan::NDCQueryExecution(NDCQueryExecution {
                execution_tree,
                selection_set,
                execution_span_attribute: "execute_command".into(),
                field_span_attribute: ir.field_name.to_string(),
                process_response_as: ProcessResponseAs::CommandResponse {
                    command_name: &ir.command_name,
                    type_container: &ir.type_container,
                },
            })
        }
    };
    Ok(query_plan)
}

fn generate_execution_tree<'s>(
    ir: &'s ModelSelection,
) -> Result<ExecutionTree<'s>, operations::Error> {
    let mut counter = MonotonicCounter::new();
    let (ndc_ir, join_locations) = operations::model_selection::ir_to_ndc_ir(ir, &mut counter)?;
    let join_locations_with_ids = assign_with_join_ids(join_locations)?;
    Ok(ExecutionTree {
        root_node: ExecutionNode {
            query: ndc_ir,
            data_connector: ir.data_connector,
        },
        remote_executions: join_locations_with_ids,
    })
}

fn assign_with_join_ids(
    join_locations: JoinLocations<RemoteJoin<'_>>,
) -> Result<JoinLocations<(RemoteJoin<'_>, JoinId)>, operations::Error> {
    let mut state = RemoteJoinCounter::new();
    let join_ids = assign_join_ids(&join_locations, &mut state);
    zip_with_join_ids(join_locations, join_ids)
}

fn zip_with_join_ids(
    join_locations: JoinLocations<RemoteJoin<'_>>,
    mut join_ids: JoinLocations<JoinId>,
) -> Result<JoinLocations<(RemoteJoin<'_>, JoinId)>, operations::Error> {
    let mut new_locations = HashMap::new();
    for (key, location) in join_locations.locations {
        let join_id_location =
            join_ids
                .locations
                .remove(&key)
                .ok_or(InternalEngineError::InternalGeneric {
                    description: "unexpected; could not find {key} in join ids tree".to_string(),
                })?;
        let new_node = match (location.join_node, join_id_location.join_node) {
            (Some(rj), Some(join_id)) => Some((rj, join_id)),
            _ => None,
        };
        let new_rest = zip_with_join_ids(location.rest, join_id_location.rest)?;
        new_locations.insert(
            key,
            Location {
                join_node: new_node,
                rest: new_rest,
            },
        );
    }
    Ok(JoinLocations {
        locations: new_locations,
    })
}

/// Once `JoinLocations<RemoteJoin>` is generated, traverse the tree and assign
/// join ids. All the join nodes (`RemoteJoin`) that are equal, are assigned the
/// same join id.
fn assign_join_ids<'s>(
    join_locations: &'s JoinLocations<RemoteJoin<'s>>,
    state: &mut RemoteJoinCounter<'s>,
) -> JoinLocations<JoinId> {
    let new_locations = join_locations
        .locations
        .iter()
        .map(|(key, location)| {
            let new_node = location
                .join_node
                .as_ref()
                .map(|join_node| assign_join_id(join_node, state));
            let new_location = Location {
                join_node: new_node.to_owned(),
                rest: assign_join_ids(&location.rest, state),
            };
            (key.to_string(), new_location)
        })
        .collect::<HashMap<_, _>>();
    JoinLocations {
        locations: new_locations,
    }
}

/// We use an associative list and check for equality of `RemoteJoin` to
/// generate it's `JoinId`. This is because `Hash` trait is not implemented for
/// `ndc::models::QueryRequest`
fn assign_join_id<'s>(join_node: &'s RemoteJoin<'s>, state: &mut RemoteJoinCounter<'s>) -> JoinId {
    let found = state.remote_joins.iter().find(|(rj, _id)| rj == &join_node);

    match found {
        None => {
            let next_id = state.counter.get_next();
            state.remote_joins.push((join_node, next_id));
            next_id
        }
        Some((_rj, id)) => *id,
    }
}

struct RemoteJoinCounter<'s> {
    remote_joins: Vec<(&'s RemoteJoin<'s>, JoinId)>,
    counter: MonotonicCounter,
}

impl<'s> RemoteJoinCounter<'s> {
    pub fn new() -> RemoteJoinCounter<'s> {
        RemoteJoinCounter {
            remote_joins: Vec::new(),
            counter: MonotonicCounter::new(),
        }
    }
}
