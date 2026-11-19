//! Custom G-code templates in Orca/PrusaSlicer syntax, so printer profiles can be pasted as
//! they are: `[variable]`, `{expression}`, `{if …}…{elsif …}…{else}…{endif}`, with numbers,
//! strings, arithmetic, comparisons, `&&`/`||`/`!` (also `and`/`or`/`not`), `min`/`max`/
//! `abs`/`int`/`round` and array indices (`name[0]`; single-extruder values ignore the index).

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Val {
    fn num(&self) -> Result<f64, String> {
        match self {
            Val::Num(n) => Ok(*n),
            Val::Bool(b) => Ok(*b as i32 as f64),
            Val::Str(s) => s.trim().parse::<f64>().map_err(|_| format!("„{}“ ist keine Zahl", s)),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Val::Num(n) => *n != 0.0,
            Val::Bool(b) => *b,
            Val::Str(s) => !s.is_empty(),
        }
    }
    pub fn text(&self) -> String {
        match self {
            Val::Num(n) => fmt_num(*n),
            Val::Bool(b) => if *b { "true".into() } else { "false".into() },
            Val::Str(s) => s.clone(),
        }
    }
}

/// Numbers as Orca prints them: integers without decimals, otherwise trimmed.
pub fn fmt_num(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 && n.abs() < 1e15 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{:.4}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub type Vars = HashMap<String, Val>;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Id(String),
    Op(&'static str),
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == 'e' || ((c[i] == '-' || c[i] == '+') && i > st && c[i - 1] == 'e')) {
                i += 1;
            }
            let t: String = c[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("Zahl „{}“", t))?));
        } else if ch == '"' || ch == '\'' {
            let q = ch;
            i += 1;
            let st = i;
            while i < c.len() && c[i] != q {
                i += 1;
            }
            if i >= c.len() {
                return Err("Text ohne Ende".into());
            }
            out.push(Tok::Str(c[st..i].iter().collect()));
            i += 1;
        } else if ch.is_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let id: String = c[st..i].iter().collect();
            match id.as_str() {
                "and" => out.push(Tok::Op("&&")),
                "or" => out.push(Tok::Op("||")),
                "not" => out.push(Tok::Op("!")),
                _ => out.push(Tok::Id(id)),
            }
        } else {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            let op2 = ["==", "!=", "<=", ">=", "&&", "||", "<>"].iter().find(|o| **o == two);
            if let Some(o) = op2 {
                out.push(Tok::Op(if *o == "<>" { "!=" } else { o }));
                i += 2;
                continue;
            }
            out.push(match ch {
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                '[' => Tok::LBrack,
                ']' => Tok::RBrack,
                ',' => Tok::Comma,
                '+' => Tok::Op("+"),
                '-' => Tok::Op("-"),
                '*' => Tok::Op("*"),
                '/' => Tok::Op("/"),
                '%' => Tok::Op("%"),
                '<' => Tok::Op("<"),
                '>' => Tok::Op(">"),
                '!' => Tok::Op("!"),
                '=' => Tok::Op("=="),
                _ => return Err(format!("Zeichen „{}“", ch)),
            });
            i += 1;
        }
    }
    Ok(out)
}

