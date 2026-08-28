mod token;

pub use token::{NumTok, Token, TokenKind};

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

pub const KEYWORDS: &[&str] = &[
    "অ্যাপ",
    "ফাংশন",
    "ধরি",
    "ধ্রুবক",
    "ফেরাও",
    "যদি",
    "নাহলে",
    "লুপ",
    "যতক্ষণ",
    "প্রতি",
    "থামো",
    "চলো",
    "ইম্পোর্ট",
    "সত্য",
    "মিথ্যা",
    "ফাঁকা",
    "এবং",
    "অথবা",
    "না",
    "শেয়ার",
    "ডিসপ্লে",
    "ক্যানভাস",
    "টেক্সট",
    "বাটন",
    "ইনপুট",
    "ছবি",
    "সারি",
    "কলাম",
    "কার্ড",
    "ডায়ালগ",
    "স্ক্রল",
    "সংখ্যা",
    "দশমিক",
    "লেখা",
    "সত্যতা",
    "অক্ষর",
    // `ফাঁকা` is both the null literal (listed above with সত্য/মিথ্যা) and the
    // void type name, so it is deliberately listed once and used in both roles.
    "ম্যাপ",
    "ডাটা",
    "চেষ্টা",
    "ধরো",
];

pub fn bn_num(n: u32) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32(0x09E6 + (c as u32 - '0' as u32)).unwrap_or(c),
            _ => c,
        })
        .collect()
}

pub fn format_error(prefix: &str, file: &str, line: u32, col: u32, msg: &str) -> String {
    format!("{}: {}:{}:{}\n\n{}", prefix, file, bn_num(line), bn_num(col), msg)
}

fn is_bangla_digit(c: char) -> bool {
    c >= '\u{09E6}' && c <= '\u{09EF}'
}

fn is_bangla_block(c: char) -> bool {
    c >= '\u{0980}' && c <= '\u{09FF}'
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_part(c: char) -> bool {
    is_ident_start(c) || is_any_digit(c) || is_bangla_block(c) || c == '\u{200C}' || c == '\u{200D}'
}

fn is_any_digit(c: char) -> bool {
    c.is_ascii_digit() || is_bangla_digit(c)
}

fn digit_value(c: char) -> i64 {
    if c.is_ascii_digit() {
        (c as u32 - '0' as u32) as i64
    } else {
        (c as u32 - 0x09E6) as i64
    }
}

fn digit_to_ascii(c: char) -> char {
    if is_bangla_digit(c) {
        char::from_u32('0' as u32 + (c as u32 - 0x09E6)).unwrap_or('0')
    } else {
        c
    }
}

fn one_char_op(c: char) -> Option<&'static str> {
    match c {
        '+' => Some("+"),
        '-' => Some("-"),
        '*' => Some("*"),
        '/' => Some("/"),
        '%' => Some("%"),
        '=' => Some("="),
        '<' => Some("<"),
        '>' => Some(">"),
        ':' => Some(":"),
        ',' => Some(","),
        '.' => Some("."),
        '(' => Some("("),
        ')' => Some(")"),
        '[' => Some("["),
        ']' => Some("]"),
        '{' => Some("{"),
        '}' => Some("}"),
        _ => None,
    }
}

struct Lx {
    cs: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
    toks: Vec<Token>,
    diags: Vec<Diagnostic>,
    opens: Vec<(&'static str, u32, u32)>,
}

impl Lx {
    fn peek(&self) -> Option<char> {
        self.cs.get(self.i).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.cs.get(self.i + 1).copied()
    }

    fn err(&mut self, line: u32, col: u32, message: String) {
        self.diags.push(Diagnostic { line, col, message });
    }

