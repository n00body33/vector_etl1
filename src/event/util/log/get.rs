use super::{Atom, PathIter, PathNode, Value};
use std::collections::BTreeMap;

/// Returns a reference to a field value specified by the given path.
pub fn get<'a>(fields: &'a BTreeMap<Atom, Value>, path: &str) -> Option<&'a Value> {
    let mut path_iter = PathIter::new(path);

    match path_iter.next() {
        Some(PathNode::Key(key)) => match fields.get(&key) {
            None => None,
            Some(value) => get_value(value, path_iter),
        },
        _ => None,
    }
}

fn get_value<'a, I>(mut value: &'a Value, mut path_iter: I) -> Option<&'a Value>
where
    I: Iterator<Item = PathNode>,
{
    loop {
        match (path_iter.next(), value) {
            (None, _) => return Some(value),
            (Some(PathNode::Key(ref key)), Value::Map(map)) => match map.get(key) {
                None => return None,
                Some(nested_value) => {
                    value = nested_value;
                }
            },
            (Some(PathNode::Index(index)), Value::Array(array)) => match array.get(index) {
                None => return None,
                Some(nested_value) => {
                    value = nested_value;
                }
            },
            _ => return None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::test::fields_from_json;
    use super::*;
    use serde_json::json;

    #[test]
    fn get_simple() {
        let fields = fields_from_json(json!({
            "field": 123
        }));
        assert_eq!(get(&fields, "field"), Some(&Value::Integer(123)));
    }

    #[test]
    fn get_nested() {
        let fields = fields_from_json(json!({
            "a": {
                "b": {
                    "c": 5
                },
                "d": 4,
                "array": [null, 3, {
                    "x": 1
                }, [2]]
            }
        }));
        let queries = [
            ("a.b.c", Some(Value::Integer(5))),
            ("a.d", Some(Value::Integer(4))),
            ("a.array[1]", Some(Value::Integer(3))),
            ("a.array[2].x", Some(Value::Integer(1))),
            ("a.array[3][0]", Some(Value::Integer(2))),
            ("a.array[3][1]", None),
            ("a.x", None),
            ("z", None),
            (".123", None),
            ("", None),
        ];

        for (query, expected) in queries.iter() {
            assert_eq!(get(&fields, query), expected.as_ref(), "{}", query);
        }
    }
}
