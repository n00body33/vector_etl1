use std::fmt;
use std::iter::{FromIterator, IntoIterator};
use std::str::FromStr;

/// Provide easy access to individual [`Segment`]s of a path.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Path {
    segments: Vec<Segment>,
}

impl FromStr for Path {
    type Err = Error;

    /// Parse a string path into a [`Path`] wrapper with easy access to
    /// individual path [`Segment`]s.
    ///
    /// This function fails if the provided path is invalid, as defined by the
    /// parser grammar.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('.') {
            parser::parse_path(s)
        } else {
            let s = format!(".{}", s);
            parser::parse_path(&s)
        }
        .map(Into::into)
        .map_err(|err| Error::Parse(err.to_string()))
    }
}

impl Path {
    /// Create a path from a list of [`Segment`]s.
    ///
    /// Note that the caller is required to uphold the invariant that the list
    /// of segments was generated by the Remap parser.
    ///
    /// Use the `from_str` method if you want to be sure the generated [`Path`]
    /// is valid.
    pub fn new_unchecked(segments: Vec<Segment>) -> Self {
        Self { segments }
    }

    /// Create a "root" path, containing no segments, which when written as a
    /// string is represented as `"."`.
    pub fn root() -> Self {
        Self { segments: vec![] }
    }

    /// Returns `true` if the path points to the _root_ of a given object.
    ///
    /// In its string form, this is represented as `.` (a single dot).
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// This is a temporary function to make it easier to interface [`Path`]
    /// with Vector.
    ///
    /// .foo[2]                 => [[".foo[2]"]]
    /// .foo.bar.(baz | qux)    => [[".foo"], [".bar"], [".baz", ".qux"]]
    pub fn to_alternative_components(&self) -> Vec<Vec<String>> {
        let mut segments = vec![];
        let handle_field = |field: &Field| field.as_str().replace('.', "\\.");

        for segment in self.segments() {
            match segment {
                Segment::Field(field) => segments.push(vec![handle_field(field)]),
                Segment::Coalesce(fields) => {
                    segments.push(fields.iter().map(|f| handle_field(f)).collect::<Vec<_>>())
                }
                Segment::Index(_) => segments.last_mut().into_iter().for_each(|vec| {
                    vec.iter_mut()
                        .for_each(|s| s.push_str(&segment.to_string()))
                }),
            }
        }

        segments
    }

    /// Similar to `to_alternative_components`, except that it produces a list of
    /// alternative strings:
    ///
    /// .foo.(bar | baz)[1].(qux | quux)
    ///
    /// // .foo.bar[1].qux
    /// // .foo.bar[1].quux
    /// // .foo.baz[1].qux
    /// // .foo.baz[1].quux
    ///
    /// Coalesced paths to the left take precedence over the ones to the right.
    pub fn to_alternative_strings(&self) -> Vec<String> {
        if self.is_root() {
            return vec![];
        }

        let components: Vec<Vec<String>> = self.to_alternative_components();
        let mut total_alternatives = components.iter().fold(1, |acc, vec| acc * vec.len());
        let mut paths: Vec<String> = Vec::with_capacity(total_alternatives - 1);
        paths.resize(total_alternatives, String::with_capacity(128));

        // Loops each of the components appending to `paths` whenever we hit an
        // alternative expansion. This loop will dot-separate the alternatives
        // inline but will add one additional dot more than required at the
        // close.
        for fields in components.into_iter() {
            debug_assert!(!fields.is_empty());

            // Each time we loop the total number of alternatives left for
            // duplication drop by the number of alternatives in `field`.
            total_alternatives /= fields.len();
            for (path_idx, buf) in paths.iter_mut().enumerate() {
                // Compute the field index by first determining the mulitple of
                // the _path_ index with regard to the remaining alternatives
                // and then map this into the field vector. This ensures we
                // generate all the combinations in the order expected.
                let idx = (path_idx / total_alternatives) % fields.len();
                buf.push_str(&fields[idx]);
                buf.push_str(".");
            }
        }
        // Loop each of the overly dotted paths and remove the final, extraneous
        // dot.
        for path in &mut paths {
            let _ = path.pop();
        }

        paths
    }

