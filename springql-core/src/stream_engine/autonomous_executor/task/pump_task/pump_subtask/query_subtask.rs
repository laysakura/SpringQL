// Copyright (c) 2021 TOYOTA MOTOR CORPORATION. Licensed under MIT OR Apache-2.0.

use std::sync::Arc;

use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::{EdgeRef, IntoNodeReferences},
};

use self::query_subtask_node::QuerySubtaskNode;
use crate::{
    error::Result,
    pipeline::{name::ColumnName, stream_model::StreamModel},
    stream_engine::{
        autonomous_executor::row::{
            column::stream_column::StreamColumns, column_values::ColumnValues, Row,
        },
        command::query_plan::{child_direction::ChildDirection, QueryPlan},
    },
    stream_engine::{
        autonomous_executor::{
            performance_metrics::metrics_update_command::metrics_update_by_task_execution::InQueueMetricsUpdateByTaskExecution,
            task::{task_context::TaskContext, tuple::Tuple},
        },
        SqlValue,
    },
};

mod query_subtask_node;

/// Process input row 1-by-1.
#[derive(Debug)]
pub(in crate::stream_engine::autonomous_executor) struct QuerySubtask {
    tree: DiGraph<QuerySubtaskNode, ChildDirection>,
}

#[derive(Clone, Debug)]
pub(in crate::stream_engine::autonomous_executor) struct SqlValues(Vec<SqlValue>);
impl SqlValues {
    /// ```text
    /// column_order = (c2, c3, c1)
    /// stream_shape = (c1, c2, c3)
    ///
    /// |
    /// v
    ///
    /// (fields[1], fields[2], fields[0])
    /// ```
    ///
    /// # Panics
    ///
    /// - Tuple fields and column_order have different length.
    /// - Type mismatch between `self.fields` (ordered) and `stream_shape`
    /// - Duplicate column names in `column_order`
    pub(in crate::stream_engine::autonomous_executor) fn into_row(
        self,
        stream_model: Arc<StreamModel>,
        column_order: Vec<ColumnName>,
    ) -> Row {
        assert_eq!(self.0.len(), column_order.len());

        let column_values = self.mk_column_values(column_order);
        let stream_columns = StreamColumns::new(stream_model, column_values)
            .expect("type or shape mismatch? must be checked on pump creation");
        Row::new(stream_columns)
    }

    fn mk_column_values(self, column_order: Vec<ColumnName>) -> ColumnValues {
        let mut column_values = ColumnValues::default();

        for (column_name, value) in column_order.into_iter().zip(self.0.into_iter()) {
            column_values
                .insert(column_name, value)
                .expect("duplicate column name");
        }

        column_values
    }
}

#[derive(Debug, new)]
pub(in crate::stream_engine::autonomous_executor) struct QuerySubtaskOut {
    pub(in crate::stream_engine::autonomous_executor) values_seq: Vec<SqlValues>,
    pub(in crate::stream_engine::autonomous_executor) in_queue_metrics_update:
        InQueueMetricsUpdateByTaskExecution,
}

impl From<&QueryPlan> for QuerySubtask {
    fn from(query_plan: &QueryPlan) -> Self {
        let plan_tree = query_plan.as_petgraph();
        let subtask_tree = plan_tree.map(
            |_, op| QuerySubtaskNode::from(op),
            |_, child_direction| child_direction.clone(),
        );
        Self { tree: subtask_tree }
    }
}

impl QuerySubtask {
    /// # Returns
    ///
    /// None when input queue does not exist or is empty.
    ///
    /// # Failures
    ///
    /// TODO
    pub(in crate::stream_engine::autonomous_executor) fn run(
        &self,
        context: &TaskContext,
    ) -> Result<Option<QuerySubtaskOut>> {
        let mut next_idx = self.leaf_node_idx();

        match self.run_leaf(next_idx, context) {
            None => Ok(None),
            Some(leaf_query_subtask_out) => {
                let mut next_tuples = leaf_query_subtask_out.values_seq;
                while let Some(parent_idx) = self.parent_node_idx(next_idx) {
                    next_idx = parent_idx;
                    next_tuples = next_tuples
                        .into_iter()
                        .map(|next_tuple| self.run_non_leaf(next_idx, next_tuple))
                        .collect::<Result<Vec<Vec<_>>>>()?
                        .concat();
                }

                Ok(Some(QuerySubtaskOut::new(
                    next_tuples,
                    leaf_query_subtask_out.in_queue_metrics_update, // leaf subtask decides in queue metrics change
                )))
            }
        }
    }

    /// # Returns
    ///
    /// Although many subtasks return single tuple, selection subtask may return empty (filtered-out) and window subtask may return multiple tuples.
    fn run_non_leaf(&self, subtask_idx: NodeIndex, child_tuple: Tuple) -> Result<Vec<Tuple>> {
        let subtask = self.tree.node_weight(subtask_idx).expect("must be found");
        match subtask {
            QuerySubtaskNode::Projection(projection_subtask) => {
                let tuple = projection_subtask.run(child_tuple)?;
                Ok(vec![tuple])
            }
            QuerySubtaskNode::EvalValueExpr(eval_value_expr_subtask) => {
                let tuple = eval_value_expr_subtask.run(child_tuple)?;
                Ok(vec![tuple])
            }
            QuerySubtaskNode::Collect(_) => unreachable!(),
        }
    }

    /// # Returns
    ///
    /// None when input queue does not exist or is empty.
    fn run_leaf(&self, subtask_idx: NodeIndex, context: &TaskContext) -> Option<QuerySubtaskOut> {
        let subtask = self.tree.node_weight(subtask_idx).expect("must be found");
        match subtask {
            QuerySubtaskNode::Collect(collect_subtask) => collect_subtask.run(context),
            _ => unreachable!(),
        }
    }

    fn leaf_node_idx(&self) -> NodeIndex {
        self.tree
            .node_references()
            .find_map(|(idx, _)| {
                self.tree
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                    .next()
                    .is_none()
                    .then(|| idx)
            })
            .expect("asserting only 1 leaf currently. TODO multiple leaves")
    }

    fn parent_node_idx(&self, node_idx: NodeIndex) -> Option<NodeIndex> {
        self.tree
            .edges_directed(node_idx, petgraph::Direction::Incoming)
            .next()
            .map(|parent_edge| parent_edge.source())
    }
}
