use std::{fmt, iter::Peekable, str::CharIndices};

use diagnostic::{DiagnosticError, Label, Span};
use ordered_float::NotNan;

use crate::template_string::{StringSegment, TemplateString};

pub type Tok<'input> = Token<&'input str>;
pub type SpannedResult<'input, Loc> = Result<Spanned<'input, Loc>, Error>;
pub type Spanned<'input, Loc> = (Loc, Tok<'input>, Loc);

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum Error {
    #[error("syntax error")]
    ParseError {
        span: Span,
        source: lalrpop_util::ParseError<usize, Token<String>, String>,
        dropped_tokens: Vec<(usize, Token<String>, usize)>,
    },

    #[error("reserved keyword")]
    ReservedKeyword {
        start: usize,
        keyword: String,
        end: usize,
    },

    #[error("invalid numeric literal")]
    NumericLiteral {
        start: usize,
        error: String,
        end: usize,
    },

    #[error("invalid string literal")]
    StringLiteral { start: usize },

    #[error("invalid literal")]
    Literal { start: usize },

    #[error("invalid escape character: \\{}", .ch.unwrap_or_default())]
    EscapeChar { start: usize, ch: Option<char> },

    #[error("unexpected parse error")]
    UnexpectedParseError(String),
}

impl DiagnosticError for Error {
    fn code(&self) -> usize {
        use Error::*;

        match self {
            ParseError { source, .. } => match source {
                lalrpop_util::ParseError::InvalidToken { .. } => 200,
                lalrpop_util::ParseError::ExtraToken { .. } => 201,
                lalrpop_util::ParseError::User { .. } => 202,
                lalrpop_util::ParseError::UnrecognizedToken { .. } => 203,
                lalrpop_util::ParseError::UnrecognizedEOF { .. } => 204,
            },
            ReservedKeyword { .. } => 205,
            NumericLiteral { .. } => 206,
            StringLiteral { .. } => 207,
            Literal { .. } => 208,
            EscapeChar { .. } => 209,
            UnexpectedParseError(..) => 210,
        }
    }

    fn labels(&self) -> Vec<Label> {
        use Error::*;

        fn update_expected(expected: Vec<String>) -> Vec<String> {
            expected
                .into_iter()
                .map(|expect| match expect.as_str() {
                    "LQuery" => r#""path literal""#.to_owned(),
                    _ => expect,
                })
                .collect::<Vec<_>>()
        }

        match self {
            ParseError { span, source, .. } => match source {
                lalrpop_util::ParseError::InvalidToken { location } => vec![Label::primary(
                    "invalid token",
                    Span::new(*location, *location + 1),
                )],
                lalrpop_util::ParseError::ExtraToken { token } => {
                    let (start, token, end) = token;
                    vec![Label::primary(
                        format!("unexpected extra token: {}", token),
                        Span::new(*start, *end),
                    )]
                }
                lalrpop_util::ParseError::User { error } => {
                    vec![Label::primary(format!("unexpected error: {}", error), span)]
                }
                lalrpop_util::ParseError::UnrecognizedToken { token, expected } => {
                    let (start, token, end) = token;
                    let span = Span::new(*start, *end);
                    let got = token.to_string();
                    let mut expected = update_expected(expected.clone());

                    // Temporary hack to improve error messages for `AnyIdent`
                    // parser rule.
                    let any_ident = [
                        r#""reserved identifier""#,
                        r#""else""#,
                        r#""false""#,
                        r#""null""#,
                        r#""true""#,
                        r#""if""#,
                    ];
                    let is_any_ident = any_ident.iter().all(|i| expected.contains(&i.to_string()));
                    if is_any_ident {
                        expected = expected
                            .into_iter()
                            .filter(|e| !any_ident.contains(&e.as_str()))
                            .collect::<Vec<_>>();
                    }

                    if token == &Token::RQuery {
                        return vec![
                            Label::primary("unexpected end of query path", span),
                            Label::context(
                                format!("expected one of: {}", expected.join(", ")),
                                span,
                            ),
                        ];
                    }

                    vec![
                        Label::primary(format!(r#"unexpected syntax token: "{}""#, got), span),
                        Label::context(format!("expected one of: {}", expected.join(", ")), span),
                    ]
                }
                lalrpop_util::ParseError::UnrecognizedEOF { location, expected } => {
                    let span = Span::new(*location, *location);
                    let expected = update_expected(expected.clone());

                    vec![
                        Label::primary("unexpected end of program", span),
                        Label::context(format!("expected one of: {}", expected.join(", ")), span),
                    ]
                }
            },

            ReservedKeyword { start, end, .. } => {
                let span = Span::new(*start, *end);

                vec![
                    Label::primary(
                        "this identifier name is reserved for future use in the language",
                        span,
                    ),
                    Label::context("use a different name instead", span),
                ]
            }

            NumericLiteral { start, error, end } => vec![Label::primary(
                format!("invalid numeric literal: {}", error),
                Span::new(*start, *end),
            )],

            StringLiteral { start } => vec![Label::primary(
                "invalid string literal",
                Span::new(*start, *start + 1),
            )],

            Literal { start } => vec![Label::primary(
                "invalid literal",
                Span::new(*start, *start + 1),
            )],

            EscapeChar { start, ch } => vec![Label::primary(
                format!(
                    "invalid escape character: {}",
                    ch.map(|ch| ch.to_string())
                        .unwrap_or_else(|| "none".to_string())
                ),
                Span::new(*start, *start + 1),
            )],

            UnexpectedParseError(string) => vec![Label::primary(string, Span::default())],
        }
    }
}

// -----------------------------------------------------------------------------
// lexer
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct Lexer<'input> {
    input: &'input str,
    chars: Peekable<CharIndices<'input>>,

    // state
    open_brackets: usize,
    open_braces: usize,
    open_parens: usize,

    /// Keep track of when the lexer is supposed to emit an `RQuery` token.
    ///
    /// For example:
    ///
    ///   [.foo].bar
    ///
    /// In this example, if `[` is at index `0`, then this value will contain:
    ///
    ///   [10, 5]
    ///
    /// Or:
    ///
    ///   [.foo].bar
    ///   ~~~~~~~~~~  0..10
    ///    ~~~~       1..5
    rquery_indices: Vec<usize>,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub enum Token<S> {
    Identifier(S),
    PathField(S),
    FunctionCall(S),
    Operator(S),

    // literals
    StringLiteral(StringLiteral<S>),
    RawStringLiteral(RawStringLiteral<S>),
    IntegerLiteral(i64),
    FloatLiteral(NotNan<f64>),
    RegexLiteral(S),
    TimestampLiteral(S),

    // Reserved for future use.
    ReservedIdentifier(S),

    InvalidToken(char),

    // keywords
    If,
    Else,
    Null,
    False,
    True,
    Abort,

    // tokens
    Colon,
    Comma,
    Dot,
    LBrace,
    LBracket,
    LParen,
    Newline,
    RBrace,
    RBracket,
    RParen,
    SemiColon,
    Underscore,
    Escape,

    Equals,
    MergeEquals,
    Bang,
    Question,

    /// The {L,R}Query token is an "instruction" token. It does not represent
    /// any character in the source, instead it represents the start or end of a
    /// sequence of tokens that together form a "query".
    ///
    /// Some examples:
    ///
    /// ```text
    /// .          => LQuery, Dot, RQuery
    /// .foo       => LQuery, Dot, Ident, RQuery
    /// foo.bar[2] => LQuery, Ident, Dot, Ident, LBracket, Integer, RBracket, RQuery
    /// foo().bar  => LQuery, FunctionCall, LParen, RParen, Dot, Ident, RQuery
    /// [1].foo    => LQuery, LBracket, Integer, RBracket, Dot, Ident, RQuery
    /// { .. }[0]  => LQuery, LBrace, ..., RBrace, LBracket, ... RBracket, RQuery
    /// ```
    ///
    /// The final example shows how the lexer does not care about the semantic
    /// validity of a query (as in, getting the index from an object does not
    /// work), it only signals that one exists.
    ///
    /// Some non-matching examples:
    ///
    /// ```text
    /// . foo      => Dot, Identifier
    /// foo() .a   => FunctionCall, LParen, RParen, LQuery, Dot, Ident, RQuery
    /// [1] [2]    => RBracket, Integer, LBracket, RBracket, Integer, RBracket
    /// ```
    ///
    /// The reason these tokens exist is to allow the parser to remain
    /// whitespace-agnostic, while still being able to distinguish between the
    /// above two groups of examples.
    LQuery,
    RQuery,
}

impl<S> Token<S> {
    pub(crate) fn map<R>(self, f: impl Fn(S) -> R) -> Token<R> {
        match self {
            Token::Identifier(s) => Token::Identifier(f(s)),
            Token::PathField(s) => Token::PathField(f(s)),
            Token::FunctionCall(s) => Token::FunctionCall(f(s)),
            Token::Operator(s) => Token::Operator(f(s)),

            Token::StringLiteral(StringLiteral(s)) => Token::StringLiteral(StringLiteral(f(s))),
            Token::RawStringLiteral(RawStringLiteral(s)) => {
                Token::RawStringLiteral(RawStringLiteral(f(s)))
            }

            Token::IntegerLiteral(s) => Token::IntegerLiteral(s),
            Token::FloatLiteral(s) => Token::FloatLiteral(s),
            Token::RegexLiteral(s) => Token::RegexLiteral(f(s)),
            Token::TimestampLiteral(s) => Token::TimestampLiteral(f(s)),

            Token::ReservedIdentifier(s) => Token::ReservedIdentifier(f(s)),

            Token::InvalidToken(s) => Token::InvalidToken(s),

            Token::Else => Token::Else,
            Token::False => Token::False,
            Token::If => Token::If,
            Token::Null => Token::Null,
            Token::True => Token::True,
            Token::Abort => Token::Abort,

            // tokens
            Token::Colon => Token::Colon,
            Token::Comma => Token::Comma,
            Token::Dot => Token::Dot,
            Token::LBrace => Token::LBrace,
            Token::LBracket => Token::LBracket,
            Token::LParen => Token::LParen,
            Token::Newline => Token::Newline,
            Token::RBrace => Token::RBrace,
            Token::RBracket => Token::RBracket,
            Token::RParen => Token::RParen,
            Token::SemiColon => Token::SemiColon,
            Token::Underscore => Token::Underscore,
            Token::Escape => Token::Escape,

            Token::Equals => Token::Equals,
            Token::MergeEquals => Token::MergeEquals,
            Token::Bang => Token::Bang,
            Token::Question => Token::Question,

            Token::LQuery => Token::LQuery,
            Token::RQuery => Token::RQuery,
        }
    }
}

impl<S> fmt::Display for Token<S>
where
    S: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Token::*;

        let s = match *self {
            Identifier(_) => "Identifier",
            PathField(_) => "PathField",
            FunctionCall(_) => "FunctionCall",
            Operator(_) => "Operator",
            StringLiteral(_) => "StringLiteral",
            RawStringLiteral(_) => "RawStringLiteral",
            IntegerLiteral(_) => "IntegerLiteral",
            FloatLiteral(_) => "FloatLiteral",
            RegexLiteral(_) => "RegexLiteral",
            TimestampLiteral(_) => "TimestampLiteral",
            ReservedIdentifier(_) => "ReservedIdentifier",
            InvalidToken(_) => "InvalidToken",

            Else => "Else",
            False => "False",
            If => "If",
            Null => "Null",
            True => "True",
            Abort => "Abort",

            // tokens
            Colon => "Colon",
            Comma => "Comma",
            Dot => "Dot",
            LBrace => "LBrace",
            LBracket => "LBracket",
            LParen => "LParen",
            Newline => "Newline",
            RBrace => "RBrace",
            RBracket => "RBracket",
            RParen => "RParen",
            SemiColon => "SemiColon",
            Underscore => "Underscore",
            Escape => "Escape",

            Equals => "Equals",
            MergeEquals => "MergeEquals",
            Bang => "Bang",
            Question => "Question",

            LQuery => "LQuery",
            RQuery => "RQuery",
        };

        s.fmt(f)
    }
}

