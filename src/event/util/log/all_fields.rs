use super::{Atom, Value};
use serde::{Serialize, Serializer};
use std::{
    collections::{btree_map, BTreeMap},
    iter, slice,
};

/// Iterates over all paths in form "a.b[0].c[1]" in alphabetical order
/// and their corresponding values.
pub fn all_fields<'a>(
    fields: &'a BTreeMap<Atom, Value>,
) -> impl Iterator<Item = (Atom, &'a Value)> + Serialize {
    FieldsIter::new(fields)
}

#[derive(Clone)]
enum LeafIter<'a> {
    Map(btree_map::Iter<'a, Atom, Value>),
    Array(iter::Enumerate<slice::Iter<'a, Value>>),
}

#[derive(Clone)]
enum Node<'a> {
    Key(&'a Atom),
    Index(usize),
}

#[derive(Clone)]
struct FieldsIter<'a> {
    stack: Vec<LeafIter<'a>>,
    nodes: Vec<Node<'a>>,
}

impl<'a> FieldsIter<'a> {
    fn new(fields: &'a BTreeMap<Atom, Value>) -> FieldsIter<'a> {
        FieldsIter {
            stack: vec![LeafIter::Map(fields.iter())],
            nodes: vec![],
        }
    }

    fn push(&mut self, value: &'a Value, node: Node<'a>) -> Option<&'a Value> {
        match value {
            Value::Map(map) => {
                self.stack.push(LeafIter::Map(map.iter()));
                self.nodes.push(node);
                None
            }
            Value::Array(array) => {
                self.stack.push(LeafIter::Array(array.iter().enumerate()));
                self.nodes.push(node);
                None
            }
            _ => Some(value),
        }
    }

    fn pop(&mut self) {
        self.stack.pop();
        self.nodes.pop();
    }

    fn make_path(&mut self, node: Node<'a>) -> Atom {
        let mut res = String::new();
        let mut nodes_iter = self.nodes.iter().chain(iter::once(&node)).peekable();
        loop {
            match nodes_iter.next() {
                None => return Atom::from(res),
                Some(Node::Key(key)) => res.push_str(&key),
                Some(Node::Index(index)) => res.push_str(&format!("[{}]", index)),
            }
            if let Some(Node::Key(_)) = nodes_iter.peek() {
                res.push('.');
            }
        }
    }
}

impl<'a> Iterator for FieldsIter<'a> {
    type Item = (Atom, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.last_mut() {
                None => return None,
                Some(LeafIter::Map(map_iter)) => match map_iter.next() {
                    None => self.pop(),
                    Some((key, value)) => {
                        if let Some(scalar_value) = self.push(value, Node::Key(key)) {
                            return Some((self.make_path(Node::Key(key)), scalar_value));
                        }
                    }
                },
                Some(LeafIter::Array(array_iter)) => match array_iter.next() {
                    None => self.pop(),
                    Some((index, value)) => {
                        if let Some(scalar_value) = self.push(value, Node::Index(index)) {
                            return Some((self.make_path(Node::Index(index)), scalar_value));
                        }
                    }
                },
            };
        }
    }
}

impl<'a> Serialize for FieldsIter<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_map(self.clone())
    }
}

#[cfg(test)]
mod test {
    use super::super::test::fields_from_json;
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_simple() {
        let fields = fields_from_json(json!({
            "field2": 3,
            "field1": 4,
            "field3": 5
        }));
        let expected: Vec<_> = vec![
            ("field1", &Value::Integer(4)),
            ("field2", &Value::Integer(3)),
            ("field3", &Value::Integer(5)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();

        let collected: Vec<_> = all_fields(&fields).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn keys_nested() {
        let fields = fields_from_json(json!({
            "a": {
                "b": {
                    "c": 5
                },
                "a": 4,
                "array": [null, 3, {
                    "x": 1
                }, [2]]
            }
        }));
        let expected: Vec<_> = vec![
            ("a.a", &Value::Integer(4)),
            ("a.array[0]", &Value::Null),
            ("a.array[1]", &Value::Integer(3)),
            ("a.array[2].x", &Value::Integer(1)),
            ("a.array[3][0]", &Value::Integer(2)),
            ("a.b.c", &Value::Integer(5)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();

        let collected: Vec<_> = all_fields(&fields).collect();
        assert_eq!(collected, expected);
    }
}
