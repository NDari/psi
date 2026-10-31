//! The API's type grammar (the header of `api/psi.toml`). build.rs includes this
//! file too, so the build and the runtime share one grammar; keep it free of mlua.

use std::fmt;

/// A parsed API type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    String,
    Integer,
    Number,
    Boolean,
    Table,
    Any,
    Nil,
    /// `T?`. A union with any optional member is optional as a whole:
    /// `"a"|"b"?` is `Optional(Literals(a, b))`.
    Optional(Box<Ty>),
    /// `T[]`
    List(Box<Ty>),
    /// `{K: V}`
    Map(Box<Ty>, Box<Ty>),
    /// `"a"|"b"`: one of these strings.
    Literals(Vec<String>),
    /// `string|ToolResult`: any of these (never nested, never optional members).
    Union(Vec<Ty>),
    /// `function(name: T, ...): R`
    Function {
        params: Vec<(String, Ty)>,
        ret: Option<Box<Ty>>,
    },
    /// A `[[type]]` from the API definition.
    Named(String),
}

/// Parses a type string such as `function(ev: table, ctx: Context): any`.
pub fn parse(src: &str) -> Result<Ty, String> {
    let mut p = Parser { src, pos: 0 };
    let ty = p.union()?;
    p.ws();
    if p.pos < src.len() {
        return Err(p.err("unexpected text"));
    }
    Ok(ty)
}

impl Ty {
    /// Calls `f` with the name of every named type this type refers to.
    pub fn for_each_named(&self, f: &mut impl FnMut(&str)) {
        match self {
            Ty::Named(n) => f(n),
            Ty::Optional(t) | Ty::List(t) => t.for_each_named(f),
            Ty::Map(k, v) => {
                k.for_each_named(f);
                v.for_each_named(f);
            }
            Ty::Union(ts) => ts.iter().for_each(|t| t.for_each_named(f)),
            Ty::Function { params, ret } => {
                params.iter().for_each(|(_, t)| t.for_each_named(f));
                if let Some(r) = ret {
                    r.for_each_named(f);
                }
            }
            _ => {}
        }
    }

