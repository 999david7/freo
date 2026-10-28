//! Expression evaluator for dimensions, parameters and relations.
//! Supports + - * / ^, parentheses, unary minus, parameters (case-insensitive) and
//! sin cos tan asin acos atan (degrees), sqrt abs min max floor ceil round log ln exp, pi.

use std::collections::HashMap;

pub type Vars = HashMap<String, f64>;

struct Parser<'a> {
    c: Vec<char>,
    pos: usize,
    vars: &'a Vars,
}

pub fn eval(text: &str, vars: &Vars) -> Result<f64, String> {
    let mut p = Parser { c: text.chars().collect(), pos: 0, vars };
    p.ws();
    if p.pos >= p.c.len() { return Err("Leerer Ausdruck".into()); }
    let v = p.expr()?;
    p.ws();
    if p.pos < p.c.len() { return Err(format!("Unerwartetes '{}' in \"{}\"", p.c[p.pos], text)); }
    if !v.is_finite() { return Err(format!("Ausdruck \"{}\" ist nicht endlich", text)); }
    Ok(v)
}

impl<'a> Parser<'a> {
    fn ws(&mut self) { while self.pos < self.c.len() && self.c[self.pos].is_whitespace() { self.pos += 1; } }
    fn peek(&mut self) -> Option<char> { self.ws(); self.c.get(self.pos).copied() }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(ch) = self.peek() {
            if ch != '+' && ch != '-' { break; }
            self.pos += 1;
            let r = self.term()?;
            v = if ch == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        while let Some(ch) = self.peek() {
            if ch != '*' && ch != '/' { break; }
            self.pos += 1;
            let r = self.unary()?;
            if ch == '*' { v *= r } else {
                if r == 0.0 { return Err("Division durch null".into()); }
                v /= r;
            }
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64, String> {
        if let Some(ch) = self.peek() {
            if ch == '-' || ch == '+' {
                self.pos += 1;
                let v = self.unary()?;
                return Ok(if ch == '-' { -v } else { v });
            }
        }
        let base = self.atom()?;
        if self.peek() == Some('^') {
            self.pos += 1;
            let e = self.unary()?;
            return Ok(base.powf(e));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<f64, String> {
        let ch = self.peek().ok_or("Unerwartetes Ende des Ausdrucks")?;
        if ch == '(' {
            self.pos += 1;
            let v = self.expr()?;
            if self.peek() != Some(')') { return Err("Fehlende ')'".into()); }
            self.pos += 1;
            return Ok(v);
        }
        if ch.is_ascii_digit() || ch == '.' {
            let mut s = String::new();
            while self.pos < self.c.len() && (self.c[self.pos].is_ascii_digit() || self.c[self.pos] == '.') {
                s.push(self.c[self.pos]);
                self.pos += 1;
            }
            if self.pos < self.c.len() && (self.c[self.pos] == 'e' || self.c[self.pos] == 'E') {
                let save = self.pos;
                let mut e = String::from("e");
                self.pos += 1;
                if self.pos < self.c.len() && (self.c[self.pos] == '-' || self.c[self.pos] == '+') { e.push(self.c[self.pos]); self.pos += 1; }
                let mut digits = String::new();
                while self.pos < self.c.len() && self.c[self.pos].is_ascii_digit() { digits.push(self.c[self.pos]); self.pos += 1; }
                if digits.is_empty() { self.pos = save; } else { s += &e; s += &digits; }
            }
            return s.parse::<f64>().map_err(|_| format!("Ungültige Zahl '{}'", s));
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut name = String::new();
            while self.pos < self.c.len() && (self.c[self.pos].is_alphanumeric() || self.c[self.pos] == '_') {
                name.push(self.c[self.pos]);
                self.pos += 1;
            }
            let upper = name.to_uppercase();
            if self.peek() == Some('(') {
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() != Some(')') {
                    args.push(self.expr()?);
                    while self.peek() == Some(',') { self.pos += 1; args.push(self.expr()?); }
                }
                if self.peek() != Some(')') { return Err(format!("Fehlende ')' nach {}(", name)); }
                self.pos += 1;
                return call(&upper, &args);
            }
            if upper == "PI" { return Ok(std::f64::consts::PI); }
            return self.vars.get(&upper).copied().ok_or(format!("Unbekannter Parameter '{}'", name));
        }
        Err(format!("Unerwartetes '{}'", ch))
    }
}

fn call(f: &str, a: &[f64]) -> Result<f64, String> {
    let one = |v: f64| -> Result<f64, String> {
        if a.len() != 1 { Err(format!("{} erwartet 1 Argument", f.to_lowercase())) } else { Ok(v) }
    };
    let x = a.first().copied().unwrap_or(0.0);
    match f {
        "SIN" => one(x.to_radians().sin()),
        "COS" => one(x.to_radians().cos()),
        "TAN" => one(x.to_radians().tan()),
        "ASIN" => one(x.asin().to_degrees()),
        "ACOS" => one(x.acos().to_degrees()),
        "ATAN" => one(x.atan().to_degrees()),
        "SQRT" => one(x.sqrt()),
        "ABS" => one(x.abs()),
        "FLOOR" => one(x.floor()),
        "CEIL" => one(x.ceil()),
        "ROUND" => one(x.round()),
        "LOG" => one(x.log10()),
        "LN" => one(x.ln()),
        "EXP" => one(x.exp()),
        "MIN" => a.iter().copied().reduce(f64::min).ok_or("min benötigt Argumente".into()),
        "MAX" => a.iter().copied().reduce(f64::max).ok_or("max benötigt Argumente".into()),
        _ => Err(format!("Unbekannte Funktion '{}'", f.to_lowercase())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basics() {
        let mut v = Vars::new();
        v.insert("W".into(), 10.0);
        assert_eq!(eval("w*2+1", &v).unwrap(), 21.0);
        assert!((eval("sin(30)", &v).unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(eval("-2^2", &v).unwrap(), -4.0);
        assert_eq!(eval("max(1, W, 3)", &v).unwrap(), 10.0);
        assert!(eval("foo", &v).is_err());
    }
}