    /// A poor-mans way to convert an "alternative" string representation to a
    /// path.
    ///
    /// This will be replaced once better path handling lands.
    pub fn from_alternative_string(path: String) -> Result<Self, Error> {
        let mut segments = vec![];
        let mut chars = path.chars().peekable();
        let mut part = String::new();

        let handle_field = |part: &mut String, segments: &mut Vec<Segment>| -> Result<(), Error> {
            let string = part.replace("\\.", ".");
            let field = Field::from_str(&string)?;
            segments.push(Segment::Field(field));
            part.clear();
            Ok(())
        };

        let mut handle_char = |c: char,
                               chars: &mut std::iter::Peekable<std::str::Chars>,
                               part: &mut String|
         -> Result<(), Error> {
            match c {
                '\\' if chars.peek() == Some(&'.') || chars.peek() == Some(&'[') => {
                    part.push(c);
                    part.push(chars.next().unwrap());
                }
                '[' => {
                    if !part.is_empty() {
                        handle_field(part, &mut segments)?;
                    }

                    for c in chars {
                        if c == ']' {
                            let index = part
                                .parse::<usize>()
                                .map_err(|err| Error::Alternative(err.to_string()))?;

                            segments.push(Segment::Index(index as i64));
                            part.clear();
                            break;
                        }

                        part.push(c);
                    }
                }
                '.' if !part.is_empty() => handle_field(part, &mut segments)?,
                '\0' if !part.is_empty() => handle_field(part, &mut segments)?,
                '.' => {}
                _ => part.push(c),
            }

            Ok(())
        };

        while let Some(c) = chars.next() {
            handle_char(c, &mut chars, &mut part)?;
        }

        if !part.is_empty() {
            handle_char('\0', &mut chars, &mut part)?;
        }

        Ok(Self::new_unchecked(segments))
    }

    /// Appends a new segment to the end of this path.
    pub fn append(&mut self, segment: Segment) {
        self.segments.push(segment);
    }

    /// Returns true if the current path starts with the same segments
    /// as the given path.
    ///
    /// ".noog.norg.nink".starts_with(".noog.norg") == true
    pub fn starts_with(&self, other: &Path) -> bool {
        if self.segments.len() < other.segments.len() {
            return false;
        }

        self.segments
            .iter()
            .take(other.segments.len())
            .zip(other.segments.iter())
            .all(|(me, them)| me == them)
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(".")?;

        let mut iter = self.segments.iter().peekable();
        while let Some(segment) = iter.next() {
            segment.fmt(f)?;

            match iter.peek() {
                Some(Segment::Field(_)) | Some(Segment::Coalesce(_)) => f.write_str(".")?,
                _ => {}
            }
        }

        Ok(())
    }
}

impl IntoIterator for Path {
    type Item = Segment;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.into_iter()
    }
}

impl FromIterator<Segment> for Path {
    fn from_iter<I: IntoIterator<Item = Segment>>(iter: I) -> Self {
        let segments = iter.into_iter().collect();
        Self { segments }
    }
}

impl From<parser::ast::Path> for Path {
    fn from(path: parser::ast::Path) -> Self {
        let segments = path.into_iter().map(Into::into).collect::<Vec<_>>();

        Self { segments }
    }
}

// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Segment {
    Field(Field),
    Coalesce(Vec<Field>),
    Index(i64),
}

impl Segment {
    pub fn is_field(&self) -> bool {
        matches!(self, Self::Field(_))
    }

    pub fn is_coalesce(&self) -> bool {
        matches!(self, Self::Coalesce(_))
    }

    pub fn is_index(&self) -> bool {
        matches!(self, Self::Index(_))
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Segment::Field(path) => write!(f, "{}", path),
            Segment::Coalesce(paths) => write!(
                f,
                "({})",
                paths
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            Segment::Index(i) => f.write_str(&format!("[{}]", i)),
        }
    }
}