    /// Whether `T[]` or `T?` can follow this type's display without parentheses.
    fn postfix_safe(&self) -> bool {
        match self {
            Ty::Union(_) => false,
            Ty::Literals(ls) => ls.len() == 1,
            Ty::Function { ret, .. } => ret.is_none(),
            Ty::Optional(t) => !matches!(**t, Ty::Union(_) | Ty::Literals(_)),
            _ => true,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::String => f.write_str("string"),
            Ty::Integer => f.write_str("integer"),
            Ty::Number => f.write_str("number"),
            Ty::Boolean => f.write_str("boolean"),
            Ty::Table => f.write_str("table"),
            Ty::Any => f.write_str("any"),
            Ty::Nil => f.write_str("nil"),
            // A trailing `?` makes a whole union optional, so no parentheses there.
            Ty::Optional(t) if matches!(**t, Ty::Union(_) | Ty::Literals(_)) => write!(f, "{t}?"),
            Ty::Optional(t) if t.postfix_safe() => write!(f, "{t}?"),
            Ty::Optional(t) => write!(f, "({t})?"),
            Ty::List(t) if t.postfix_safe() => write!(f, "{t}[]"),
            Ty::List(t) => write!(f, "({t})[]"),
            Ty::Map(k, v) => write!(f, "{{{k}: {v}}}"),
            Ty::Literals(ls) => {
                for (i, l) in ls.iter().enumerate() {
                    let sep = if i == 0 { "" } else { "|" };
                    write!(f, "{sep}\"{l}\"")?;
                }
                Ok(())
            }
            Ty::Union(ts) => {
                for (i, t) in ts.iter().enumerate() {
                    let sep = if i == 0 { "" } else { "|" };
                    // A function's return type would swallow the members after it.
                    if matches!(t, Ty::Function { ret: Some(_), .. }) {
                        write!(f, "{sep}({t})")?;
                    } else {
                        write!(f, "{sep}{t}")?;
                    }
                }
                Ok(())
            }
            Ty::Function { params, ret } => {
                f.write_str("function(")?;
                for (i, (name, t)) in params.iter().enumerate() {
                    let sep = if i == 0 { "" } else { ", " };
                    write!(f, "{sep}{name}: {t}")?;
                }
                f.write_str(")")?;
                match ret {
                    Some(r) => write!(f, ": {r}"),
                    None => Ok(()),
                }
            }
            Ty::Named(n) => f.write_str(n),
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> String {
        format!("{msg} at column {} in {:?}", self.pos + 1, self.src)
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn ws(&mut self) {
        let trimmed = self.rest().trim_start();
        self.pos = self.src.len() - trimmed.len();
    }

    /// Skips whitespace, then consumes `tok` if it comes next.
    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.rest().starts_with(tok) {
            self.pos += tok.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &str) -> Result<(), String> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(self.err(&format!("expected `{tok}`")))
        }
    }

    fn ident(&mut self) -> Option<&str> {
        self.ws();
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if len == 0 || rest.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        let start = self.pos;
        self.pos += len;
        Some(&self.src[start..self.pos])
    }

    /// `postfix ('|' postfix)*`
    fn union(&mut self) -> Result<Ty, String> {
        let mut members = Vec::new();
        let mut optional = false;
        loop {
            let mut t = self.postfix()?;
            if let Ty::Optional(inner) = t {
                optional = true;
                t = *inner;
            }
            match t {
                Ty::Union(ts) => members.extend(ts),
                t => members.push(t),
            }
            if !self.eat("|") {
                break;
            }
        }
        // Merge neighbouring literals into one `Literals`.
        let mut merged: Vec<Ty> = Vec::new();
        for t in members {
            match (merged.last_mut(), t) {
                (Some(Ty::Literals(a)), Ty::Literals(b)) => a.extend(b),
                (_, t) => merged.push(t),
            }
        }
        let ty = if merged.len() == 1 {
            merged.remove(0)
        } else {
            Ty::Union(merged)
        };
        Ok(if optional {
            Ty::Optional(Box::new(ty))
        } else {
            ty
        })
    }

    /// `primary ('[]' | '?')*`
    fn postfix(&mut self) -> Result<Ty, String> {
        let mut t = self.primary()?;
        loop {
            if self.eat("[]") {
                t = Ty::List(Box::new(t));
            } else if self.eat("?") {
                if matches!(t, Ty::Optional(_)) {
                    return Err(self.err("`?` twice"));
                }
                t = Ty::Optional(Box::new(t));
            } else {
                return Ok(t);
            }
        }
    }

    fn primary(&mut self) -> Result<Ty, String> {
        if self.eat("(") {
            let t = self.union()?;
            self.expect(")")?;
            return Ok(t);
        }
        if self.eat("{") {
            let k = self.union()?;
            self.expect(":")?;
            let v = self.union()?;
            self.expect("}")?;
            return Ok(Ty::Map(Box::new(k), Box::new(v)));
        }
        if self.eat("\"") {
            let Some(len) = self.rest().find('"') else {
                return Err(self.err("unterminated string literal"));
            };
            let lit = self.rest()[..len].to_owned();
            self.pos += len + 1;
            return Ok(Ty::Literals(vec![lit]));
        }
        let Some(word) = self.ident() else {
            return Err(self.err("expected a type"));
        };
        Ok(match word {
            "string" => Ty::String,
            "integer" => Ty::Integer,
            "number" => Ty::Number,
            "boolean" => Ty::Boolean,
            "table" => Ty::Table,
            "any" => Ty::Any,
            "nil" => Ty::Nil,
            "function" => self.function()?,
            name => Ty::Named(name.to_owned()),
        })
    }

    /// After `function`: `(name: T, ...)` and an optional `: R`.
    fn function(&mut self) -> Result<Ty, String> {
        self.expect("(")?;
        let mut params = Vec::new();
        if !self.eat(")") {
            loop {
                let Some(name) = self.ident() else {
                    return Err(self.err("expected a parameter name"));
                };
                let name = name.to_owned();
                self.expect(":")?;
                params.push((name, self.union()?));
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let ret = if self.eat(":") {
            Some(Box::new(self.union()?))
        } else {
            None
        };
        Ok(Ty::Function { params, ret })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &str) {
        let t = parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(t.to_string(), s, "display of {t:?}");
        assert_eq!(parse(&t.to_string()), Ok(t));
    }

    #[test]
    fn every_type_in_the_api_parses_and_round_trips() {
        let mut n = 0;
        for f in crate::FUNCTIONS {
            for p in f.params {
                round_trip(p.ty);
                n += 1;
            }
            f.returns.into_iter().for_each(round_trip);
        }
        for e in crate::EVENTS {
            round_trip(e.payload);
            e.result.into_iter().for_each(round_trip);
        }
        for t in crate::TYPES {
            for field in t.fields {
                round_trip(field.ty);
            }
        }
        assert!(n > 30, "only {n} params seen");
    }

    #[test]
    fn shapes() {
        assert_eq!(
            parse(r#""info"|"warning"|"error"?"#),
            Ok(Ty::Optional(Box::new(Ty::Literals(vec![
                "info".into(),
                "warning".into(),
                "error".into()
            ]))))
        );
        assert_eq!(
            parse("function()?"),
            Ok(Ty::Optional(Box::new(Ty::Function {
                params: vec![],
                ret: None
            })))
        );
        assert_eq!(
            parse("function(a: table): string|ToolResult"),
            Ok(Ty::Function {
                params: vec![("a".into(), Ty::Table)],
                ret: Some(Box::new(Ty::Union(vec![
                    Ty::String,
                    Ty::Named("ToolResult".into())
                ]))),
            })
        );
        assert_eq!(
            parse("{string: ParamSpec}"),
            Ok(Ty::Map(
                Box::new(Ty::String),
                Box::new(Ty::Named("ParamSpec".into()))
            ))
        );
        assert_eq!(
            parse("string[]?"),
            Ok(Ty::Optional(Box::new(Ty::List(Box::new(Ty::String)))))
        );
        // Whitespace is free; display is canonical.
        assert_eq!(
            parse(" function ( x : integer ) : string [] ")
                .unwrap()
                .to_string(),
            "function(x: integer): string[]"
        );
    }

    #[test]
    fn display_adds_parentheses_where_needed() {
        for s in [
            "(string|integer)[]",
            r#"("a"|"b")[]"#,
            "(function(): string)?",
            "(function(): string)[]",
            "(function(): string)|nil",
            "function()?[]",
            "string?[]",
            "(string|integer?)[]",
            "{string: function(x: any): boolean}",
        ] {
            round_trip(s);
        }
    }

    #[test]
    fn junk_is_an_error() {
        for s in [
            "",
            "string?|",
            "string??",
            "[]",
            "function(x)",
            "function(x: string",
            "{string}",
            "\"open",
            "string integer",
            "1abc",
            "string!",
        ] {
            assert!(parse(s).is_err(), "{s:?} parsed as {:?}", parse(s));
        }
    }
}
