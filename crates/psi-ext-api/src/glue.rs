//! Argument checks used by the generated glue. Each check reads the raw
//! `mlua::Value` so the error can name the function, argument and types.

use std::fmt;

use mlua::{Function, Value};

/// An API error as Lua sees it: just the message (no "runtime error:" prefix).
#[derive(Debug)]
struct ApiError(String);

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApiError {}

fn error(msg: String) -> mlua::Error {
    mlua::Error::external(ApiError(msg))
}

/// Where an argument sits: `psi.ui.confirm`, 2, `message`.
pub(crate) struct Arg {
    pub func: &'static str,
    pub index: usize,
    pub name: &'static str,
}

impl Arg {
    fn mismatch(&self, expected: &str, got: &str) -> mlua::Error {
        error(format!(
            "{}: argument {} '{}' expected {expected}, got {got}",
            self.func, self.index, self.name
        ))
    }
}

/// The Lua `type()` of a value.
pub(crate) fn lua_type(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Boolean(_) => "boolean",
        Value::Integer(_) | Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Table(_) => "table",
        Value::Function(_) => "function",
        Value::Thread(_) => "thread",
        _ => "userdata",
    }
}

/// Refuses more than `max` arguments.
pub(crate) fn arity(func: &str, got: usize, min: usize, max: usize) -> mlua::Result<()> {
    if got <= max {
        return Ok(());
    }
    let s = if max == 1 { "" } else { "s" };
    Err(error(match max {
        0 => format!("{func}: expected no arguments, got {got}"),
        _ if min == max => format!("{func}: expected {max} argument{s}, got {got}"),
        _ => format!("{func}: expected at most {max} argument{s}, got {got}"),
    }))
}

/// `nil` is `None`; anything else goes through `check`.
pub(crate) fn optional<T>(
    v: Value,
    check: impl FnOnce(Value) -> mlua::Result<T>,
) -> mlua::Result<Option<T>> {
    if v.is_nil() {
        Ok(None)
    } else {
        check(v).map(Some)
    }
}

pub(crate) fn string(a: &Arg, v: Value) -> mlua::Result<String> {
    match v {
        Value::String(s) => Ok(s.to_string_lossy()),
        v => Err(a.mismatch("string", lua_type(&v))),
    }
}

/// Also accepts a float with an exact integer value, as Lua does.
pub(crate) fn integer(a: &Arg, v: Value) -> mlua::Result<i64> {
    match v {
        Value::Integer(i) => Ok(i),
        // The range check keeps the cast exact.
        Value::Number(n) if n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64 => {
            Ok(n as i64)
        }
        v => Err(a.mismatch("integer", lua_type(&v))),
    }
}

pub(crate) fn number(a: &Arg, v: Value) -> mlua::Result<f64> {
    match v {
        Value::Integer(i) => Ok(i as f64),
        Value::Number(n) => Ok(n),
        v => Err(a.mismatch("number", lua_type(&v))),
    }
}

pub(crate) fn boolean(a: &Arg, v: Value) -> mlua::Result<bool> {
    match v {
        Value::Boolean(b) => Ok(b),
        v => Err(a.mismatch("boolean", lua_type(&v))),
    }
}

pub(crate) fn function(a: &Arg, v: Value) -> mlua::Result<Function> {
    match v {
        Value::Function(f) => Ok(f),
        v => Err(a.mismatch("function", lua_type(&v))),
    }
}

/// One of `allowed`, like `"info"|"warning"|"error"`.
pub(crate) fn literal(a: &Arg, v: Value, allowed: &[&str]) -> mlua::Result<String> {
    let got = match &v {
        Value::String(s) => {
            let s = s.to_string_lossy();
            if allowed.contains(&s.as_str()) {
                return Ok(s);
            }
            format!("{s:?}")
        }
        v => lua_type(v).to_owned(),
    };
    let expected: Vec<String> = allowed.iter().map(|l| format!("\"{l}\"")).collect();
    Err(a.mismatch(&expected.join("|"), &got))
}

/// A sequence of strings.
pub(crate) fn string_list(a: &Arg, v: Value) -> mlua::Result<Vec<String>> {
    let Value::Table(t) = v else {
        return Err(a.mismatch("string[]", lua_type(&v)));
    };
    let mut out = Vec::with_capacity(t.raw_len());
    for i in 1..=t.raw_len() {
        match t.raw_get::<Value>(i)? {
            Value::String(s) => out.push(s.to_string_lossy()),
            v => return Err(a.mismatch("string[]", &format!("{} at [{i}]", lua_type(&v)))),
        }
    }
    Ok(out)
}

/// Any other type, checked shallowly by `ok` and passed on as is.
pub(crate) fn value(
    a: &Arg,
    v: Value,
    ok: impl Fn(&Value) -> bool,
    expected: &str,
) -> mlua::Result<Value> {
    if ok(&v) {
        Ok(v)
    } else {
        Err(a.mismatch(expected, lua_type(&v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Arg = Arg {
        func: "psi.x",
        index: 1,
        name: "n",
    };

    #[test]
    fn integer_accepts_whole_floats_only() {
        assert_eq!(integer(&AT, Value::Integer(3)).unwrap(), 3);
        assert_eq!(integer(&AT, Value::Number(2.0)).unwrap(), 2);
        assert_eq!(
            integer(&AT, Value::Number(1.5)).unwrap_err().to_string(),
            "psi.x: argument 1 'n' expected integer, got number"
        );
        assert!(integer(&AT, Value::Number(f64::NAN)).is_err());
        assert!(integer(&AT, Value::Number(1e300)).is_err());
    }

    #[test]
    fn number_and_boolean() {
        assert_eq!(number(&AT, Value::Integer(2)).unwrap(), 2.0);
        assert_eq!(
            number(&AT, Value::Boolean(true)).unwrap_err().to_string(),
            "psi.x: argument 1 'n' expected number, got boolean"
        );
        assert!(boolean(&AT, Value::Boolean(false)).is_ok());
        assert_eq!(
            boolean(&AT, Value::Nil).unwrap_err().to_string(),
            "psi.x: argument 1 'n' expected boolean, got nil"
        );
    }

    #[test]
    fn arity_messages() {
        let msg = |got, min, max| arity("psi.x", got, min, max).unwrap_err().to_string();
        assert!(arity("psi.x", 2, 1, 2).is_ok());
        assert_eq!(msg(1, 0, 0), "psi.x: expected no arguments, got 1");
        assert_eq!(msg(2, 1, 1), "psi.x: expected 1 argument, got 2");
        assert_eq!(msg(3, 2, 2), "psi.x: expected 2 arguments, got 3");
        assert_eq!(msg(3, 1, 2), "psi.x: expected at most 2 arguments, got 3");
    }
}