struct Parser<'a> {
    t: Vec<Tok>,
    i: usize,
    vars: &'a Vars,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn eat_op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Op(o)) = self.peek() {
            if let Some(f) = ops.iter().find(|x| *x == o) {
                self.i += 1;
                return Some(f);
            }
        }
        None
    }
    fn or(&mut self) -> Result<Val, String> {
        let mut a = self.and()?;
        while self.eat_op(&["||"]).is_some() {
            let b = self.and()?;
            a = Val::Bool(a.truthy() || b.truthy());
        }
        Ok(a)
    }
    fn and(&mut self) -> Result<Val, String> {
        let mut a = self.cmp()?;
        while self.eat_op(&["&&"]).is_some() {
            let b = self.cmp()?;
            a = Val::Bool(a.truthy() && b.truthy());
        }
        Ok(a)
    }
    fn cmp(&mut self) -> Result<Val, String> {
        let a = self.add()?;
        if let Some(op) = self.eat_op(&["==", "!=", "<", "<=", ">", ">="]) {
            let b = self.add()?;
            let r = match (&a, &b) {
                (Val::Str(x), Val::Str(y)) => match op {
                    "==" => x == y,
                    "!=" => x != y,
                    "<" => x < y,
                    "<=" => x <= y,
                    ">" => x > y,
                    _ => x >= y,
                },
                _ => {
                    let (x, y) = (a.num()?, b.num()?);
                    match op {
                        "==" => (x - y).abs() < 1e-9,
                        "!=" => (x - y).abs() >= 1e-9,
                        "<" => x < y,
                        "<=" => x <= y,
                        ">" => x > y,
                        _ => x >= y,
                    }
                }
            };
            return Ok(Val::Bool(r));
        }
        Ok(a)
    }
    fn add(&mut self) -> Result<Val, String> {
        let mut a = self.mul()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let b = self.mul()?;
            a = match (op, &a, &b) {
                ("+", Val::Str(x), _) => Val::Str(format!("{}{}", x, b.text())),
                ("+", _, Val::Str(y)) => Val::Str(format!("{}{}", a.text(), y)),
                ("+", _, _) => Val::Num(a.num()? + b.num()?),
                _ => Val::Num(a.num()? - b.num()?),
            };
        }
        Ok(a)
    }
    fn mul(&mut self) -> Result<Val, String> {
        let mut a = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "/", "%"]) {
            let b = self.unary()?.num()?;
            let x = a.num()?;
            a = Val::Num(match op {
                "*" => x * b,
                "/" => {
                    if b == 0.0 {
                        return Err("Division durch 0".into());
                    }
                    x / b
                }
                _ => {
                    if b == 0.0 {
                        return Err("Division durch 0".into());
                    }
                    x % b
                }
            });
        }
        Ok(a)
    }
    fn unary(&mut self) -> Result<Val, String> {
        if self.eat_op(&["-"]).is_some() {
            return Ok(Val::Num(-self.unary()?.num()?));
        }
        if self.eat_op(&["+"]).is_some() {
            return Ok(Val::Num(self.unary()?.num()?));
        }
        if self.eat_op(&["!"]).is_some() {
            return Ok(Val::Bool(!self.unary()?.truthy()));
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<Val, String> {
        let t = self.peek().cloned().ok_or("Ausdruck endet zu früh")?;
        self.i += 1;
        match t {
            Tok::Num(n) => Ok(Val::Num(n)),
            Tok::Str(s) => Ok(Val::Str(s)),
            Tok::LParen => {
                let v = self.or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err("„)“ fehlt".into());
                }
                self.i += 1;
                Ok(v)
            }
            Tok::Id(id) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = vec![];
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.or()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.i += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if self.peek() != Some(&Tok::RParen) {
                        return Err("„)“ fehlt".into());
                    }
                    self.i += 1;
                    let n: Vec<f64> = args.iter().map(|a| a.num()).collect::<Result<_, _>>()?;
                    let need = |k: usize| if n.len() == k { Ok(()) } else { Err(format!("{}() braucht {} Werte", id, k)) };
                    return Ok(Val::Num(match id.as_str() {
                        "min" if !n.is_empty() => n.iter().cloned().fold(f64::INFINITY, f64::min),
                        "max" if !n.is_empty() => n.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                        "abs" => {
                            need(1)?;
                            n[0].abs()
                        }
                        "int" => {
                            need(1)?;
                            n[0].trunc()
                        }
                        "round" => {
                            need(1)?;
                            n[0].round()
                        }
                        "ceil" => {
                            need(1)?;
                            n[0].ceil()
                        }
                        "floor" => {
                            need(1)?;
                            n[0].floor()
                        }
                        _ => return Err(format!("Funktion „{}“ unbekannt", id)),
                    }));
                }
                if self.peek() == Some(&Tok::LBrack) {
                    // array index: one extruder, every index gives the same value
                    self.i += 1;
                    let st = self.i;
                    let iv = self.or()?;
                    if self.peek() != Some(&Tok::RBrack) {
                        return Err("„]“ fehlt".into());
                    }
                    self.i += 1;
                    // first_layer_print_min[1] and similar real 2-element values
                    let idx = match (&self.t[st], self.i - st) {
                        (Tok::Num(_), 2) => iv.num().ok().map(|n| n as usize),
                        _ => None,
                    };
                    if let Some(k) = idx {
                        if let Some(v) = self.vars.get(&format!("{}[{}]", id, k)) {
                            return Ok(v.clone());
                        }
                    }
                }
                match id.as_str() {
                    "true" => Ok(Val::Bool(true)),
                    "false" => Ok(Val::Bool(false)),
                    _ => self.vars.get(&id).cloned().ok_or(format!("Variable „{}“ unbekannt", id)),
                }
            }
            _ => Err("unerwartetes Zeichen".into()),
        }
    }
}

pub fn eval(expr: &str, vars: &Vars) -> Result<Val, String> {
    let t = lex(expr)?;
    let mut p = Parser { t, i: 0, vars };
    let v = p.or()?;
    if p.i != p.t.len() {
        return Err(format!("unerwartetes Ende in „{}“", expr));
    }
    Ok(v)
}

