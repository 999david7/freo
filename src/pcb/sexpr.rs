//! S-expressions as used by KiCad's .kicad_sch / .kicad_pcb files.

#[derive(Clone, Debug, PartialEq)]
pub enum Sx {
    Atom(String),
    Str(String),
    List(Vec<Sx>),
}

impl Sx {
    pub fn list(items: Vec<Sx>) -> Sx {
        Sx::List(items)
    }
    pub fn head(&self) -> Option<&str> {
        match self {
            Sx::List(v) => match v.first() {
                Some(Sx::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn items(&self) -> &[Sx] {
        match self {
            Sx::List(v) => v,
            _ => &[],
        }
    }
    /// Text of an atom or string.
    pub fn text(&self) -> Option<&str> {
        match self {
            Sx::Atom(a) | Sx::Str(a) => Some(a),
            _ => None,
        }
    }
    pub fn num(&self) -> Option<f64> {
        self.text()?.parse().ok()
    }
    /// First child list with this head.
    pub fn get(&self, key: &str) -> Option<&Sx> {
        self.items().iter().find(|c| c.head() == Some(key))
    }
    pub fn all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Sx> + 'a {
        self.items().iter().filter(move |c| c.head() == Some(key))
    }
    /// n-th element (0 = head) as text.
    pub fn at(&self, n: usize) -> Option<&str> {
        self.items().get(n).and_then(|c| c.text())
    }
    pub fn atn(&self, n: usize) -> Option<f64> {
        self.items().get(n).and_then(|c| c.num())
    }
    /// Value of a (key value) child.
    pub fn val(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|c| c.at(1))
    }
    pub fn valn(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|c| c.atn(1))
    }
    /// KiCad property by name: (property "Reference" "R1" …).
    pub fn property(&self, name: &str) -> Option<&str> {
        self.all("property").find(|p| p.at(1) == Some(name)).and_then(|p| p.at(2))
    }
}

pub fn parse(src: &str) -> Result<Sx, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Vec<Sx>> = vec![vec![]];
    while i < b.len() {
        let c = b[i];
        match c {
            b'(' => {
                stack.push(vec![]);
                i += 1;
            }
            b')' => {
                let done = stack.pop().ok_or("Klammerfehler")?;
                stack.last_mut().ok_or("Klammerfehler: zu viele ')'")?.push(Sx::List(done));
                i += 1;
            }
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        i += 1;
                        s.push(match b[i] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            o => o,
                        });
                    } else {
                        s.push(b[i]);
                    }
                    i += 1;
                }
                i += 1;
                stack.last_mut().ok_or("Klammerfehler")?.push(Sx::Str(String::from_utf8_lossy(&s).into_owned()));
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let st = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'(' && b[i] != b')' {
                    i += 1;
                }
                stack.last_mut().ok_or("Klammerfehler")?.push(Sx::Atom(String::from_utf8_lossy(&b[st..i]).into_owned()));
            }
        }
    }
    if stack.len() != 1 {
        return Err("Unvollständige Datei: Klammern nicht geschlossen".into());
    }
    let mut top = stack.pop().unwrap();
    match top.len() {
        1 => Ok(top.pop().unwrap()),
        0 => Err("Leere Datei".into()),
        _ => Ok(Sx::List(top)),
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Pretty printer close to KiCad's own layout (short lists stay on one line).
pub fn write(sx: &Sx) -> String {
    let mut out = String::new();
    emit(sx, 0, &mut out);
    out.push('\n');
    out
}

fn flat(sx: &Sx) -> Option<String> {
    match sx {
        Sx::Atom(a) => Some(a.clone()),
        Sx::Str(s) => Some(format!("\"{}\"", esc(s))),
        Sx::List(v) => {
            if v.iter().any(|c| matches!(c, Sx::List(k) if k.iter().any(|d| matches!(d, Sx::List(_))))) || v.len() > 12 {
                return None;
            }
            let parts: Option<Vec<String>> = v.iter().map(flat).collect();
            let s = format!("({})", parts?.join(" "));
            if s.len() > 100 {
                None
            } else {
                Some(s)
            }
        }
    }
}

fn emit(sx: &Sx, depth: usize, out: &mut String) {
    if let Some(s) = flat(sx) {
        out.push_str(&s);
        return;
    }
    let Sx::List(v) = sx else { return };
    out.push('(');
    let mut first = true;
    for c in v {
        if first {
            emit(c, depth + 1, out);
            first = false;
            continue;
        }
        if matches!(c, Sx::List(_)) {
            out.push('\n');
            out.push_str(&"\t".repeat(depth + 1));
        } else {
            out.push(' ');
        }
        emit(c, depth + 1, out);
    }
    if v.iter().skip(1).any(|c| matches!(c, Sx::List(_))) {
        out.push('\n');
        out.push_str(&"\t".repeat(depth));
    }
    out.push(')');
}

/// Builders.
pub fn a(s: &str) -> Sx {
    Sx::Atom(s.into())
}
pub fn s(t: &str) -> Sx {
    Sx::Str(t.into())
}
pub fn n(v: f64) -> Sx {
    let r = (v * 1e6).round() / 1e6;
    let t = format!("{}", r);
    Sx::Atom(if t == "-0" { "0".into() } else { t })
}
pub fn l(head: &str, rest: Vec<Sx>) -> Sx {
    let mut v = vec![a(head)];
    v.extend(rest);
    Sx::List(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let src = "(kicad_pcb (version 20240108) (net 1 \"GND\") (gr_line (start 0 0) (end 10.5 -2) (layer \"Edge.Cuts\")))";
        let sx = parse(src).unwrap();
        assert_eq!(sx.head(), Some("kicad_pcb"));
        assert_eq!(sx.get("net").unwrap().at(2), Some("GND"));
        let again = parse(&write(&sx)).unwrap();
        assert_eq!(sx, again);
    }
}
