use crate::{
    diagnostic::{self, Note},
    parser::{self, ParsedExpression, Parser},
    state, value, Diagnostic, Expression, Function, TypeDef,
};

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum Error {
    #[error("unable to parse program")]
    Parse(#[from] parser::Error),

    #[error("unexpected return value")]
    ReturnValue { want: value::Kind, got: value::Kind },

    #[error("expected to be infallible, but is not")]
    Fallible,
}

/// The constraint applied to the result of a program.
pub struct TypeConstraint {
    /// The type definition constraint for the program.
    pub type_def: TypeDef,

    /// If set to `true`, then a program that can return "any" value is
    /// considered to be valid.
    ///
    /// Note that the configured `type_def.kind` value still holds when a
    /// program returns anything other than any.
    ///
    /// Meaning, if a program returns a boolean or a string, and the constraint
    /// is set to return a float, and `allow_any` is set to `true`, the
    /// constraint will fail.
    ///
    /// However, for the same configuration, if the program returns "any" value,
    /// the constraint holds, unless `allow_any` is set to `false`.
    pub allow_any: bool,
}

/// The program to execute.
///
/// This object is passed to [`Runtime::execute`](crate::Runtime::execute).
///
/// You can create a program using [`Program::from_str`]. The provided string
/// will be parsed. If parsing fails, an [`Error`] is returned.
#[derive(Debug, Clone)]
pub struct Program<'a> {
    pub(crate) source: &'a str,
    pub(crate) expressions: Vec<ParsedExpression>,
}

impl<'a> Program<'a> {
    pub fn new(
        source: &'a str,
        function_definitions: &[Box<dyn Function>],
        constraint: Option<TypeConstraint>,
        allow_regex_return: bool, // TODO: move this into a builder pattern
    ) -> diagnostic::Result<Self> {
        let mut state = state::Compiler::default();
        let parser = Parser::new(function_definitions, &mut state, allow_regex_return);

        let (expressions, mut diagnostics) = parser.program_from_str(source)?;

        // optional type constraint checking
        if let Some(constraint) = constraint {
            let mut type_defs = expressions
                .iter()
                .map(|e| e.type_def(&state))
                .collect::<Vec<_>>();

            let program_def = type_defs.pop().unwrap_or(TypeDef {
                fallible: true,
                kind: value::Kind::Null,
                ..Default::default()
            });

            if !constraint.type_def.contains(&program_def)
                && (!program_def.kind.is_all() || !constraint.allow_any)
            {
                let want = constraint.type_def.kind;
                let got = program_def.kind;

                let span = expressions.last().map(|e| e.span()).unwrap_or_default();

                diagnostics.push(
                    Diagnostic::error("unexpected return value")
                        .with_primary(format!("got: {}", got), span)
                        .with_context(format!("expected: {}", want), span),
                );
            }
        }

        // TODO: this should point to the exact expression that is fallible,
        // not the entire root expression.
        expressions
            .iter()
            .filter(|e| e.type_def(&state).is_fallible())
            .for_each(|e| {
                diagnostics.push(
                    Diagnostic::error("unhandled error")
                        .with_primary("expression can result in runtime error", e.span())
                        .with_context("handle the error case to ensure runtime success", e.span())
                        .with_note(Note::SeeErrDocs),
                )
            });

        if diagnostics.is_err() {
            return Err(diagnostics);
        }

        Ok((
            Self {
                source,
                expressions,
            },
            diagnostics,
        ))
    }

    pub fn expressions(&self) -> &[ParsedExpression] {
        &self.expressions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value;
    use std::error::Error;

    #[test]
    fn program_test() {
        use value::Kind;

        let cases = vec![
            (
                ".foo",
                None,
                Ok(()),
            ),
            // "any" value is allowed
            (
                ".foo",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: true,
                        kind: Kind::Boolean,
                        ..Default::default()
                    },
                    allow_any: true,
                }),
                Ok(()),
            ),
            // "any" value is allowed, but resolves to non-allowed kind
            (
                "42",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: false,
                        kind: Kind::Boolean,
                        ..Default::default()
                    },
                    allow_any: true,
                }),
                Err("expected to resolve to boolean value, but instead resolves to integer value"),
            ),
            // "any" value is disallowed, and resolves to any
            (
                ".foo",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: true,
                        kind: Kind::Boolean,
                        ..Default::default()
                    },
                    allow_any: false,
                }),
                Err("expected to resolve to boolean value, but instead resolves to any value"),
            ),
            // The final expression is infallible, but the first one isn't, so
            // this isn't allowed.
            (
                ".foo\ntrue",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: false,
                        ..Default::default()
                    },
                    allow_any: false,
                }),
                Err("expected to be infallible, but is not"),
            ),
            (
                ".foo",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: false,
                        ..Default::default()
                    },
                    allow_any: false,
                }),
                Err("expected to resolve to any value, but instead resolves to an error, or any value"),
            ),
            (
                ".foo",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: false,
                        kind: Kind::Bytes,
                        ..Default::default()
                    },
                    allow_any: false,
                }),
                Err("expected to resolve to string value, but instead resolves to an error, or any value"),
            ),
            (
                "false || 2",
                Some(TypeConstraint {
                    type_def: TypeDef {
                        fallible: false,
                        kind: Kind::Bytes | Kind::Float,
                        ..Default::default()
                    },
                    allow_any: false,
                }),
                Err("expected to resolve to string or float values, but instead resolves to integer or boolean values"),
            ),
        ];

        for (source, constraint, expect) in cases {
            let program = Program::new(source, &[], constraint, false)
                .map(|_| ())
                .map_err(|e| {
                    e.source()
                        .and_then(|e| e.source().map(|e| e.to_string()))
                        .unwrap()
                });

            assert_eq!(program, expect.map_err(ToOwned::to_owned));
        }
    }
}