/// Expands a template. Unknown `[names]` are left as they are (G-code may use brackets);
/// errors in `{…}` blocks are reported and the block is dropped.
pub fn render(src: &str, vars: &Vars) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut errs = vec![];
    // stack of (this branch active, some branch already taken, parent active)
    let mut stack: Vec<(bool, bool, bool)> = vec![];
    let active = |s: &Vec<(bool, bool, bool)>| s.last().map(|x| x.0 && x.2).unwrap_or(true);
    let b = src.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let c = b[i];
        if c == b'{' {
            if let Some(j) = src[i + 1..].find('}') {
                let inner = src[i + 1..i + 1 + j].trim();
                i += j + 2;
                let kw_end = inner.find(|ch: char| !(ch.is_alphanumeric() || ch == '_')).unwrap_or(inner.len());
                let kw = &inner[..kw_end];
                match kw {
                    "if" => {
                        let parent = active(&stack);
                        let v = if parent { eval(&inner[2..], vars) } else { Ok(Val::Bool(false)) };
                        let t = match v {
                            Ok(v) => v.truthy(),
                            Err(e) => {
                                errs.push(e);
                                false
                            }
                        };
                        stack.push((t, t, parent));
                    }
                    "elsif" | "elif" => {
                        if let Some(top) = stack.last_mut() {
                            if top.1 || !top.2 {
                                top.0 = false;
                            } else {
                                let t = match eval(&inner[kw.len()..], vars) {
                                    Ok(v) => v.truthy(),
                                    Err(e) => {
                                        errs.push(e);
                                        false
                                    }
                                };
                                top.0 = t;
                                top.1 = t;
                            }
                        } else {
                            errs.push("„elsif“ ohne „if“".into());
                        }
                    }
                    "else" => {
                        if let Some(top) = stack.last_mut() {
                            top.0 = !top.1;
                            top.1 = true;
                        } else {
                            errs.push("„else“ ohne „if“".into());
                        }
                    }
                    "endif" => {
                        if stack.pop().is_none() {
                            errs.push("„endif“ ohne „if“".into());
                        }
                    }
                    _ => {
                        if active(&stack) {
                            match eval(inner, vars) {
                                Ok(v) => out.push_str(&v.text()),
                                Err(e) => errs.push(e),
                            }
                        }
                    }
                }
                continue;
            }
        }
        if c == b'[' {
            if let Some(j) = src[i + 1..].find(']') {
                let name = &src[i + 1..i + 1 + j];
                if !name.is_empty() && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
                    if let Some(v) = vars.get(name) {
                        if active(&stack) {
                            out.push_str(&v.text());
                        }
                        i += j + 2;
                        continue;
                    }
                }
            }
        }
        let ch = src[i..].chars().next().unwrap();
        if active(&stack) {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    if !stack.is_empty() {
        errs.push("„endif“ fehlt".into());
    }
    (out, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let mut v = Vars::new();
        v.insert("bed".into(), Val::Num(60.0));
        v.insert("filament_type".into(), Val::Str("PLA".into()));
        v.insert("max_layer_z".into(), Val::Num(30.0));
        v.insert("printable_height".into(), Val::Num(260.0));
        v.insert("z_offset".into(), Val::Num(0.0));
        v.insert("first_layer_print_min[0]".into(), Val::Num(12.5));
        v.insert("first_layer_print_min[1]".into(), Val::Num(40.0));
        v.insert("initial_no_support_extruder".into(), Val::Num(0.0));
        v
    }

    #[test]
    fn expands_orca_templates() {
        let v = vars();
        let (s, e) = render("M190 S[bed]\n{if filament_type[initial_no_support_extruder]==\"ABS\"}M191 S47\n{endif}G1 X{first_layer_print_min[0]} Y{first_layer_print_min[1]}", &v);
        assert!(e.is_empty(), "{:?}", e);
        assert_eq!(s, "M190 S60\nG1 X12.5 Y40");
        let (s, e) = render("{if max_layer_z < printable_height}G1 Z{z_offset+min(max_layer_z+2, printable_height)} F300{endif}", &v);
        assert!(e.is_empty(), "{:?}", e);
        assert_eq!(s, "G1 Z32 F300");
        let (s, _) = render("{if bed > 100}hot{elsif bed > 50}warm{else}cold{endif}", &v);
        assert_eq!(s, "warm");
        let (s, _) = render("M106 S{50*255/100.0}", &v);
        assert_eq!(s, "M106 S127.5");
        // unknown brackets stay
        let (s, _) = render("; [not_a_var] [bed]", &v);
        assert_eq!(s, "; [not_a_var] 60");
        let (_, e) = render("{if x}", &v);
        assert!(!e.is_empty());
    }
}
