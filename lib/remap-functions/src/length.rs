use remap::prelude::*;
use std::convert::TryFrom;

#[derive(Clone, Copy, Debug)]
pub struct Length;

impl Function for Length {
    fn identifier(&self) -> &'static str {
        "length"
    }

    fn parameters(&self) -> &'static [Parameter] {
        &[Parameter {
            keyword: "value",
            accepts: |v| matches!(v, Value::Array(_) | Value::Map(_)),
            required: true,
        }]
    }

    fn compile(&self, mut arguments: ArgumentList) -> Result<Box<dyn Expression>> {
        let value = arguments.required("value")?.boxed();

        Ok(Box::new(LengthFn { value }))
    }
}

#[derive(Debug, Clone)]
struct LengthFn {
    value: Box<dyn Expression>,
}

impl Expression for LengthFn {
    fn execute(&self, state: &mut state::Program, object: &mut dyn Object) -> Result<Value> {
        use Value::*;

        let value = self.value.execute(state, object)?;

        match value {
            Array(v) => safe_i64(v.len()),
            Map(v) => safe_i64(v.len()),
            _ => Err("unsupported type".into()),
        }
    }

    fn type_def(&self, state: &state::Compiler) -> TypeDef {
        use value::Kind;

        self.value
            .type_def(state)
            .fallible_unless(Kind::Array | Kind::Map)
            .with_constraint(Kind::Integer)
    }
}

fn safe_i64(n: usize) -> Result<Value> {
    match i64::try_from(n) {
        Ok(n) => Ok(Value::from(n)),
        Err(_) => Err("64-bit integer overflow".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use value::Kind;

    test_type_def![
        value_map_infallible {
            expr: |_| LengthFn {
                value: map! {"foo": "bar", "baz": 27, "baq": false}.boxed()
            },
            def: TypeDef {
                fallible: false,
                kind: Kind::Integer,
                inner_type_def: Some(
                    TypeDef {
                        fallible: false,
                        kind: Kind::Bytes | Kind::Integer | Kind::Boolean,
                        inner_type_def: None
                    }
                    .boxed()
                )
            },
        }

        value_array_infallible {
            expr: |_| LengthFn {
                value: array!["foo", 127, false].boxed()
            },
            def: TypeDef {
                fallible: false,
                kind: Kind::Integer,
                inner_type_def: Some(
                    TypeDef {
                        fallible: false,
                        kind: Kind::Bytes | Kind::Integer | Kind::Boolean,
                        inner_type_def: None
                    }
                    .boxed()
                )
            },
        }

        value_string_fallible {
            expr: |_| LengthFn {
                value: lit!("this is fallible").boxed()
            },
            def: TypeDef {
                fallible: true,
                kind: Kind::Integer,
                inner_type_def: None
            },
        }
    ];

    test_function![
        length => Length;

        non_empty_map_value {
            args: func_args![value: map!["foo": "bar", "baz": true, "baq": array![1, 2, 3]]],
            want: Ok(value!(3)),
        }

        empty_map_value {
            args: func_args![value: map![]],
            want: Ok(value!(0)),
        }

        nested_map_value {
            args: func_args![value: map!["nested": map!["foo": "bar"]]],
            want: Ok(value!(1)),
        }

        non_empty_array_value {
            args: func_args![value: array![1, 2, 3, 4, true, "hello"]],
            want: Ok(value!(6)),
        }

        empty_array_value {
            args: func_args![value: array![]],
            want: Ok(value!(0)),
        }
    ];
}
