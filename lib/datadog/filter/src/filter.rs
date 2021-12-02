use super::{Fielder, Matcher, Run};
use datadog_search_syntax::{BooleanType, Comparison, ComparisonValue, Field, QueryNode};
use dyn_clone::{clone_trait_object, DynClone};

/// A `Filter` is a generic type that contains methods that are invoked by the `build_filter`
/// function. Each method returns a heap-allocated `Matcher<V>` (typically a closure) containing
/// logic to determine whether the value matches the filter. A filter is intended to be side-effect
/// free and idempotent, and so only receives an immutable reference to self.
pub trait Filter<'a, V: std::fmt::Debug + Send + Sync + Clone + 'static>: DynClone {
    /// Determine whether a field value exists.
    fn exists(&'a self, field: Field) -> Box<dyn Matcher<V>>;

    /// Determine whether a field value equals `to_match`.
    fn equals(&'a self, field: Field, to_match: &str) -> Box<dyn Matcher<V>>;

    /// Determine whether a value starts with a prefix.
    fn prefix(&'a self, field: Field, prefix: &str) -> Box<dyn Matcher<V>>;

    /// Determine whether a value matches a wilcard.
    fn wildcard(&'a self, field: Field, wildcard: &str) -> Box<dyn Matcher<V>>;

    /// Compare a field value against `comparison_value`, using one of the `comparator` operators.
    fn compare(
        &'a self,
        field: Field,
        comparator: Comparison,
        comparison_value: ComparisonValue,
    ) -> Box<dyn Matcher<V>>;

    /// Determine whether a field value falls within a range. By default, this will use
    /// `self.compare` on both the lower and upper bound.
    fn range(
        &'a self,
        field: Field,
        lower: ComparisonValue,
        lower_inclusive: bool,
        upper: ComparisonValue,
        upper_inclusive: bool,
    ) -> Box<dyn Matcher<V>> {
        match (&lower, &upper) {
            // If both bounds are wildcards, just check that the field exists to catch the
            // special case for "tags".
            (ComparisonValue::Unbounded, ComparisonValue::Unbounded) => self.exists(field),
            // Unbounded lower.
            (ComparisonValue::Unbounded, _) => {
                let op = if upper_inclusive {
                    Comparison::Lte
                } else {
                    Comparison::Lt
                };

                self.compare(field, op, upper)
            }
            // Unbounded upper.
            (_, ComparisonValue::Unbounded) => {
                let op = if lower_inclusive {
                    Comparison::Gte
                } else {
                    Comparison::Gt
                };

                self.compare(field, op, lower)
            }
            // Definitive range.
            _ => {
                let lower_op = if lower_inclusive {
                    Comparison::Gte
                } else {
                    Comparison::Gt
                };

                let upper_op = if upper_inclusive {
                    Comparison::Lte
                } else {
                    Comparison::Lt
                };

                let lower_func = self.compare(field.clone(), lower_op, lower);
                let upper_func = self.compare(field, upper_op, upper);

                Run::boxed(move |value: &V| lower_func.run(value) && upper_func.run(value))
            }
        }
    }
}

clone_trait_object!(<V>Filter<'_, V>);

/// Returns a closure that negates the value of the provided `Matcher`.
fn not<V>(matcher: Box<dyn Matcher<V>>) -> Box<dyn Matcher<V>>
where
    V: std::fmt::Debug + Send + Sync + Clone + 'static,
{
    Run::boxed(move |value| !matcher.run(value))
}

/// Returns a closure that returns true if any of the vector of `Matcher<V>` return true.
fn any<V>(matchers: Vec<Box<dyn Matcher<V>>>) -> Box<dyn Matcher<V>>
where
    V: std::fmt::Debug + Send + Sync + Clone + 'static,
{
    Run::boxed(move |value| matchers.iter().any(|func| func.run(value)))
}

/// Returns a closure that returns true if all of the vector of `Matcher<V>` return true.
fn all<V>(matchers: Vec<Box<dyn Matcher<V>>>) -> Box<dyn Matcher<V>>
where
    V: std::fmt::Debug + Send + Sync + Clone + 'static,
{
    Run::boxed(move |value| matchers.iter().all(|func| func.run(value)))
}

/// Build a filter by parsing a Datadog Search Syntax `QueryNode`, and invoking the appropriate
/// method on a `Fielder` + `Filter` implementation to determine the matching logic. Each method
/// returns a `Matcher<V>` which is intended to be invoked at runtime. `F` should implement both
/// `Fielder` + `Filter` in order to applying any required caching which may affect the operation
/// of a filter method. This function is intended to be used at boot-time and NOT in a hot path!
pub fn build_filter<'a, V, F>(node: &'a QueryNode, f: &'a F) -> Box<dyn Matcher<V>>
where
    V: std::fmt::Debug + Send + Sync + Clone + 'static,
    F: Fielder + Filter<'a, V>,
{
    match node {
        QueryNode::MatchNoDocs => Box::new(false),
        QueryNode::MatchAllDocs => Box::new(true),
        QueryNode::AttributeExists { attr } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| f.exists(field))
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::AttributeMissing { attr } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| not(f.exists(field)))
                .collect::<Vec<_>>();

            all(matchers)
        }
        QueryNode::AttributeTerm { attr, value }
        | QueryNode::QuotedAttribute {
            attr,
            phrase: value,
        } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| f.equals(field, value))
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::AttributePrefix { attr, prefix } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| f.prefix(field, prefix))
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::AttributeWildcard { attr, wildcard } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| f.wildcard(field, wildcard))
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::AttributeComparison {
            attr,
            comparator,
            value,
        } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| f.compare(field, *comparator, value.clone()))
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::AttributeRange {
            attr,
            lower,
            lower_inclusive,
            upper,
            upper_inclusive,
        } => {
            let matchers = f
                .build_fields(attr)
                .into_iter()
                .map(|field| {
                    f.range(
                        field,
                        lower.clone(),
                        *lower_inclusive,
                        upper.clone(),
                        *upper_inclusive,
                    )
                })
                .collect::<Vec<_>>();

            any(matchers)
        }
        QueryNode::NegatedNode { node } => not(build_filter(node, f)),
        QueryNode::Boolean { oper, nodes } => {
            let funcs = nodes
                .iter()
                .map(|node| build_filter(node, f))
                .collect::<Vec<_>>();

            match oper {
                BooleanType::And => all(funcs),
                BooleanType::Or => any(funcs),
            }
        }
    }
}
