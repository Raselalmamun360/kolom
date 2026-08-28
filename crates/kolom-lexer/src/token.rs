#[derive(Debug, Clone, PartialEq)]
pub enum NumTok {
    Int(i64),
    Float(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Kw(&'static str),
    Num(NumTok),
    Str(String),
    Chr(char),
    Newline,
    Op(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
}
