use lang_graphql as gql;
use lang_graphql::ast::common as ast;

use serde::Serialize;

use crate::schema::operations::{commands, relay, select_many, select_one};
use crate::schema::{Role, GDS};

#[derive(Serialize, Debug)]
pub enum RootField<'n, 's> {
    QueryRootField(QueryRootField<'n, 's>),
    MutationRootField(MutationRootField<'n, 's>),
}

/// IR of a query root field
#[derive(Serialize, Debug)]
pub enum QueryRootField<'n, 's> {
    // __typename field on query root
    TypeName {
        type_name: ast::TypeName,
    },
    // __schema field
    SchemaField {
        role: Role,
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        schema: &'s gql::schema::Schema<GDS>,
    },
    // __type field
    TypeField {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        schema: &'s gql::schema::Schema<GDS>,
        type_name: ast::TypeName,
        role: Role,
    },
    // Operation that selects a single row from a model
    ModelSelectOne {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        ir: select_one::ModelSelectOne<'s>,
    },
    // Operation that selects many rows from a model
    ModelSelectMany {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        ir: select_many::ModelSelectMany<'s>,
    },
    // Operation that selects a single row from the model corresponding
    // to the Global Id input.
    NodeSelect(Option<relay::NodeSelect<'n, 's>>),
    CommandRepresentation {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        ir: commands::CommandRepresentation<'s>,
    },
}

/// IR of a mutation root field
#[derive(Serialize, Debug)]
pub enum MutationRootField<'n, 's> {
    // __typename field on mutation root
    TypeName {
        type_name: ast::TypeName,
    },
    CommandRepresentation {
        selection_set: &'n gql::normalized_ast::SelectionSet<'s, GDS>,
        ir: commands::CommandRepresentation<'s>,
    },
}
