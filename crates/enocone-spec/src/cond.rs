//! `Only if:` conditions: `{{row.status}} == "Pending" and {{row.age}} >= 18`.
//!
//! Values are `{{…}}` templates, quoted strings, numbers, `true`/`false` and
//! `null`. Comparisons: `== != < <= > >=` and `in [ … ]`; combined with
//! `and`, `or`, `not` and parentheses. A bare value is true unless it is
//! false, null, 0, "" or an empty list.

use crate::template::Resolver;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// The expression inside `{{ }}`, e.g. `row.status`.
    Var(String),
    Lit(Value),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    Or(Box<Cond>, Box<Cond>),
    And(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
    Cmp(Operand, Op, Operand),
    In(Operand, Vec<Operand>),
    Truthy(Operand),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Var(String),
    Lit(Value),
    Op(Op),
    And,
    Or,
    Not,
    In,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
        if two == "{{" {
            let rest: String = cs[i + 2..].iter().collect();
            let end = rest.find("}}").ok_or("a `{{` has no closing `}}`")?;
            let name = rest[..end].trim().to_string();
            if name.is_empty() {
                return Err("`{{}}` names no variable".into());
            }
            i += 2 + rest[..end].chars().count() + 2;
            out.push(Tok::Var(name));
            continue;
        }
        let op = match two.as_str() {
            "==" => Some(Op::Eq),
            "!=" => Some(Op::Ne),
            "<=" => Some(Op::Le),
            ">=" => Some(Op::Ge),
            _ => None,
        };
        if let Some(op) = op {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        match c {
            '<' => out.push(Tok::Op(Op::Lt)),
            '>' => out.push(Tok::Op(Op::Gt)),
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            '[' => out.push(Tok::LBracket),
            ']' => out.push(Tok::RBracket),
            ',' => out.push(Tok::Comma),
            '=' => return Err("use `==` to compare, not `=`".into()),
            '"' | '\'' => {
                let mut j = i + 1;
                let mut text = String::new();
                while j < cs.len() && cs[j] != c {
                    if cs[j] == '\\' && j + 1 < cs.len() {
                        j += 1;
                    }
                    text.push(cs[j]);
                    j += 1;
                }
                if j >= cs.len() {
                    return Err(format!("a string starting {c}{text} has no closing {c}"));
                }
                out.push(Tok::Lit(Value::String(text)));
                i = j + 1;
                continue;
            }
            _ if c.is_ascii_digit() || (c == '-' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let mut j = i + 1;
                while j < cs.len() && (cs[j].is_ascii_digit() || cs[j] == '.') {
                    j += 1;
                }
                let n: String = cs[i..j].iter().collect();
                let v = n.parse::<i64>().map(Value::from).or_else(|_| n.parse::<f64>().map(Value::from)).map_err(|_| format!("`{n}` isn't a number"))?;
                out.push(Tok::Lit(v));
                i = j;
                continue;
            }
            _ if c.is_alphabetic() => {
                let mut j = i;
                while j < cs.len() && (cs[j].is_alphanumeric() || cs[j] == '_') {
                    j += 1;
                }
                let w: String = cs[i..j].iter().collect();
                out.push(match w.as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "in" => Tok::In,
                    "true" => Tok::Lit(Value::Bool(true)),
                    "false" => Tok::Lit(Value::Bool(false)),
                    "null" => Tok::Lit(Value::Null),
                    _ => return Err(format!("`{w}` isn't a value: write text in quotes (\"{w}\"), or a variable as {{{{{w}}}}}")),
                });
                i = j;
                continue;
            }
            _ => return Err(format!("unexpected `{c}`")),
        }
        i += 1;
    }
    Ok(out)
}