    fn emit_newline(&mut self, line: u32, col: u32) {
        if self.opens.is_empty() {
            self.toks.push(Token {
                kind: TokenKind::Newline,
                line,
                col,
            });
        }
        self.line += 1;
        self.col = 1;
    }

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                '\u{FEFF}' => {
                    self.i += 1;
                }
                ' ' | '\t' | '\u{000C}' => {
                    self.i += 1;
                    self.col += 1;
                }
                '\r' => {
                    let (l, co) = (self.line, self.col);
                    if self.peek2() == Some('\n') {
                        self.i += 2;
                        self.col += 2;
                    } else {
                        self.i += 1;
                        self.col += 1;
                    }
                    self.emit_newline(l, co);
                }
                '\n' => {
                    let (l, co) = (self.line, self.col);
                    self.i += 1;
                    self.emit_newline(l, co);
                }
                '/' => self.slash(),
                '"' => self.string(),
                '\'' => self.char_lit(),
                _ if is_any_digit(c) => self.number(c),
                _ if is_ident_start(c) => self.ident(),
                _ => self.operator(c),
            }
        }
    }

    fn slash(&mut self) {
        if self.peek2() == Some('/') {
            self.i += 2;
            self.col += 2;
            while let Some(c) = self.peek() {
                if c == '\n' || c == '\r' {
                    break;
                }
                self.i += 1;
                self.col += 1;
            }
        } else if self.peek2() == Some('*') {
            let (sl, sc) = (self.line, self.col);
            self.i += 2;
            self.col += 2;
            let mut closed = false;
            while let Some(c) = self.peek() {
                if c == '*' && self.peek2() == Some('/') {
                    self.i += 2;
                    self.col += 2;
                    closed = true;
                    break;
                }
                if c == '\n' {
                    self.line += 1;
                    self.col = 1;
                } else {
                    self.col += 1;
                }
                self.i += 1;
            }
            if !closed {
                self.err(sl, sc, "কমেন্ট বন্ধ হয়নি — '*/' পাওয়া যায়নি".to_string());
            }
        } else {
            self.toks.push(Token {
                kind: TokenKind::Op("/"),
                line: self.line,
                col: self.col,
            });
            self.i += 1;
            self.col += 1;
        }
    }

    fn escape_char(&mut self) -> Option<char> {
        let el = self.line;
        let ec = self.col;
        self.i += 1;
        self.col += 1;
        let e = match self.peek() {
            Some(c) => c,
            None => {
                self.err(
                    el,
                    ec,
                    "এস্কেপ সিকোয়েন্স অসম্পূর্ণ — লিটারেল বন্ধ হয়নি".to_string(),
                );
                return None;
            }
        };
        self.i += 1;
        self.col += 1;
        match e {
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            '\\' => Some('\\'),
            '"' => Some('"'),
            '\'' => Some('\''),
            'u' => self.unicode_escape(el, ec),
            _ => {
                self.err(el, ec, format!("অজানা এস্কেপ সিকোয়েন্স '\\{}'", e));
                None
            }
        }
    }

    fn unicode_escape(&mut self, sl: u32, sc: u32) -> Option<char> {
        if self.peek() != Some('{') {
            self.err(sl, sc, "'\\u'-এর পরে '{' প্রত্যাশিত".to_string());
            return None;
        }
        self.i += 1;
        self.col += 1;
        let mut hex = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_hexdigit() && hex.len() < 6 {
                hex.push(c);
                self.i += 1;
                self.col += 1;
            } else {
                break;
            }
        }
        if hex.is_empty() || self.peek() != Some('}') {
            self.err(sl, sc, "অবৈধ \\u{...} এস্কেপ সিকোয়েন্স".to_string());
            return None;
        }
        self.i += 1;
        self.col += 1;
        let v = u32::from_str_radix(&hex, 16).unwrap_or(0x11_0000);
        match char::from_u32(v) {
            Some(ch) => Some(ch),
            None => {
                self.err(
                    sl,
                    sc,
                    "\\u{...}-এ অবৈধ ইউনিকোড স্কেলার ভ্যালু".to_string(),
                );
                None
            }
        }
    }

    fn string(&mut self) {
        let (sl, sc) = (self.line, self.col);
        self.i += 1;
        self.col += 1;
        let mut s = String::new();
        loop {
            match self.peek() {
                None => {
                    self.err(sl, sc, "লেখা লিটারেল বন্ধ হয়নি — '\"' পাওয়া যায়নি".to_string());
                    break;
                }
                Some('"') => {
                    self.i += 1;
                    self.col += 1;
                    self.toks.push(Token {
                        kind: TokenKind::Str(s),
                        line: sl,
                        col: sc,
                    });
                    break;
                }
                Some('\n') | Some('\r') => {
                    self.err(
                        sl,
                        sc,
                        "লেখা লিটারেলে সরাসরি নিউলাইন আসতে পারে না — '\\n' ব্যবহার করুন"
                            .to_string(),
                    );
                    break;
                }
                Some('\\') => {
                    if let Some(ch) = self.escape_char() {
                        s.push(ch);
                    }
                }
                Some(x) => {
                    s.push(x);
                    self.i += 1;
                    self.col += 1;
                }
            }
        }
    }

    fn char_lit(&mut self) {
        let (sl, sc) = (self.line, self.col);
        self.i += 1;
        self.col += 1;
        let ch: Option<char> = match self.peek() {
            None => {
                self.err(sl, sc, "ক্যারেক্টার লিটারেল বন্ধ হয়নি".to_string());
                return;
            }
            Some('\n') | Some('\r') => {
                self.err(
                    sl,
                    sc,
                    "ক্যারেক্টার লিটারেলে নিউলাইন আসতে পারে না".to_string(),
                );
                None
            }
            Some('\\') => self.escape_char(),
            Some(x) => {
                self.i += 1;
                self.col += 1;
                Some(x)
            }
        };
        if ch.is_none() {
            while let Some(c) = self.peek() {
                if c == '\'' || c == '\n' || c == '\r' {
                    break;
                }
                self.i += 1;
                self.col += 1;
            }
            if self.peek() == Some('\'') {
                self.i += 1;
                self.col += 1;
            }
            return;
        }
        if self.peek() == Some('\'') {
            self.i += 1;
            self.col += 1;
            self.toks.push(Token {
                kind: TokenKind::Chr(ch.unwrap()),
                line: sl,
                col: sc,
            });
        } else {
            while let Some(c) = self.peek() {
                if c == '\'' || c == '\n' || c == '\r' {
                    break;
                }
                self.i += 1;
                self.col += 1;
            }
            if self.peek() == Some('\'') {
                self.i += 1;
                self.col += 1;
                self.err(
                    sl,
                    sc,
                    "ক্যারেক্টার লিটারেলে ঠিক একটি ক্যারেক্টার থাকতে হবে".to_string(),
                );
            } else {
                self.err(sl, sc, "ক্যারেক্টার লিটারেল বন্ধ হয়নি — \"'\" পাওয়া যায়নি".to_string());
            }
        }
    }

    fn number(&mut self, first: char) {
        let (sl, sc) = (self.line, self.col);
        let bangla = is_bangla_digit(first);
        let mut text = String::new();
        let mut mixed = false;
        let mut float = false;
        while let Some(c) = self.peek() {
            if !is_any_digit(c) {
                break;
            }
            if is_bangla_digit(c) != bangla {
                mixed = true;
            }
            text.push(c);
            self.i += 1;
            self.col += 1;
        }
        if self.peek() == Some('.') && self.peek2().map(is_any_digit).unwrap_or(false) {
            float = true;
            text.push('.');
            self.i += 1;
            self.col += 1;
            while let Some(c) = self.peek() {
                if !is_any_digit(c) {
                    break;
                }
                if is_bangla_digit(c) != bangla {
                    mixed = true;
                }
                text.push(c);
                self.i += 1;
                self.col += 1;
            }
        }
        if mixed {
            self.err(
                sl,
                sc,
                "একটি সংখ্যা লিটারেলে বাংলা ও ইংরেজি অঙ্ক মেশানো যাবে না".to_string(),
            );
        } else if float {
            let ascii: String = text.chars().map(digit_to_ascii).collect();
            let v: f64 = ascii.parse().unwrap_or(0.0);
            self.toks.push(Token {
                kind: TokenKind::Num(NumTok::Float(v)),
                line: sl,
                col: sc,
            });
        } else {
            let mut acc: i64 = 0;
            let mut overflow = false;
            for ch in text.chars() {
                acc = match acc.checked_mul(10).and_then(|x| x.checked_add(digit_value(ch))) {
                    Some(v) => v,
                    None => {
                        overflow = true;
                        break;
                    }
                };
            }
            if overflow {
                self.err(sl, sc, "সংখ্যাটি 'সংখ্যা' টাইপের সীমার বাইরে".to_string());
            } else {
                self.toks.push(Token {
                    kind: TokenKind::Num(NumTok::Int(acc)),
                    line: sl,
                    col: sc,
                });
            }
        }
    }

    fn ident(&mut self) {
        let (sl, sc) = (self.line, self.col);
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if is_ident_part(c) {
                s.push(c);
                self.i += 1;
                self.col += 1;
            } else {
                break;
            }
        }
        let kind = match KEYWORDS.iter().find(|k| **k == s.as_str()) {
            Some(kw) => TokenKind::Kw(kw),
            None => TokenKind::Ident(s),
        };
        self.toks.push(Token {
            kind,
            line: sl,
            col: sc,
        });
    }

    fn operator(&mut self, c: char) {
        let two: Option<&'static str> = match (c, self.peek2()) {
            ('=', Some('=')) => Some("=="),
            ('!', Some('=')) => Some("!="),
            ('<', Some('=')) => Some("<="),
            ('>', Some('=')) => Some(">="),
            ('-', Some('>')) => Some("->"),
            ('+', Some('=')) => Some("+="),
            ('-', Some('=')) => Some("-="),
            ('*', Some('=')) => Some("*="),
            ('/', Some('=')) => Some("/="),
            _ => None,
        };
        if let Some(o) = two {
            self.toks.push(Token {
                kind: TokenKind::Op(o),
                line: self.line,
                col: self.col,
            });
            self.i += 2;
            self.col += 2;
            return;
        }
        if c == '!' {
            self.err(
                self.line,
                self.col,
                "অপ্রত্যাশিত ক্যারেক্টর '!' — অসমতা চিহ্ন '!='".to_string(),
            );
            self.i += 1;
            self.col += 1;
            return;
        }
        match one_char_op(c) {
            Some(o) => {
                let (l, co) = (self.line, self.col);
                match o {
                    "(" | "[" => self.opens.push((o, l, co)),
                    ")" | "]" => {
                        let want = if o == ")" { "(" } else { "[" };
                        match self.opens.pop() {
                            Some((open, _, _)) if open == want => {}
                            _ => {
                                self.err(l, co, format!("অমিল বন্ধকরণ '{}'", o));
                            }
                        }
                    }
                    _ => {}
                }
                self.toks.push(Token {
                    kind: TokenKind::Op(o),
                    line: l,
                    col: co,
                });
                self.i += 1;
                self.col += 1;
            }
            None => {
                self.err(self.line, self.col, format!("অপ্রত্যাশিত ক্যারেক্টর '{}'", c));
                self.i += 1;
                self.col += 1;
            }
        }
    }
}

pub fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lx {
        cs: src.chars().collect(),
        i: 0,
        line: 1,
        col: 1,
        toks: Vec::new(),
        diags: Vec::new(),
        opens: Vec::new(),
    };
    lx.run();
    for (o, l, c) in lx.opens.drain(..) {
        lx.diags
            .push(Diagnostic { line: l, col: c, message: format!("'{}' খোলা অবস্থায় ফাইল শেষ — বন্ধ হয়নি", o) });
    }
    (lx.toks, lx.diags)
}