impl From<parser::ast::PathSegment> for Segment {
    fn from(segment: parser::ast::PathSegment) -> Self {
        use parser::ast::PathSegment::*;

        match segment {
            Field(field) => Segment::Field(field.into()),
            Index(i) => Segment::Index(i),
            Coalesce(fields) => fields
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

impl From<Vec<Field>> for Segment {
    fn from(fields: Vec<Field>) -> Self {
        Self::Coalesce(fields)
    }
}

// -----------------------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum Field {
    Regular(String),
    Quoted(String),
}

impl Field {
    pub fn as_str(&self) -> &str {
        match self {
            Field::Regular(field) => &field,
            Field::Quoted(field) => &field,
        }
    }
}

impl FromStr for Field {
    type Err = Error;

    /// Parse a string field into a [`Field`].
    ///
    /// If the string represents a valid identifier, this function returns
    /// `Field::Regular`, otherwise if it's a valid string, it'll return
    /// `Field::Quoted`.
    ///
    /// If neither is true, an error is returned.
    fn from_str(field: &str) -> Result<Self, Self::Err> {
        parser::parse_field(field)
            .or_else(|_| parser::parse_field(format!(r#""{}""#, field)))
            .map(Into::into)
            .map_err(|err| Error::Parse(err.to_string()))
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::Regular(path) => f.write_str(path),
            Field::Quoted(path) => {
                f.write_str("\"")?;
                f.write_str(path)?;
                f.write_str("\"")
            }
        }
    }
}

impl From<parser::ast::Field> for Field {
    fn from(field: parser::ast::Field) -> Self {
        use parser::ast::Field::*;

        match field {
            Regular(ident) => Field::Regular(ident.into_inner()),
            Quoted(string) => Field::Quoted(string),
        }
    }
}

// -----------------------------------------------------------------------------

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum Error {
    #[error("unable to create path from alternative string: {0}")]
    Alternative(String),

    #[error("unable to parse path")]
    Parse(String),
}

// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Field;
    use super::*;
    use Field::*;
    use Segment::*;

    #[test]
    fn test_starts_with() {
        assert!(Path::from_str(".noog.nork.nink")
            .unwrap()
            .starts_with(&Path::from_str(".noog.nork").unwrap()));

        assert!(!Path::from_str(".noog.nork")
            .unwrap()
            .starts_with(&Path::from_str(".noog.nork.nink").unwrap()));

        assert!(Path::from_str(".noog.nork.nink")
            .unwrap()
            .starts_with(&Path::from_str(".noog.nork.nink").unwrap()));
    }

    #[test]
    fn test_path() {
        let cases = vec![
            (".foo", vec![Field(Regular("foo".to_owned()))]),
            (
                ".foo.bar",
                vec![
                    Field(Regular("foo".to_owned())),
                    Field(Regular("bar".to_owned())),
                ],
            ),
            (
                ".foo.(bar | baz)",
                vec![
                    Field(Regular("foo".to_owned())),
                    Coalesce(vec![Regular("bar".to_owned()), Regular("baz".to_owned())]),
                ],
            ),
            (".foo[2]", vec![Field(Regular("foo".to_owned())), Index(2)]),
            (
                r#".foo."bar baz""#,
                vec![
                    Field(Regular("foo".to_owned())),
                    Field(Quoted("bar baz".to_owned())),
                ],
            ),
            (
                r#".foo.("bar baz" | qux)[0]"#,
                vec![
                    Field(Regular("foo".to_owned())),
                    Coalesce(vec![
                        Quoted("bar baz".to_owned()),
                        Regular("qux".to_owned()),
                    ]),
                    Index(0),
                ],
            ),
            (
                r#".foo.("bar baz" | qux | quux)[0][2].bla"#,
                vec![
                    Field(Regular("foo".to_owned())),
                    Coalesce(vec![
                        Quoted("bar baz".to_owned()),
                        Regular("qux".to_owned()),
                        Regular("quux".to_owned()),
                    ]),
                    Index(0),
                    Index(2),
                    Field(Regular("bla".to_owned())),
                ],
            ),
        ];

        for (string, segments) in cases {
            let path = Path::from_str(string);
            assert_eq!(Ok(segments.clone()), path.map(|p| p.segments().to_owned()));

            let path = Path::new_unchecked(segments).to_string();
            assert_eq!(string.to_string(), path);
        }
    }

    #[test]
    fn test_to_alternate_components() {
        let path = Path::from_str(r#".a.(b | c | d | e).f.(g | h | i).(j | k)"#).unwrap();

        assert_eq!(
            path.to_alternative_components(),
            vec![
                vec!["a".to_owned()],
                vec![
                    "b".to_owned(),
                    "c".to_owned(),
                    "d".to_owned(),
                    "e".to_owned(),
                ],
                vec!["f".to_owned()],
                vec!["g".to_owned(), "h".to_owned(), "i".to_owned(),],
                vec!["j".to_owned(), "k".to_owned(),],
            ]
        );
    }

    #[test]
    fn test_to_alternate_strings() {
        let path = Path::from_str(r#".a.(b | c | d | e).f.(g | h | i).(j | k)"#).unwrap();

        let actual: Vec<String> = path.to_alternative_strings();
        let expected: Vec<&str> = vec![
            "a.b.f.g.j",
            "a.b.f.g.k",
            "a.b.f.h.j",
            "a.b.f.h.k",
            "a.b.f.i.j",
            "a.b.f.i.k",
            //
            "a.c.f.g.j",
            "a.c.f.g.k",
            "a.c.f.h.j",
            "a.c.f.h.k",
            "a.c.f.i.j",
            "a.c.f.i.k",
            //
            "a.d.f.g.j",
            "a.d.f.g.k",
            "a.d.f.h.j",
            "a.d.f.h.k",
            "a.d.f.i.j",
            "a.d.f.i.k",
            //
            "a.e.f.g.j",
            "a.e.f.g.k",
            "a.e.f.h.j",
            "a.e.f.h.k",
            "a.e.f.i.j",
            "a.e.f.i.k",
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_from_alternate_string() {
        let path = "foo.bar\\.baz[2][1].foobar".to_string();

        let path = Path::from_alternative_string(path);
        assert_eq!(
            path.map(|p| p.segments().to_owned()),
            Ok(vec![
                Field(Regular("foo".to_owned())),
                Field(Quoted("bar.baz".to_owned())),
                Index(2),
                Index(1),
                Field(Regular("foobar".to_owned())),
            ]),
        );
    }
}
