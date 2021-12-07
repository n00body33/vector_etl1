use bytes::Bytes;
use datadog_filter::{
    regex::{wildcard_regex, word_regex},
    Filter, Matcher, Resolver, Run,
};
use datadog_search_syntax::{Comparison, ComparisonValue, Field};
use vector_core::event::{LogEvent, Value};

#[derive(Default, Clone)]
pub struct EventFilter;

/// Uses the default `Resolver`, to build a `Vec<Field>`.
impl Resolver for EventFilter {}

impl Filter<LogEvent> for EventFilter {
    fn exists(&self, field: Field) -> Box<dyn Matcher<LogEvent>> {
        match field {
            Field::Tag(tag) => {
                let starts_with = format!("{}:", tag);

                Run::boxed(move |log: &LogEvent| match log.get("tags") {
                    Some(Value::Array(v)) => v.iter().any(|v| {
                        let bytes = v.as_bytes();
                        let str_value = String::from_utf8_lossy(&bytes);

                        // The tag matches using either 'key' or 'key:value' syntax.
                        str_value == tag || str_value.starts_with(&starts_with)
                    }),
                    _ => false,
                })
            }
            // Literal field 'tags' needs to be compared by key.
            Field::Reserved(f) if f == "tags" => {
                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Array(v)) => v.iter().any(|v| {
                        let bytes = v.as_bytes();
                        let str_value = String::from_utf8_lossy(&bytes);

                        str_value == f
                    }),
                    _ => false,
                })
            }
            Field::Default(f) | Field::Facet(f) | Field::Reserved(f) => {
                Run::boxed(move |log: &LogEvent| log.get(&f).is_some())
            }
        }
    }

    fn equals(&self, field: Field, to_match: &str) -> Box<dyn Matcher<LogEvent>> {
        match field {
            // Default fields are compared by word boundary.
            Field::Default(f) => {
                let re = word_regex(to_match);

                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Bytes(val)) => re.is_match(&String::from_utf8_lossy(val)),
                    _ => false,
                })
            }
            // A literal "tags" field should match by key.
            Field::Reserved(f) if f == "tags" => {
                let to_match = to_match.to_owned();

                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Array(v)) => {
                        v.contains(&Value::Bytes(Bytes::copy_from_slice(to_match.as_bytes())))
                    }
                    _ => false,
                })
            }
            // Individual tags are compared by element key:value.
            Field::Tag(tag) => {
                let value_bytes = Value::Bytes(format!("{}:{}", tag, to_match).into());

                Run::boxed(move |log: &LogEvent| match log.get("tags") {
                    Some(Value::Array(v)) => v.contains(&value_bytes),
                    _ => false,
                })
            }
            // Everything else is matched by string equality.
            Field::Reserved(f) | Field::Facet(f) => {
                let to_match = to_match.to_owned();

                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Bytes(v)) => {
                        let str_value = String::from_utf8_lossy(&v);

                        str_value == to_match
                    }
                    _ => false,
                })
            }
        }
    }

    fn prefix(&self, field: Field, prefix: &str) -> Box<dyn Matcher<LogEvent>> {
        match field {
            // Default fields are matched by word boundary.
            Field::Default(f) => {
                let re = word_regex(&format!("{}*", prefix));

                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Bytes(v)) => re.is_match(&String::from_utf8_lossy(&v)),
                    _ => false,
                })
            }
            // Tags are recursed until a match is found.
            Field::Tag(tag) => {
                let starts_with = format!("{}:{}", tag, prefix);

                Run::boxed(move |log: &LogEvent| match log.get("tags") {
                    Some(Value::Array(v)) => v.iter().any(|v| match v {
                        Value::Bytes(v) => String::from_utf8_lossy(v).starts_with(&starts_with),
                        _ => false,
                    }),
                    _ => false,
                })
            }
            // All other field types are compared by complete value.
            Field::Reserved(f) | Field::Facet(f) => {
                let prefix = prefix.to_owned();

                Run::boxed(move |log: &LogEvent| match log.get(&f) {
                    Some(Value::Bytes(v)) => String::from_utf8_lossy(v).starts_with(&prefix),
                    _ => false,
                })
            }
        }
    }

    fn wildcard(&self, field: Field, wildcard: &str) -> Box<dyn Matcher<LogEvent>> {
        todo!()
    }

    fn compare(
        &self,
        field: Field,
        comparator: Comparison,
        comparison_value: ComparisonValue,
    ) -> Box<dyn Matcher<LogEvent>> {
        todo!()
    }
}
