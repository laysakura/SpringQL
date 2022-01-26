use crate::pipeline::{field::field_pointer::FieldPointer, name::FieldAlias};

/// Window operation parameters
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum WindowOperationParameter {
    /// TODO [support complex expression with aggregations](https://gh01.base.toyota-tokyo.tech/SpringQL-internal/SpringQL/issues/152)
    ///
    /// ```sql
    /// SELECT group_by, aggregate_function(aggregated) AS aggregated_alias
    ///   FROM s
    ///   GROUP BY group_by
    ///   SLIDING WINDOW ...;
    /// ```
    Aggregation {
        group_by: FieldPointer,
        aggregated: FieldPointer,
        aggregated_alias: FieldAlias,
        aggregate_function: AggregateFunctionParameter,
    },
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum AggregateFunctionParameter {
    Avg,
}