impl<'input> Token<&'input str> {
    /// Returns either a literal, reserved, or generic identifier.
    fn ident(s: &'input str) -> Self {
        use Token::*;

        match s {
            "if" => If,
            "else" => Else,
            "true" => True,
            "false" => False,
            "null" => Null,
            "abort" => Abort,

            // reserved identifiers
            "array" | "bool" | "boolean" | "break" | "continue" | "do" | "emit" | "float"
            | "for" | "forall" | "foreach" | "all" | "each" | "any" | "try" | "undefined"
            | "int" | "integer" | "iter" | "object" | "regex" | "return" | "string"
            | "traverse" | "timestamp" | "duration" | "unless" | "walk" | "while" | "loop" => {
                ReservedIdentifier(s)
            }

            _ if s.contains('@') => PathField(s),

            _ => Identifier(s),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct StringLiteral<S>(pub S);

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct RawStringLiteral<S>(pub S);

impl StringLiteral<&str> {
    /// Takes the string and splits it into segments of literals and templates.
    /// A templated section is delimited by `{{..}}`. ``{{` can be escaped using
    /// `/{{.../}}`.
    pub fn template(&self, span: Span) -> TemplateString {
        let mut segments = Vec::new();

        let chars = self.0.chars().collect::<Vec<_>>();
        let mut template = false;
        let mut current = String::new();

        let mut pos = 0;
        while pos < chars.len() {
            match chars[pos] {
                '}' if template && chars.get(pos + 1) == Some(&'}') => {
                    // Handle closing template `}}`.
                    if !current.is_empty() {
                        let seg = std::mem::take(&mut current);
                        segments.push(StringSegment::Template(
                            seg.trim().to_string(),
                            Span::new(span.start() + pos - seg.len() - 1, span.start() + pos + 3),
                        ));
                    }
                    template = false;
                    pos += 2;
                }
                '\\' if !template
                    && chars.get(pos + 1) == Some(&'{')
                    && chars.get(pos + 2) == Some(&'{') =>
                {
                    // Handle open escape `/{{`.
                    current.push_str(r#"{{"#);
                    pos += 3;
                }
                '\\' if !template
                    && chars.get(pos + 1) == Some(&'}')
                    && chars.get(pos + 2) == Some(&'}') =>
                {
                    // Handle close escape
                    current.push_str(r#"}}"#);
                    pos += 3;
                }
                '{' if !template && chars.get(pos + 1) == Some(&'{') => {
                    // Handle start of template.
                    if !current.is_empty() {
                        let seg = std::mem::take(&mut current);
                        segments.push(StringSegment::Literal(unescape_string_literal(&seg)));
                    }
                    template = true;
                    pos += 2;
                }
                chr => {
                    current.push(chr);
                    pos += 1;
                }
            }
        }

        if !template && !current.is_empty() {
            segments.push(StringSegment::Literal(unescape_string_literal(&current)));
        }

        TemplateString(segments)
    }

    pub fn unescape(&self) -> String {
        unescape_string_literal(self.0)
    }
}

impl RawStringLiteral<&str> {
    pub fn unescape(&self) -> String {
        self.0.to_string()
    }
}

// -----------------------------------------------------------------------------
// lexing iterator
// -----------------------------------------------------------------------------

impl<'input> Iterator for Lexer<'input> {
    type Item = SpannedResult<'input, usize>;

    fn next(&mut self) -> Option<Self::Item> {
        use Token::*;

        loop {
            let start = self.next_index();

            // Check if we need to emit a `LQuery` token.
            //
            // We don't advance the internal iterator, because this token does not
            // represent a physical character, instead it is a boundary marker.
            match self.query_start(start) {
                Err(err) => return Some(Err(err)),
                Ok(true) => {
                    // dbg!("LQuery"); // NOTE: uncomment this for debugging
                    return Some(Ok(self.token2(start, start + 1, LQuery)));
                }
                Ok(false) => (),
            }

            // Check if we need to emit a `RQuery` token.
            //
            // We don't advance the internal iterator, because this token does not
            // represent a physical character, instead it is a boundary marker.
            if let Some(pos) = self.query_end(start) {
                // dbg!("RQuery"); // NOTE: uncomment this for debugging
                return Some(Ok(self.token2(pos, pos + 1, RQuery)));
            }

            // Advance the internal iterator and emit the next token, or loop
            // again if we encounter a token we want to ignore (e.g. whitespace).
            if let Some((start, ch)) = self.bump() {
                let result = match ch {
                    '"' => Some(self.string_literal(start)),

                    ';' => Some(Ok(self.token(start, SemiColon))),
                    '\n' => Some(Ok(self.token(start, Newline))),
                    '\\' => Some(Ok(self.token(start, Escape))),

                    '(' => Some(Ok(self.open(start, LParen))),
                    '[' => Some(Ok(self.open(start, LBracket))),
                    '{' => Some(Ok(self.open(start, LBrace))),
                    '}' => Some(Ok(self.close(start, RBrace))),
                    ']' => Some(Ok(self.close(start, RBracket))),
                    ')' => Some(Ok(self.close(start, RParen))),

                    '.' => Some(Ok(self.token(start, Dot))),
                    ':' => Some(Ok(self.token(start, Colon))),
                    ',' => Some(Ok(self.token(start, Comma))),

                    '_' if !self.test_peek(is_ident_continue) => {
                        Some(Ok(self.token(start, Underscore)))
                    }

                    '!' if self.test_peek(|ch| ch == '!' || !is_operator(ch)) => {
                        Some(Ok(self.token(start, Bang)))
                    }

                    '#' => {
                        self.take_until(start, |ch| ch == '\n');
                        continue;
                    }

                    'r' if self.test_peek(|ch| ch == '\'') => Some(self.regex_literal(start)),
                    's' if self.test_peek(|ch| ch == '\'') => Some(self.raw_string_literal(start)),
                    't' if self.test_peek(|ch| ch == '\'') => Some(self.timestamp_literal(start)),

                    ch if is_ident_start(ch) => Some(Ok(self.identifier_or_function_call(start))),
                    ch if is_digit(ch) || (ch == '-' && self.test_peek(is_digit)) => {
                        Some(self.numeric_literal_or_identifier(start))
                    }
                    ch if is_operator(ch) => Some(Ok(self.operator(start))),
                    ch if ch.is_whitespace() => continue,

                    ch => Some(Ok(self.token(start, InvalidToken(ch)))),
                };

                // dbg!(&result); // NOTE: uncomment this for debugging

                return result;

            // If we've parsed the final character, and there are still open
            // queries, we need to keep the iterator going and close those
            // queries.
            } else if let Some(end) = self.rquery_indices.pop() {
                // dbg!("RQuery"); // NOTE: uncomment this for debugging
                return Some(Ok(self.token2(end, end + 1, RQuery)));
            }

            return None;
        }
    }
}

// -----------------------------------------------------------------------------
// lexing logic
// -----------------------------------------------------------------------------

impl<'input> Lexer<'input> {
    fn open(&mut self, start: usize, token: Token<&'input str>) -> Spanned<'input, usize> {
        match &token {
            Token::LParen => self.open_parens += 1,
            Token::LBracket => self.open_brackets += 1,
            Token::LBrace => self.open_braces += 1,
            _ => {}
        };

        self.token(start, token)
    }

    fn close(&mut self, start: usize, token: Token<&'input str>) -> Spanned<'input, usize> {
        match &token {
            Token::RParen => self.open_parens = self.open_parens.saturating_sub(1),
            Token::RBracket => self.open_brackets = self.open_brackets.saturating_sub(1),
            Token::RBrace => self.open_braces = self.open_braces.saturating_sub(1),
            _ => {}
        };

        self.token(start, token)
    }

    fn token(&mut self, start: usize, token: Token<&'input str>) -> Spanned<'input, usize> {
        let end = self.next_index();
        self.token2(start, end, token)
    }

    fn token2(
        &mut self,
        start: usize,
        end: usize,
        token: Token<&'input str>,
    ) -> Spanned<'input, usize> {
        (start, token, end)
    }

    fn query_end(&mut self, start: usize) -> Option<usize> {
        match self.rquery_indices.last() {
            Some(end) if start > 0 && start.saturating_sub(1) == *end => self.rquery_indices.pop(),
            _ => None,
        }
    }

    fn query_start(&mut self, start: usize) -> Result<bool, Error> {
        // If we already opened a query for the current position, we don't want
        // to open another one.
        if self.rquery_indices.last() == Some(&start) {
            return Ok(false);
        }

        // If the iterator is at the end, we don't want to open another one
        if self.peek().is_none() {
            return Ok(false);
        }

        // Take a clone of the existing chars iterator, to allow us to look
        // ahead without advancing the lexer's iterator. This is cheap, since
        // the original iterator only holds references.
        let mut chars = self.chars.clone();
        debug_assert!(chars.peek().is_some());

        // Only continue if the current character is a valid query start
        // character. We know there's at least one more char, given the above
        // assertion.
        if !is_query_start(chars.peek().unwrap().1) {
            return Ok(false);
        }

        // Track if the current chain is a valid one.
        //
        // A valid chain consists of a target, and a path to query that target.
        //
        // Valid examples:
        //
        //   .foo         (target = external, path = .foo)
        //   foo.bar      (target = internal, path = .bar)
        //   { .. }.bar   (target = object, path = .bar)
        //   [1][2]       (target = array, path = [2])
        //
        // Invalid examples:
        //
        //   foo          (target = internal, no path)
        //   { .. }       (target = object, no path)
        //   [1]          (target = array, no path)
        let mut valid = false;

        // Track the last char, so that we know if the next one is valid or not.
        let mut last_char = None;

        // We need to manually track for even open/close characters, to
        // determine when the span will end.
        let mut braces = 0;
        let mut brackets = 0;
        let mut parens = 0;

        let mut end = 0;
        while let Some((pos, ch)) = chars.next() {
            let take_until_end =
                |result: SpannedResult<'input, usize>,
                 last_char: &mut Option<char>,
                 end: &mut usize,
                 chars: &mut Peekable<CharIndices<'input>>| {
                    result.map(|(_, _, new)| {
                        for (i, ch) in chars {
                            *last_char = Some(ch);
                            if i == new + pos {
                                break;
                            }
                        }

                        *end = pos + new;
                    })
                };

            match ch {
                // containers
                '{' => braces += 1,
                '(' => parens += 1,
                '[' if braces == 0 && parens == 0 && brackets == 0 => {
                    brackets += 1;

                    if last_char == Some(']') {
                        valid = true
                    }

                    if last_char == Some('}') {
                        valid = true
                    }

                    if last_char == Some(')') {
                        valid = true
                    }

                    if last_char.map(is_ident_continue) == Some(true) {
                        valid = true
                    }
                }
                '[' => brackets += 1,

                // literals
                '"' => {
                    let result = Lexer::new(&self.input[pos + 1..]).string_literal(0);
                    match take_until_end(result, &mut last_char, &mut end, &mut chars) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                's' if chars.peek().map(|(_, ch)| ch) == Some(&'\'') => {
                    let result = Lexer::new(&self.input[pos + 1..]).raw_string_literal(0);
                    match take_until_end(result, &mut last_char, &mut end, &mut chars) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                'r' if chars.peek().map(|(_, ch)| ch) == Some(&'\'') => {
                    let result = Lexer::new(&self.input[pos + 1..]).regex_literal(0);
                    match take_until_end(result, &mut last_char, &mut end, &mut chars) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                't' if chars.peek().map(|(_, ch)| ch) == Some(&'\'') => {
                    let result = Lexer::new(&self.input[pos + 1..]).timestamp_literal(0);
                    match take_until_end(result, &mut last_char, &mut end, &mut chars) {
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }

                '}' if braces == 0 => break,
                '}' => braces -= 1,

                ')' if parens == 0 => break,
                ')' => parens -= 1,

                ']' if brackets == 0 => break,
                ']' => brackets -= 1,

                // the lexer doesn't care about the semantic validity inside
                // delimited regions in a query.
                _ if braces > 0 || brackets > 0 || parens > 0 => {
                    let (start_delim, end_delim) = if braces > 0 {
                        ('{', '}')
                    } else if brackets > 0 {
                        ('[', ']')
                    } else {
                        ('(', ')')
                    };

                    let mut skip_delim = 0;
                    while let Some((pos, ch)) = chars.peek() {
                        let pos = *pos;

                        let literal_check = |result: Spanned<'input, usize>, chars: &mut Peekable<CharIndices<'input>>| {
                            let (_, _, new) = result;

                            #[allow(clippy::while_let_on_iterator)]
                            while let Some((i, _)) = chars.next() {
                                if i == new + pos {
                                    break;
                                }
                            }
                            match chars.peek().map(|(_, ch)| ch) {
                                Some(ch) => Ok(*ch),
                                None => Err(()),
                            }
                        };

                        let ch = match &self.input[pos..] {
                            s if s.starts_with('#') => {
                                for (_, chr) in chars.by_ref() {
                                    if chr == '\n' {
                                        break;
                                    }
                                }
                                match chars.peek().map(|(_, ch)| ch) {
                                    Some(ch) => *ch,
                                    None => {
                                        return Err(Error::UnexpectedParseError(
                                            "Expected characters at end of comment.".to_string(),
                                        ));
                                    }
                                }
                            }
                            s if s.starts_with('"') => {
                                let r = Lexer::new(&self.input[pos + 1..]).string_literal(0)?;
                                match literal_check(r, &mut chars) {
                                    Ok(ch) => ch,
                                    Err(_) => {
                                        // The call to lexer above should have raised an appropriate error by now,
                                        // so these errors should only occur if there is a bug somewhere previously.
                                        return Err(Error::UnexpectedParseError(
                                            "Expected characters at end of string literal."
                                                .to_string(),
                                        ));
                                    }
                                }
                            }
                            s if s.starts_with("s'") => {
                                let r = Lexer::new(&self.input[pos + 1..]).raw_string_literal(0)?;
                                match literal_check(r, &mut chars) {
                                    Ok(ch) => ch,
                                    Err(_) => {
                                        return Err(Error::UnexpectedParseError(
                                            "Expected characters at end of raw string literal."
                                                .to_string(),
                                        ));
                                    }
                                }
                            }
                            s if s.starts_with("r'") => {
                                let r = Lexer::new(&self.input[pos + 1..]).regex_literal(0)?;
                                match literal_check(r, &mut chars) {
                                    Ok(ch) => ch,
                                    Err(_) => {
                                        return Err(Error::UnexpectedParseError(
                                            "Expected characters at end of regex literal."
                                                .to_string(),
                                        ));
                                    }
                                }
                            }
                            s if s.starts_with("t'") => {
                                let r = Lexer::new(&self.input[pos + 1..]).timestamp_literal(0)?;
                                match literal_check(r, &mut chars) {
                                    Ok(ch) => ch,
                                    Err(_) => {
                                        return Err(Error::UnexpectedParseError(
                                            "Expected characters at end of timestamp literal."
                                                .to_string(),
                                        ));
                                    }
                                }
                            }
                            _ => *ch,
                        };

                        if skip_delim == 0 && ch == end_delim {
                            break;
                        }
                        if let Some((_, c)) = chars.next() {
                            if c == start_delim {
                                skip_delim += 1;
                            }
                            if c == end_delim {
                                skip_delim -= 1;
                            }
                        };
                    }
                }

                '.' if last_char.is_none() => valid = true,
                '.' if last_char == Some(')') => valid = true,
                '.' if last_char == Some('}') => valid = true,
                '.' if last_char == Some(']') => valid = true,
                '.' if last_char == Some('"') => valid = true,
                '.' if last_char.map(is_ident_continue) == Some(true) => {
                    // we need to make sure we're not dealing with a float here
                    let digits = self.input[..pos]
                        .chars()
                        .rev()
                        .take_while(|ch| !ch.is_whitespace())
                        .all(|ch| is_digit(ch) || ch == '_');

                    if !digits {
                        valid = true
                    }
                }

                // function-call-abort
                '!' => {}

                // comments
                '#' => {
                    #[allow(clippy::while_let_on_iterator)]
                    while let Some((pos, ch)) = chars.next() {
                        if ch == '\n' {
                            break;
                        }

                        end = pos;
                    }
                    continue;
                }

                ch if is_ident_continue(ch) => {}

                // Any other character breaks the query chain.
                _ => break,
            }

            last_char = Some(ch);
            end = pos;
        }

        // Skip invalid query chains
        if !valid {
            return Ok(false);
        }

        // If we already tracked the current chain, we want to ignore another one.
        if self.rquery_indices.contains(&end) {
            return Ok(false);
        }

        self.rquery_indices.push(end);
        Ok(true)
    }

    fn string_literal(&mut self, start: usize) -> SpannedResult<'input, usize> {
        let content_start = self.next_index();

        loop {
            let scan_start = self.next_index();
            self.take_until(scan_start, |c| c == '"' || c == '\\');

            match self.bump() {
                Some((escape_start, '\\')) => self.escape_code(escape_start)?,
                Some((content_end, '"')) => {
                    let end = self.next_index();
                    let slice = self.slice(content_start, content_end);
                    let token = Token::StringLiteral(StringLiteral(slice));
                    return Ok((start, token, end));
                }
                _ => break,
            };
        }

        Err(Error::StringLiteral { start })
    }

    fn regex_literal(&mut self, start: usize) -> SpannedResult<'input, usize> {
        self.quoted_literal(start, Token::RegexLiteral)
    }

    fn raw_string_literal(&mut self, start: usize) -> SpannedResult<'input, usize> {
        self.quoted_literal(start, |c| Token::RawStringLiteral(RawStringLiteral(c)))
    }

    fn timestamp_literal(&mut self, start: usize) -> SpannedResult<'input, usize> {
        self.quoted_literal(start, Token::TimestampLiteral)
    }

    fn numeric_literal_or_identifier(&mut self, start: usize) -> SpannedResult<'input, usize> {
        let (end, int) = self.take_while(start, |ch| is_digit(ch) || ch == '_');

        let negative = self.input.get(start..start + 1) == Some("-");
        match self.peek() {
            Some((_, ch)) if is_ident_continue(ch) && !negative => {
                self.bump();
                let (end, ident) = self.take_while(start, is_ident_continue);
                Ok((start, Token::ident(ident), end))
            }
            Some((_, '.')) => {
                self.bump();
                let (end, float) = self.take_while(start, |ch| is_digit(ch) || ch == '_');

                match float.replace('_', "").parse() {
                    Ok(float) => {
                        let float = NotNan::new(float).unwrap();
                        Ok((start, Token::FloatLiteral(float), end))
                    }
                    Err(err) => Err(Error::NumericLiteral {
                        start,
                        end,
                        error: err.to_string(),
                    }),
                }
            }
            None | Some(_) => match int.replace('_', "").parse() {
                Ok(int) => Ok((start, Token::IntegerLiteral(int), end)),
                Err(err) => Err(Error::NumericLiteral {
                    start,
                    end,
                    error: err.to_string(),
                }),
            },
        }
    }

    fn identifier_or_function_call(&mut self, start: usize) -> Spanned<'input, usize> {
        let (end, ident) = self.take_while(start, is_ident_continue);

        let token = if self.test_peek(|ch| ch == '(' || ch == '!') {
            Token::FunctionCall(ident)
        } else {
            Token::ident(ident)
        };

        (start, token, end)
    }

    fn operator(&mut self, start: usize) -> Spanned<'input, usize> {
        let (end, op) = self.take_while(start, is_operator);

        let token = match op {
            "=" => Token::Equals,
            "|=" => Token::MergeEquals,
            "?" => Token::Question,
            op => Token::Operator(op),
        };

        (start, token, end)
    }

    fn quoted_literal(
        &mut self,
        start: usize,
        tok: impl Fn(&'input str) -> Tok<'input>,
    ) -> SpannedResult<'input, usize> {
        self.bump();
        let content_start = self.next_index();

        loop {
            let scan_start = self.next_index();
            self.take_until(scan_start, |c| c == '\'' || c == '\\');

            match self.bump() {
                Some((_, '\\')) => self.bump(),
                Some((end, '\'')) => {
                    let content = self.slice(content_start, end);
                    let token = tok(content);
                    let end = self.next_index();

                    return Ok((start, token, end));
                }
                _ => break,
            };
        }

        Err(Error::Literal { start })
    }
}

// -----------------------------------------------------------------------------
// lexing helpers
// -----------------------------------------------------------------------------

impl<'input> Lexer<'input> {
    pub fn new(input: &'input str) -> Lexer<'input> {
        Self {
            input,
            chars: input.char_indices().peekable(),
            open_braces: 0,
            open_brackets: 0,
            open_parens: 0,
            rquery_indices: vec![],
        }
    }

    fn bump(&mut self) -> Option<(usize, char)> {
        self.chars.next()
    }

    fn peek(&mut self) -> Option<(usize, char)> {
        self.chars.peek().copied()
    }

    fn take_while<F>(&mut self, start: usize, mut keep_going: F) -> (usize, &'input str)
    where
        F: FnMut(char) -> bool,
    {
        self.take_until(start, |c| !keep_going(c))
    }

    fn take_until<F>(&mut self, start: usize, mut terminate: F) -> (usize, &'input str)
    where
        F: FnMut(char) -> bool,
    {
        while let Some((end, ch)) = self.peek() {
            if terminate(ch) {
                return (end, self.slice(start, end));
            } else {
                self.bump();
            }
        }

        let loc = self.next_index();

        (loc, self.slice(start, loc))
    }

    fn test_peek<F>(&mut self, mut test: F) -> bool
    where
        F: FnMut(char) -> bool,
    {
        self.peek().map_or(false, |(_, ch)| test(ch))
    }

    fn slice(&self, start: usize, end: usize) -> &'input str {
        &self.input[start..end]
    }

    fn next_index(&mut self) -> usize {
        self.peek().as_ref().map_or(self.input.len(), |l| l.0)
    }

    /// Returns Ok if the next char is a valid escape code.
    fn escape_code(&mut self, start: usize) -> Result<(), Error> {
        match self.bump() {
            Some((_, '\n')) => Ok(()),
            Some((_, '\'')) => Ok(()),
            Some((_, '"')) => Ok(()),
            Some((_, '\\')) => Ok(()),
            Some((_, 'n')) => Ok(()),
            Some((_, 'r')) => Ok(()),
            Some((_, 't')) => Ok(()),
            Some((_, '{')) => Ok(()),
            Some((_, '}')) => Ok(()),
            Some((start, ch)) => Err(Error::EscapeChar {
                start,
                ch: Some(ch),
            }),
            None => Err(Error::EscapeChar { start, ch: None }),
        }
    }
}

// -----------------------------------------------------------------------------
// generic helpers
// -----------------------------------------------------------------------------

fn is_ident_start(ch: char) -> bool {
    matches!(ch, '@' | '_' | 'a'..='z' | 'A'..='Z')
}

fn is_ident_continue(ch: char) -> bool {
    match ch {
        '0'..='9' => true,
        ch => is_ident_start(ch),
    }
}

fn is_query_start(ch: char) -> bool {
    match ch {
        '.' | '{' | '[' => true,
        ch => is_ident_start(ch),
    }
}

fn is_digit(ch: char) -> bool {
    ch.is_digit(10)
}

pub fn is_operator(ch: char) -> bool {
    matches!(
        ch,
        '!' | '%' | '&' | '*' | '+' | '-' | '/' | '<' | '=' | '>' | '?' | '|'
    )
}

fn unescape_string_literal(mut s: &str) -> String {
    let mut string = String::with_capacity(s.len());
    while let Some(i) = s.bytes().position(|b| b == b'\\') {
        let next = s.as_bytes()[i + 1];
        if next == b'\n' {
            // Remove the \n and any ensuing spaces or tabs
            string.push_str(&s[..i]);
            let remaining = &s[i + 2..];
            let whitespace: usize = remaining
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(|c| c.len_utf8())
                .sum();
            s = &s[i + whitespace + 2..];
        } else {
            let c = match next {
                b'\'' => '\'',
                b'"' => '"',
                b'\\' => '\\',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                _ => unimplemented!("invalid escape"),
            };

            string.push_str(&s[..i]);
            string.push(c);
            s = &s[i + 2..];
        }
    }

    string.push_str(s);
    string
}

#[cfg(test)]
mod test {
    #![allow(clippy::print_stdout)] // tests

    use super::{StringLiteral, *};
    use crate::lex::Token;

    fn lexer(input: &str) -> impl Iterator<Item = SpannedResult<'_, usize>> + '_ {
        let mut lexer = Lexer::new(input);
        Box::new(std::iter::from_fn(move || lexer.next()))
    }

    // only exists to visually align assertions with inputs in tests
    fn data(source: &str) -> &str {
        source
    }

    fn test(input: &str, expected: Vec<(&str, Tok<'_>)>) {
        let mut lexer = lexer(input);
        let mut count = 0;
        let length = expected.len();
        for (token, (expected_span, expected_tok)) in lexer.by_ref().zip(expected.into_iter()) {
            count += 1;
            println!("{:?}", token);
            let start = expected_span.find('~').unwrap_or_default();
            let end = expected_span.rfind('~').map(|i| i + 1).unwrap_or_default();

            let expect = (start, expected_tok, end);
            assert_eq!(Ok(expect), token);
        }

        assert_eq!(count, length);
        assert!(count > 0);
        assert!(lexer.next().is_none());
    }

    #[test]
    fn unterminated_literal_errors() {
        let mut lexer = Lexer::new("a(m, r')");
        assert_eq!(Some(Err(Error::Literal { start: 0 })), lexer.next());
    }

    #[test]
    fn invalid_grok_pattern() {
        // Grok pattern has an invalid escape char -> `\]`
        let mut lexer = Lexer::new(
            r#"parse_grok!("1.2.3.4 - - [23/Mar/2021:06:46:35 +0000]", "%{IPORHOST:remote_ip} %{USER:ident} %{USER:user_name} \[%{HTTPDATE:timestamp}\]""#,
        );
        assert_eq!(
            Some(Err(Error::EscapeChar {
                start: 55,
                ch: Some('[')
            })),
            lexer.next()
        );
    }

    #[test]
    #[rustfmt::skip]
    fn string_literals() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"foo "bar\"\n" baz "" "\t" "\"\"""#),
            vec![
                (r#"~~~                             "#, Token::Identifier("foo")),
                (r#"    ~~~~~~~~~                   "#, L(S("bar\\\"\\n"))),
                (r#"              ~~~               "#, Token::Identifier("baz")),
                (r#"                  ~~            "#, L(S(""))),
                (r#"                     ~~~~       "#, L(S("\\t"))),
                (r#"                          ~~~~~~"#, L(S(r#"\"\""#))),
            ],
        );
        assert_eq!(TemplateString(vec![StringSegment::Literal(r#""""#.to_string())]), StringLiteral(r#"\"\""#).template(Span::default()));
    }

    #[test]
    #[rustfmt::skip]
    fn multiline_string_literals() {
        let mut lexer = lexer(r#""foo \
                                  bar""#);

        match lexer.next() {
            Some(Ok((_, Token::StringLiteral(s), _))) => assert_eq!(TemplateString(vec![StringSegment::Literal("foo bar".to_string())]), s.template(Span::default())),
            _ => panic!("Not a string literal"),
        }
    }

    #[test]
    fn string_literal_unexpected_escape_code() {
        assert_eq!(
            lexer(r#""\X""#).last(),
            Some(Err(Error::StringLiteral { start: 3 }))
        );
    }

    #[test]
    fn string_literal_unterminated() {
        assert_eq!(
            lexer(r#"foo "bar\"\n baz"#).last(),
            Some(Err(Error::StringLiteral { start: 4 }))
        );
    }

    #[test]
    #[rustfmt::skip]
    fn regex_literals() {
        test(
            data(r#"r'[fb]oo+' r'a/b\[rz\]' r''"#),
            vec![
                (r#"~~~~~~~~~~                 "#, Token::RegexLiteral("[fb]oo+")),
                (r#"           ~~~~~~~~~~~~    "#, Token::RegexLiteral("a/b\\[rz\\]")),
                (r#"                        ~~~"#, Token::RegexLiteral("")),
            ],
        );
    }

    #[test]
    fn regex_literal_unterminated() {
        assert_eq!(
            lexer(r#"r'foo bar"#).last(),
            Some(Err(Error::Literal { start: 0 }))
        );
    }

    #[test]
    #[rustfmt::skip]
    fn timestamp_literals() {
        test(
            data(r#"t'foo \' bar'"#),
            vec![
                (r#"~~~~~~~~~~~~~"#, Token::TimestampLiteral("foo \\' bar")),
            ],
        );
    }

    #[test]
    fn timestamp_literal_unterminated() {
        assert_eq!(
            lexer(r#"t'foo"#).last(),
            Some(Err(Error::Literal { start: 0 }))
        );
    }

    #[test]
    #[rustfmt::skip]
    fn raw_string_literals() {
        use RawStringLiteral as S;
        use Token::RawStringLiteral as R;

        test(
            data(r#"s'a "bc" \n \'d'"#),
            vec![
                (r#"~~~~~~~~~~~~~~~~"#, R(S(r#"a "bc" \n \'d"#))),
            ],
        );
    }

    #[test]
    fn raw_string_literal_unterminated() {
        assert_eq!(
            lexer(r#"s'foo"#).last(),
            Some(Err(Error::Literal { start: 0 }))
        );
    }

    #[test]
    #[rustfmt::skip]
    fn number_literals() {
        test(
            data(r#"12 012 12.43 12. 0 902.0001"#),
            vec![
                (r#"~~                         "#, Token::IntegerLiteral(12)),
                (r#"   ~~~                     "#, Token::IntegerLiteral(12)),
                (r#"       ~~~~~               "#, Token::FloatLiteral(NotNan::new(12.43).unwrap())),
                (r#"             ~~~           "#, Token::FloatLiteral(NotNan::new(12.0).unwrap())),
                (r#"                 ~         "#, Token::IntegerLiteral(0)),
                (r#"                   ~~~~~~~~"#, Token::FloatLiteral(NotNan::new(902.0001).unwrap())),
            ],
        );
    }

    #[test]
    #[rustfmt::skip]
    fn number_literals_underscore() {
        test(
            data(r#"1_000 1_2_3._4_0_"#),
            vec![
                (r#"~~~~~            "#, Token::IntegerLiteral(1000)),
                (r#"      ~~~~~~~~~~~"#, Token::FloatLiteral(NotNan::new(123.40).unwrap())),
            ],
        );
    }

    #[test]
    fn identifiers() {
        test(
            data(r#"foo bar1 if baz_12_qux else "#),
            vec![
                (r#"~~~                         "#, Token::Identifier("foo")),
                (r#"    ~~~~                    "#, Token::Identifier("bar1")),
                (r#"         ~~                 "#, Token::If),
                (
                    r#"            ~~~~~~~~~~      "#,
                    Token::Identifier("baz_12_qux"),
                ),
                (r#"                       ~~~~ "#, Token::Else),
            ],
        );
    }

    #[test]
    fn function_calls() {
        test(
            data(r#"foo() bar_1() if() "#),
            vec![
                (r#"~~~                "#, Token::FunctionCall("foo")),
                (r#"   ~               "#, Token::LParen),
                (r#"    ~              "#, Token::RParen),
                (r#"      ~~~~~        "#, Token::FunctionCall("bar_1")),
                (r#"           ~       "#, Token::LParen),
                (r#"            ~      "#, Token::RParen),
                (r#"              ~~   "#, Token::FunctionCall("if")),
                (r#"                ~  "#, Token::LParen),
                (r#"                 ~ "#, Token::RParen),
            ],
        );
    }

    #[test]
    fn single_query() {
        test(
            data(r#"."#),
            vec![
                //
                (r#"~"#, Token::LQuery),
                (r#"~"#, Token::Dot),
                (r#"~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn root_query() {
        test(
            data(r#". .foo . .bar ."#),
            vec![
                (r#"~              "#, Token::LQuery),
                (r#"~              "#, Token::Dot),
                (r#"~              "#, Token::RQuery),
                (r#"  ~            "#, Token::LQuery),
                (r#"  ~            "#, Token::Dot),
                (r#"   ~~~         "#, Token::Identifier("foo")),
                (r#"     ~         "#, Token::RQuery),
                (r#"       ~       "#, Token::LQuery),
                (r#"       ~       "#, Token::Dot),
                (r#"       ~       "#, Token::RQuery),
                (r#"         ~     "#, Token::LQuery),
                (r#"         ~     "#, Token::Dot),
                (r#"          ~~~  "#, Token::Identifier("bar")),
                (r#"            ~  "#, Token::RQuery),
                (r#"              ~"#, Token::LQuery),
                (r#"              ~"#, Token::Dot),
                (r#"              ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn ampersat_in_query() {
        test(
            data(r#".@foo .bar.@ook"#),
            vec![
                (r#"~              "#, Token::LQuery),
                (r#"~              "#, Token::Dot),
                (r#" ~~~~          "#, Token::PathField("@foo")),
                (r#"    ~          "#, Token::RQuery),
                (r#"      ~        "#, Token::LQuery),
                (r#"      ~        "#, Token::Dot),
                (r#"       ~~~     "#, Token::Identifier("bar")),
                (r#"          ~    "#, Token::Dot),
                (r#"           ~~~~"#, Token::PathField("@ook")),
                (r#"              ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn queries() {
        test(
            data(r#".foo bar.baz .baz.qux"#),
            vec![
                (r#"~                    "#, Token::LQuery),
                (r#"~                    "#, Token::Dot),
                (r#" ~~~                 "#, Token::Identifier("foo")),
                (r#"   ~                 "#, Token::RQuery),
                (r#"     ~               "#, Token::LQuery),
                (r#"     ~~~             "#, Token::Identifier("bar")),
                (r#"        ~            "#, Token::Dot),
                (r#"         ~~~         "#, Token::Identifier("baz")),
                (r#"           ~         "#, Token::RQuery),
                (r#"             ~       "#, Token::LQuery),
                (r#"             ~       "#, Token::Dot),
                (r#"              ~~~    "#, Token::Identifier("baz")),
                (r#"                 ~   "#, Token::Dot),
                (r#"                  ~~~"#, Token::Identifier("qux")),
                (r#"                    ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    #[rustfmt::skip]
    fn nested_queries() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"[.foo].bar { "foo": [2][0] }"#),
            vec![
                (r#"~                           "#, Token::LQuery),
                (r#"~                           "#, Token::LBracket),
                (r#" ~                          "#, Token::LQuery),
                (r#" ~                          "#, Token::Dot),
                (r#"  ~~~                       "#, Token::Identifier("foo")),
                (r#"    ~                       "#, Token::RQuery),
                (r#"     ~                      "#, Token::RBracket),
                (r#"      ~                     "#, Token::Dot),
                (r#"       ~~~                  "#, Token::Identifier("bar")),
                (r#"         ~                  "#, Token::RQuery),
                (r#"           ~                "#, Token::LBrace),
                (r#"             ~~~~~          "#, L(S("foo"))),
                (r#"                  ~         "#, Token::Colon),
                (r#"                    ~       "#, Token::LQuery),
                (r#"                    ~       "#, Token::LBracket),
                (r#"                     ~      "#, Token::IntegerLiteral(2)),
                (r#"                      ~     "#, Token::RBracket),
                (r#"                       ~    "#, Token::LBracket),
                (r#"                        ~   "#, Token::IntegerLiteral(0)),
                (r#"                         ~  "#, Token::RBracket),
                (r#"                         ~  "#, Token::RQuery),
                (r#"                           ~"#, Token::RBrace),
            ],
        );
    }

    #[test]
    fn coalesced_queries() {
        test(
            data(r#".foo.(bar | baz)"#),
            vec![
                (r#"~               "#, Token::LQuery),
                (r#"~               "#, Token::Dot),
                (r#" ~~~            "#, Token::Identifier("foo")),
                (r#"    ~           "#, Token::Dot),
                (r#"     ~          "#, Token::LParen),
                (r#"      ~~~       "#, Token::Identifier("bar")),
                (r#"          ~     "#, Token::Operator("|")),
                (r#"            ~~~ "#, Token::Identifier("baz")),
                (r#"               ~"#, Token::RParen),
                (r#"               ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn complex_query_1() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#".a.(b | c  )."d\"e"[2 ][ 1]"#),
            vec![
                (r#"~                          "#, Token::LQuery),
                (r#"~                          "#, Token::Dot),
                (r#" ~                         "#, Token::Identifier("a")),
                (r#"  ~                        "#, Token::Dot),
                (r#"   ~                       "#, Token::LParen),
                (r#"    ~                      "#, Token::Identifier("b")),
                (r#"      ~                    "#, Token::Operator("|")),
                (r#"        ~                  "#, Token::Identifier("c")),
                (r#"           ~               "#, Token::RParen),
                (r#"            ~              "#, Token::Dot),
                (r#"             ~~~~~~        "#, L(S("d\\\"e"))),
                (r#"                   ~       "#, Token::LBracket),
                (r#"                    ~      "#, Token::IntegerLiteral(2)),
                (r#"                      ~    "#, Token::RBracket),
                (r#"                       ~   "#, Token::LBracket),
                (r#"                         ~ "#, Token::IntegerLiteral(1)),
                (r#"                          ~"#, Token::RBracket),
            ],
        );
    }

    #[test]
    #[rustfmt::skip]
    fn complex_query_2() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"{ "a": parse_json!("{ \"b\": 0 }").c }"#),
            vec![
                (r#"~                                     "#, Token::LBrace),
                (r#"  ~~~                                 "#, L(S("a"))),
                (r#"     ~                                "#, Token::Colon),
                (r#"       ~                              "#, Token::LQuery),
                (r#"       ~~~~~~~~~~                     "#, Token::FunctionCall("parse_json")),
                (r#"                 ~                    "#, Token::Bang),
                (r#"                  ~                   "#, Token::LParen),
                (r#"                   ~~~~~~~~~~~~~~     "#, L(S("{ \\\"b\\\": 0 }"))),
                (r#"                                 ~    "#, Token::RParen),
                (r#"                                  ~   "#, Token::Dot),
                (r#"                                   ~  "#, Token::Identifier("c")),
                (r#"                                   ~  "#, Token::RQuery),
                (r#"                                     ~"#, Token::RBrace),
            ],
        );
    }

    #[test]
    #[rustfmt::skip]
    fn query_with_literals() {
        use StringLiteral as S;
        use RawStringLiteral as RS;
        use Token::StringLiteral as L;
        use Token::RawStringLiteral as R;

        test(
            data(r#"{ "a": r'b?c', "d": s'"e"\'f', "g": t'1.0T0' }.h"#),
            vec![
                (r#"~                                               "#, Token::LQuery),
                (r#"~                                               "#, Token::LBrace),
                (r#"  ~~~                                           "#, L(S("a"))),
                (r#"     ~                                          "#, Token::Colon),
                (r#"       ~~~~~~                                   "#, Token::RegexLiteral("b?c")),
                (r#"             ~                                  "#, Token::Comma),
                (r#"               ~~~                              "#, L(S("d"))),
                (r#"                  ~                             "#, Token::Colon),
                (r#"                    ~~~~~~~~~                   "#, R(RS("\"e\"\\\'f"))),
                (r#"                             ~                  "#, Token::Comma),
                (r#"                               ~~~              "#, L(S("g"))),
                (r#"                                  ~             "#, Token::Colon),
                (r#"                                    ~~~~~~~~    "#, Token::TimestampLiteral("1.0T0")),
                (r#"                                             ~  "#, Token::RBrace),
                (r#"                                              ~ "#, Token::Dot),
                (r#"                                               ~"#, Token::Identifier("h")),
                (r#"                                               ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn variable_queries() {
        test(
            data(r#"foo.bar foo[2]"#),
            vec![
                (r#"~             "#, Token::LQuery),
                (r#"~~~           "#, Token::Identifier("foo")),
                (r#"   ~          "#, Token::Dot),
                (r#"    ~~~       "#, Token::Identifier("bar")),
                (r#"      ~       "#, Token::RQuery),
                (r#"        ~     "#, Token::LQuery),
                (r#"        ~~~   "#, Token::Identifier("foo")),
                (r#"           ~  "#, Token::LBracket),
                (r#"            ~ "#, Token::IntegerLiteral(2)),
                (r#"             ~"#, Token::RBracket),
                (r#"             ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn object_queries() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"{ "foo": "bar" }.foo"#),
            vec![
                (r#"~                   "#, Token::LQuery),
                (r#"~                   "#, Token::LBrace),
                (r#"  ~~~~~             "#, L(S("foo"))),
                (r#"       ~            "#, Token::Colon),
                (r#"         ~~~~~      "#, L(S("bar"))),
                (r#"               ~    "#, Token::RBrace),
                (r#"                ~   "#, Token::Dot),
                (r#"                 ~~~"#, Token::Identifier("foo")),
                (r#"                   ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn array_queries() {
        test(
            data(r#"[ 1, 2 , 3].foo"#),
            vec![
                (r#"~              "#, Token::LQuery),
                (r#"~              "#, Token::LBracket),
                (r#"  ~            "#, Token::IntegerLiteral(1)),
                (r#"   ~           "#, Token::Comma),
                (r#"     ~         "#, Token::IntegerLiteral(2)),
                (r#"       ~       "#, Token::Comma),
                (r#"         ~     "#, Token::IntegerLiteral(3)),
                (r#"          ~    "#, Token::RBracket),
                (r#"           ~   "#, Token::Dot),
                (r#"            ~~~"#, Token::Identifier("foo")),
                (r#"              ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn function_call_queries() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"foo(ab: "c")[2].d"#),
            vec![
                (r#"~                "#, Token::LQuery),
                (r#"~~~              "#, Token::FunctionCall("foo")),
                (r#"   ~             "#, Token::LParen),
                (r#"    ~~           "#, Token::Identifier("ab")),
                (r#"      ~          "#, Token::Colon),
                (r#"        ~~~      "#, L(S("c"))),
                (r#"           ~     "#, Token::RParen),
                (r#"            ~    "#, Token::LBracket),
                (r#"             ~   "#, Token::IntegerLiteral(2)),
                (r#"              ~  "#, Token::RBracket),
                (r#"               ~ "#, Token::Dot),
                (r#"                ~"#, Token::Identifier("d")),
                (r#"                ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn queries_in_array() {
        test(
            data("[foo[0]]"),
            vec![
                ("~       ", Token::LBracket),
                (" ~      ", Token::LQuery),
                (" ~~~    ", Token::Identifier("foo")),
                ("    ~   ", Token::LBracket),
                ("     ~  ", Token::IntegerLiteral(0)),
                ("      ~ ", Token::RBracket),
                ("      ~ ", Token::RQuery),
                ("       ~", Token::RBracket),
            ],
        );
    }

    #[test]
    fn queries_op() {
        test(
            data(r#".a + 3 .b == true"#),
            vec![
                (r#"~                "#, Token::LQuery),
                (r#"~                "#, Token::Dot),
                (r#" ~               "#, Token::Identifier("a")),
                (r#" ~               "#, Token::RQuery),
                (r#"   ~             "#, Token::Operator("+")),
                (r#"     ~           "#, Token::IntegerLiteral(3)),
                (r#"       ~         "#, Token::LQuery),
                (r#"       ~         "#, Token::Dot),
                (r#"        ~        "#, Token::Identifier("b")),
                (r#"        ~        "#, Token::RQuery),
                (r#"          ~~     "#, Token::Operator("==")),
                (r#"             ~~~~"#, Token::True),
            ],
        );
    }

    #[test]
    fn invalid_queries() {
        test(
            data(".foo.\n"),
            vec![
                ("~      ", Token::LQuery),
                ("~      ", Token::Dot),
                (" ~~~   ", Token::Identifier("foo")),
                ("    ~  ", Token::Dot),
                ("    ~  ", Token::RQuery),
                ("     ~ ", Token::Newline),
            ],
        );
    }

    #[test]
    fn queries_in_multiline() {
        test(
            data(".foo\n.bar = true"),
            vec![
                ("~               ", Token::LQuery),
                ("~               ", Token::Dot),
                (" ~~~            ", Token::Identifier("foo")),
                ("   ~            ", Token::RQuery),
                ("    ~           ", Token::Newline),
                ("     ~          ", Token::LQuery),
                ("     ~          ", Token::Dot),
                ("      ~~~       ", Token::Identifier("bar")),
                ("        ~       ", Token::RQuery),
                ("          ~     ", Token::Equals),
                ("            ~~~~", Token::True),
            ],
        );
    }

    #[test]
    #[rustfmt::skip]
    fn quoted_path_queries() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"."parent.key.with.special characters".child"#),
            vec![
                (r#"~                                          "#, Token::LQuery),
                (r#"~                                          "#, Token::Dot),
                (r#" ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~      "#, L(S("parent.key.with.special characters"))),
                (r#"                                     ~     "#, Token::Dot),
                (r#"                                      ~~~~~"#, Token::Identifier("child")),
                (r#"                                          ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn queries_digit_path() {
        test(
            data(r#".0foo foo.00_7bar.tar"#),
            vec![
                (r#"~                    "#, Token::LQuery),
                (r#"~                    "#, Token::Dot),
                (r#" ~~~~                "#, Token::Identifier("0foo")),
                (r#"    ~                "#, Token::RQuery),
                (r#"      ~              "#, Token::LQuery),
                (r#"      ~~~            "#, Token::Identifier("foo")),
                (r#"         ~           "#, Token::Dot),
                (r#"          ~~~~~~~    "#, Token::Identifier("00_7bar")),
                (r#"                 ~   "#, Token::Dot),
                (r#"                  ~~~"#, Token::Identifier("tar")),
                (r#"                    ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn queries_nested_delims() {
        use StringLiteral as S;
        use Token::StringLiteral as L;

        test(
            data(r#"{ "foo": [true] }.foo[0]"#),
            vec![
                (r#"~                       "#, Token::LQuery),
                (r#"~                       "#, Token::LBrace),
                (r#"  ~~~~~                 "#, L(S("foo"))),
                (r#"       ~                "#, Token::Colon),
                (r#"         ~              "#, Token::LBracket),
                (r#"          ~~~~          "#, Token::True),
                (r#"              ~         "#, Token::RBracket),
                (r#"                ~       "#, Token::RBrace),
                (r#"                 ~      "#, Token::Dot),
                (r#"                  ~~~   "#, Token::Identifier("foo")),
                (r#"                     ~  "#, Token::LBracket),
                (r#"                      ~ "#, Token::IntegerLiteral(0)),
                (r#"                       ~"#, Token::RBracket),
                (r#"                       ~"#, Token::RQuery),
            ],
        );
    }

    #[test]
    fn queries_negative_index() {
        test(
            data("v[-1] = 2"),
            vec![
                ("~        ", Token::LQuery),
                ("~        ", Token::Identifier("v")),
                (" ~       ", Token::LBracket),
                ("  ~~     ", Token::IntegerLiteral(-1)),
                ("    ~    ", Token::RBracket),
                ("    ~    ", Token::RQuery),
                ("      ~  ", Token::Equals),
                ("        ~", Token::IntegerLiteral(2)),
            ],
        );
    }

    #[test]
    fn multi_byte_character_1() {
        use RawStringLiteral as RS;
        use Token::RawStringLiteral as R;

        test(
            data("a * s'漢字' * a"),
            vec![
                ("~                ", Token::Identifier("a")),
                ("  ~              ", Token::Operator("*")),
                ("    ~~~~~~~~~    ", R(RS("漢字"))),
                ("              ~  ", Token::Operator("*")),
                ("                ~", Token::Identifier("a")),
            ],
        );
    }

    #[test]
    fn multi_byte_character_2() {
        use RawStringLiteral as RS;
        use Token::RawStringLiteral as R;

        test(
            data("a * s'¡' * a"),
            vec![
                ("~            ", Token::Identifier("a")),
                ("  ~          ", Token::Operator("*")),
                ("    ~~~~~    ", R(RS("¡"))),
                ("          ~  ", Token::Operator("*")),
                ("            ~", Token::Identifier("a")),
            ],
        );
    }

    #[test]
    fn comment_in_block() {
        test(
            data("if x {\n   # It's an apostrophe.\n   3\n}"),
            vec![
                ("~~                                    ", Token::If),
                (
                    "   ~                                  ",
                    Token::Identifier("x"),
                ),
                ("     ~                                ", Token::LBrace),
                ("      ~                               ", Token::Newline),
                ("                               ~      ", Token::Newline),
                (
                    "                                   ~  ",
                    Token::IntegerLiteral(3),
                ),
                ("                                    ~ ", Token::Newline),
                ("                                     ~", Token::RBrace),
            ],
        );
    }

    #[test]
    fn unescape_string_literal() {
        let string = StringLiteral("zork {{ zonk }} zoog");
        assert_eq!(
            TemplateString(vec![
                StringSegment::Literal("zork ".to_string()),
                StringSegment::Template("zonk".to_string(), Span::new(6, 16)),
                StringSegment::Literal(" zoog".to_string()),
            ]),
            string.template(Span::new(0, 20))
        );
    }
}