struct P {
    toks: Vec<Tok>,
    at: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn or(&mut self) -> Result<Cond, String> {
        let mut l = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.at += 1;
            l = Cond::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Cond, String> {
        let mut l = self.not()?;
        while self.peek() == Some(&Tok::And) {
            self.at += 1;
            l = Cond::And(Box::new(l), Box::new(self.not()?));
        }
        Ok(l)
    }
    fn not(&mut self) -> Result<Cond, String> {
        if self.peek() == Some(&Tok::Not) {
            self.at += 1;
            return Ok(Cond::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Cond, String> {
        if self.peek() == Some(&Tok::LParen) {
            self.at += 1;
            let c = self.or()?;
            if self.next() != Some(Tok::RParen) {
                return Err("a `(` has no closing `)`".into());
            }
            return Ok(c);
        }
        let l = self.operand()?;
        match self.peek().cloned() {
            Some(Tok::Op(op)) => {
                self.at += 1;
                Ok(Cond::Cmp(l, op, self.operand()?))
            }
            Some(Tok::In) => {
                self.at += 1;
                if self.next() != Some(Tok::LBracket) {
                    return Err("`in` needs a list: in [\"a\", \"b\"]".into());
                }
                let mut items = Vec::new();
                if self.peek() == Some(&Tok::RBracket) {
                    self.at += 1;
                    return Ok(Cond::In(l, items));
                }
                loop {
                    items.push(self.operand()?);
                    match self.next() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RBracket) => break,
                        _ => return Err("a list needs commas between its values and a closing `]`".into()),
                    }
                }
                Ok(Cond::In(l, items))
            }
            _ => Ok(Cond::Truthy(l)),
        }
    }
    fn operand(&mut self) -> Result<Operand, String> {
        match self.next() {
            Some(Tok::Var(v)) => Ok(Operand::Var(v)),
            Some(Tok::Lit(v)) => Ok(Operand::Lit(v)),
            Some(t) => Err(format!("expected a value, found {}", describe(&t))),
            None => Err("the condition ends where a value was expected".into()),
        }
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Op(_) => "a comparison".into(),
        Tok::And => "`and`".into(),
        Tok::Or => "`or`".into(),
        Tok::Not => "`not`".into(),
        Tok::In => "`in`".into(),
        Tok::LParen => "`(`".into(),
        Tok::RParen => "`)`".into(),
        Tok::LBracket => "`[`".into(),
        Tok::RBracket => "`]`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Var(v) => format!("{{{{{v}}}}}"),
        Tok::Lit(v) => v.to_string(),
    }
}

pub fn parse(s: &str) -> Result<Cond, String> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err("the condition is empty".into());
    }
    let mut p = P { toks, at: 0 };
    let c = p.or()?;
    if let Some(t) = p.peek() {
        return Err(format!("unexpected {} after a complete condition", describe(t)));
    }
    Ok(c)
}

impl Cond {
    /// The variables it reads, e.g. `row.status`.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |o| {
            if let Operand::Var(v) = o {
                out.push(v.clone());
            }
        });
        out
    }

    fn walk(&self, f: &mut dyn FnMut(&Operand)) {
        match self {
            Cond::Or(a, b) | Cond::And(a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Cond::Not(a) => a.walk(f),
            Cond::Cmp(a, _, b) => {
                f(a);
                f(b);
            }
            Cond::In(a, items) => {
                f(a);
                items.iter().for_each(&mut *f);
            }
            Cond::Truthy(a) => f(a),
        }
    }

    /// Whether it holds, and the values its variables had (for the report).
    pub fn eval(&self, r: &dyn Resolver) -> Result<(bool, Vec<(String, Value)>), String> {
        let mut seen: Vec<(String, Value)> = Vec::new();
        let ok = self.eval_in(r, &mut seen)?;
        Ok((ok, seen))
    }

    fn eval_in(&self, r: &dyn Resolver, seen: &mut Vec<(String, Value)>) -> Result<bool, String> {
        let mut val = |o: &Operand| -> Result<Value, String> {
            match o {
                Operand::Lit(v) => Ok(v.clone()),
                Operand::Var(name) => {
                    let v = r.lookup(name).ok_or_else(|| format!("unknown variable `{name}`"))?;
                    if !seen.iter().any(|(n, _)| n == name) {
                        seen.push((name.clone(), v.clone()));
                    }
                    Ok(v)
                }
            }
        };
        Ok(match self {
            Cond::Or(a, b) => a.eval_in(r, seen)? || b.eval_in(r, seen)?,
            Cond::And(a, b) => a.eval_in(r, seen)? && b.eval_in(r, seen)?,
            Cond::Not(a) => !a.eval_in(r, seen)?,
            Cond::Truthy(a) => truthy(&val(a)?),
            Cond::Cmp(a, op, b) => {
                let (x, y) = (val(a)?, val(b)?);
                compare(&x, *op, &y)?
            }
            Cond::In(a, items) => {
                let x = val(a)?;
                let mut found = false;
                for i in items {
                    if compare(&x, Op::Eq, &val(i)?)? {
                        found = true;
                    }
                }
                found
            }
        })
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) => true,
    }
}

