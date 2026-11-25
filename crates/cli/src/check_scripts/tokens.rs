// Part of Agent Record Adapter (see NOTICE).
//! Tokens of a JavaScript, TypeScript or Python source: enough to find calls
//! and the string literals passed to them. Comments are dropped; a string
//! with a part filled at run time (a template or an f-string) is `Tmpl`.

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Str(String),
    /// A string with a part filled at run time: the written text before it.
    Tmpl(String),
    Ident(String),
    Num(String),
    Punct(char),
    /// `=>`
    Arrow,
    /// `...`
    Spread,
    /// A regular expression literal.
    Other,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    pub line: usize,
    /// Where it is in the source, as char offsets (start, end).
    pub span: (usize, usize),
}

impl Token {
    pub(crate) fn is(&self, ch: char) -> bool {
        self.tok == Tok::Punct(ch)
    }

    pub(crate) fn ident(&self) -> Option<&str> {
        match &self.tok {
            Tok::Ident(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

const REGEX_AFTER: &str = "(,=:[!&|?{};+-*%<>~^";
const REGEX_AFTER_WORDS: &[&str] = &[
    "return", "typeof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw",
    "yield", "await",
];

pub(crate) fn tokens(text: &str, python: bool) -> Vec<Token> {
    let c: Vec<char> = text.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < c.len() {
        // The token pushed last ends where this iteration starts.
        if let Some(last) = out.last_mut().filter(|last| last.span.1 == 0) {
            last.span.1 = i;
        }
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let next = c.get(i + 1).copied();
        if (python && ch == '#') || (!python && ch == '/' && next == Some('/')) {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if !python && ch == '/' && next == Some('*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                if c[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        let at = line;
        let from = i;
        if python && ch.is_ascii_alphabetic() {
            let mut j = i;
            while j < c.len() && j - i < 3 && c[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j < c.len() && (c[j] == '\'' || c[j] == '"') {
                let prefix: String = c[i..j].iter().collect::<String>().to_ascii_lowercase();
                if prefix.chars().all(|p| "rbfu".contains(p)) {
                    let (tok, end, lines) =
                        python_string(&c, j, prefix.contains('f'), prefix.contains('r'));
                    out.push(Token { tok, line: at, span: (from, 0) });
                    line += lines;
                    i = end;
                    continue;
                }
            }
        }
        if ch == '\'' || ch == '"' {
            let (tok, end, lines) = if python {
                python_string(&c, i, false, false)
            } else {
                js_string(&c, i)
            };
            out.push(Token { tok, line: at, span: (from, 0) });
            line += lines;
            i = end;
            continue;
        }
        if !python && ch == '`' {
            let (tok, end, lines) = template(&c, i);
            out.push(Token { tok, line: at, span: (from, 0) });
            line += lines;
            i = end;
            continue;
        }
        if !python && ch == '/' && regex_allowed(out.last()) {
            i = regex_end(&c, i);
            out.push(Token {
                tok: Tok::Other,
                line: at,
                span: (from, 0),
            });
            continue;
        }
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(c[start..i].iter().collect()),
                line: at,
                span: (from, 0),
            });
            continue;
        }
        if ch.is_ascii_digit() {
            let start = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Num(c[start..i].iter().collect()),
                line: at,
                span: (from, 0),
            });
            continue;
        }
        if !python && ch == '=' && next == Some('>') {
            out.push(Token {
                tok: Tok::Arrow,
                line: at,
                span: (from, 0),
            });
            i += 2;
            continue;
        }
        if ch == '.' && next == Some('.') && c.get(i + 2) == Some(&'.') {
            out.push(Token {
                tok: Tok::Spread,
                line: at,
                span: (from, 0),
            });
            i += 3;
            continue;
        }
        out.push(Token {
            tok: Tok::Punct(ch),
            line: at,
                span: (from, 0),
        });
        i += 1;
    }
    if let Some(last) = out.last_mut().filter(|last| last.span.1 == 0) {
        last.span.1 = i.min(c.len());
    }
    out
}

fn regex_allowed(previous: Option<&Token>) -> bool {
    match previous.map(|token| &token.tok) {
        None | Some(Tok::Arrow) => true,
        Some(Tok::Punct(p)) => REGEX_AFTER.contains(*p),
        Some(Tok::Ident(word)) => REGEX_AFTER_WORDS.contains(&word.as_str()),
        _ => false,
    }
}

fn regex_end(c: &[char], start: usize) -> usize {
    let mut i = start + 1;
    let mut class = false;
    while i < c.len() && c[i] != '\n' {
        match c[i] {
            '\\' => i += 1,
            '[' => class = true,
            ']' => class = false,
            '/' if !class => {
                i += 1;
                while i < c.len() && c[i].is_ascii_alphabetic() {
                    i += 1;
                }
                return i;
            }
            _ => {}
        }
        i += 1;
    }
    i
}

fn escaped(ch: char) -> char {
    match ch {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        other => other,
    }
}

/// A '...' or "..." string of JavaScript: (token, index after it, newlines).
fn js_string(c: &[char], start: usize) -> (Tok, usize, usize) {
    let quote = c[start];
    let mut i = start + 1;
    let mut value = String::new();
    let mut lines = 0;
    while i < c.len() && c[i] != quote && c[i] != '\n' {
        if c[i] == '\\' && i + 1 < c.len() {
            if c[i + 1] == '\n' {
                lines += 1;
            } else {
                value.push(escaped(c[i + 1]));
            }
            i += 2;
            continue;
        }
        value.push(c[i]);
        i += 1;
    }
    (Tok::Str(value), (i + 1).min(c.len()), lines)
}

/// A `...` template: a plain string when it has no `${...}`.
fn template(c: &[char], start: usize) -> (Tok, usize, usize) {
    let mut i = start + 1;
    let mut value = String::new();
    let mut lines = 0;
    let mut filled = false;
    let mut prefix = String::new();
    while i < c.len() && c[i] != '`' {
        if c[i] == '\n' {
            lines += 1;
        }
        if c[i] == '\\' && i + 1 < c.len() {
            value.push(escaped(c[i + 1]));
            i += 2;
            continue;
        }
        if c[i] == '$' && c.get(i + 1) == Some(&'{') {
            if !filled {
                prefix = value.clone();
            }
            filled = true;
            let mut depth = 0;
            while i < c.len() {
                match c[i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    '\n' => lines += 1,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        value.push(c[i]);
        i += 1;
    }
    let tok = if filled {
        Tok::Tmpl(prefix)
    } else {
        Tok::Str(value)
    };
    (tok, (i + 1).min(c.len()), lines)
}

/// A Python string starting at its quote; `f` strings with a `{...}` are filled.
fn python_string(c: &[char], start: usize, f: bool, raw: bool) -> (Tok, usize, usize) {
    let quote = c[start];
    let triple = c.get(start + 1) == Some(&quote) && c.get(start + 2) == Some(&quote);
    let mut i = start + if triple { 3 } else { 1 };
    let mut value = String::new();
    let mut lines = 0;
    let mut filled = false;
    let mut prefix = String::new();
    while i < c.len() {
        if triple {
            if c[i] == quote && c.get(i + 1) == Some(&quote) && c.get(i + 2) == Some(&quote) {
                i += 3;
                break;
            }
        } else if c[i] == quote || c[i] == '\n' {
            i += 1;
            break;
        }
        if c[i] == '\n' {
            lines += 1;
        }
        if c[i] == '\\' && i + 1 < c.len() {
            if raw {
                value.push('\\');
                value.push(c[i + 1]);
            } else {
                value.push(escaped(c[i + 1]));
            }
            i += 2;
            continue;
        }
        if f && c[i] == '{' {
            if c.get(i + 1) == Some(&'{') {
                value.push('{');
                i += 2;
                continue;
            }
            if !filled {
                prefix = value.clone();
            }
            filled = true;
        }
        value.push(c[i]);
        i += 1;
    }
    let tok = if filled {
        Tok::Tmpl(prefix)
    } else {
        Tok::Str(value)
    };
    (tok, i.min(c.len()), lines)
}