/// A number, or text that reads as one (table cells and captures are often text).
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn compare(x: &Value, op: Op, y: &Value) -> Result<bool, String> {
    if let (Some(a), Some(b)) = (number(x), number(y))
        && (x.is_number() || y.is_number())
    {
        return Ok(match op {
            Op::Eq => a == b,
            Op::Ne => a != b,
            Op::Lt => a < b,
            Op::Le => a <= b,
            Op::Gt => a > b,
            Op::Ge => a >= b,
        });
    }
    match op {
        Op::Eq => Ok(x == y),
        Op::Ne => Ok(x != y),
        _ => match (x, y) {
            (Value::String(a), Value::String(b)) => Ok(match op {
                Op::Lt => a < b,
                Op::Le => a <= b,
                Op::Gt => a > b,
                _ => a >= b,
            }),
            _ => Err(format!("can't order {x} and {y}: `<` and `>` compare numbers or text")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct R(Value);
    impl Resolver for R {
        fn lookup(&self, path: &str) -> Option<Value> {
            crate::template::dig(&self.0, &path.split('.').collect::<Vec<_>>()).cloned()
        }
    }

    fn holds(c: &str, vars: Value) -> bool {
        parse(c).unwrap().eval(&R(vars)).unwrap().0
    }

    #[test]
    fn compares_and_combines() {
        let v = json!({ "row": { "status": "Pending", "age": 20, "n": "7" } });
        assert!(holds(r#"{{row.status}} == "Pending""#, v.clone()));
        assert!(!holds(r#"{{row.status}} != 'Pending'"#, v.clone()));
        assert!(holds("{{row.age}} >= 18 and {{row.age}} < 65", v.clone()));
        assert!(holds("{{row.n}} > 5", v.clone()));
        assert!(holds(r#"{{row.status}} in ["Active", "Pending"]"#, v.clone()));
        assert!(holds(r#"not {{row.status}} in ["Active"]"#, v.clone()));
        assert!(holds(r#"({{row.age}} < 18 or {{row.status}} == "Pending") and true"#, v.clone()));
        assert!(!holds(r#"{{row.age}} < 18 or {{row.status}} == "Active""#, v.clone()));
        assert!(holds("{{row.status}}", v.clone()));
    }

    #[test]
    fn reports_what_it_saw() {
        let (ok, seen) = parse(r#"{{row.status}} == "Pending""#).unwrap().eval(&R(json!({ "row": { "status": "Active" } }))).unwrap();
        assert!(!ok);
        assert_eq!(seen, vec![("row.status".to_string(), json!("Active"))]);
    }

    #[test]
    fn rejects_what_it_cant_read() {
        for (c, msg) in [
            ("{{a}} = 1", "use `==`"),
            ("{{a}} ==", "ends where a value"),
            ("({{a}} == 1", "no closing `)`"),
            ("{{a}} == Pending", "isn't a value"),
            ("{{a}} in [1 2]", "commas"),
            ("{{a}} == 1 {{b}}", "after a complete condition"),
            ("\"open", "no closing"),
            ("", "empty"),
        ] {
            let e = parse(c).unwrap_err();
            assert!(e.contains(msg), "{c}: {e}");
        }
    }
}
