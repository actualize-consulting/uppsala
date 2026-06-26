//! Zero-copy XPath 2.0 lexer.

use std::borrow::Cow;
use std::ops::Range;

use crate::error::{XmlError, XmlResult};

/// XPath 2.0 token with a source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token<'expr> {
    /// Token kind.
    pub kind: TokenKind<'expr>,
    /// Byte range in the original expression.
    pub span: Range<usize>,
}

/// XPath 2.0 token kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind<'expr> {
    Name(&'expr str),
    StringLiteral(Cow<'expr, str>),
    IntegerLiteral(&'expr str),
    DecimalLiteral(&'expr str),
    DoubleLiteral(&'expr str),
    Slash,
    DoubleSlash,
    Dot,
    DoubleDot,
    At,
    Star,
    Pipe,
    Plus,
    Minus,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    NodeBefore,
    NodeAfter,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dollar,
    Colon,
    ColonColon,
}

/// Tokenize an XPath 2.0 expression.
pub fn tokenize(input: &str) -> XmlResult<Vec<Token<'_>>> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut offset = 0;

    while offset < bytes.len() {
        offset = skip_trivia(input, offset)?;
        if offset >= bytes.len() {
            break;
        }

        let start = offset;
        let b = bytes[offset];
        match b {
            b'/' => {
                if bytes.get(offset + 1) == Some(&b'/') {
                    offset += 2;
                    push(&mut tokens, TokenKind::DoubleSlash, start, offset);
                } else {
                    offset += 1;
                    push(&mut tokens, TokenKind::Slash, start, offset);
                }
            }
            b'.' => {
                if bytes.get(offset + 1) == Some(&b'.') {
                    offset += 2;
                    push(&mut tokens, TokenKind::DoubleDot, start, offset);
                } else if bytes
                    .get(offset + 1)
                    .is_some_and(|next| next.is_ascii_digit())
                {
                    offset = scan_number(input, offset, &mut tokens)?;
                } else {
                    offset += 1;
                    push(&mut tokens, TokenKind::Dot, start, offset);
                }
            }
            b'@' => {
                offset += 1;
                push(&mut tokens, TokenKind::At, start, offset);
            }
            b'*' => {
                offset += 1;
                push(&mut tokens, TokenKind::Star, start, offset);
            }
            b'|' => {
                offset += 1;
                push(&mut tokens, TokenKind::Pipe, start, offset);
            }
            b'+' => {
                offset += 1;
                push(&mut tokens, TokenKind::Plus, start, offset);
            }
            b'-' => {
                offset += 1;
                push(&mut tokens, TokenKind::Minus, start, offset);
            }
            b'=' => {
                offset += 1;
                push(&mut tokens, TokenKind::Equal, start, offset);
            }
            b'!' => {
                if bytes.get(offset + 1) == Some(&b'=') {
                    offset += 2;
                    push(&mut tokens, TokenKind::NotEqual, start, offset);
                } else {
                    return Err(XmlError::xpath("unexpected '!' in XPath expression"));
                }
            }
            b'<' => {
                if bytes.get(offset + 1) == Some(&b'=') {
                    offset += 2;
                    push(&mut tokens, TokenKind::LessThanOrEqual, start, offset);
                } else if bytes.get(offset + 1) == Some(&b'<') {
                    offset += 2;
                    push(&mut tokens, TokenKind::NodeBefore, start, offset);
                } else {
                    offset += 1;
                    push(&mut tokens, TokenKind::LessThan, start, offset);
                }
            }
            b'>' => {
                if bytes.get(offset + 1) == Some(&b'=') {
                    offset += 2;
                    push(&mut tokens, TokenKind::GreaterThanOrEqual, start, offset);
                } else if bytes.get(offset + 1) == Some(&b'>') {
                    offset += 2;
                    push(&mut tokens, TokenKind::NodeAfter, start, offset);
                } else {
                    offset += 1;
                    push(&mut tokens, TokenKind::GreaterThan, start, offset);
                }
            }
            b'(' => {
                offset += 1;
                push(&mut tokens, TokenKind::LeftParen, start, offset);
            }
            b')' => {
                offset += 1;
                push(&mut tokens, TokenKind::RightParen, start, offset);
            }
            b'[' => {
                offset += 1;
                push(&mut tokens, TokenKind::LeftBracket, start, offset);
            }
            b']' => {
                offset += 1;
                push(&mut tokens, TokenKind::RightBracket, start, offset);
            }
            b',' => {
                offset += 1;
                push(&mut tokens, TokenKind::Comma, start, offset);
            }
            b'$' => {
                offset += 1;
                push(&mut tokens, TokenKind::Dollar, start, offset);
            }
            b':' => {
                if bytes.get(offset + 1) == Some(&b':') {
                    offset += 2;
                    push(&mut tokens, TokenKind::ColonColon, start, offset);
                } else {
                    offset += 1;
                    push(&mut tokens, TokenKind::Colon, start, offset);
                }
            }
            b'\'' | b'"' => {
                offset = scan_string(input, offset, &mut tokens)?;
            }
            b if b.is_ascii_digit() => {
                offset = scan_number(input, offset, &mut tokens)?;
            }
            _ if is_name_start(input, offset) => {
                offset = scan_name(input, offset, &mut tokens);
            }
            _ => {
                return Err(XmlError::xpath(format!(
                    "unexpected character '{}' in XPath expression",
                    input[offset..].chars().next().unwrap_or('\0')
                )));
            }
        }
    }

    Ok(tokens)
}

fn push<'expr>(tokens: &mut Vec<Token<'expr>>, kind: TokenKind<'expr>, start: usize, end: usize) {
    tokens.push(Token {
        kind,
        span: start..end,
    });
}

fn skip_trivia(input: &str, mut offset: usize) -> XmlResult<usize> {
    loop {
        offset = skip_ascii_whitespace(input.as_bytes(), offset);
        if input.as_bytes().get(offset..offset + 2) == Some(b"(:") {
            offset = skip_comment(input, offset)?;
            continue;
        }
        return Ok(offset);
    }
}

fn skip_comment(input: &str, mut offset: usize) -> XmlResult<usize> {
    debug_assert_eq!(input.as_bytes().get(offset..offset + 2), Some(&b"(:"[..]));
    let bytes = input.as_bytes();
    let mut depth = 1usize;
    offset += 2;

    while offset + 1 < bytes.len() {
        match &bytes[offset..offset + 2] {
            b"(:" => {
                depth += 1;
                offset += 2;
            }
            b":)" => {
                depth -= 1;
                offset += 2;
                if depth == 0 {
                    return Ok(offset);
                }
            }
            _ => {
                offset += 1;
            }
        }
    }

    Err(XmlError::xpath("unterminated XPath comment"))
}

fn scan_string<'expr>(
    input: &'expr str,
    start: usize,
    tokens: &mut Vec<Token<'expr>>,
) -> XmlResult<usize> {
    let bytes = input.as_bytes();
    let quote = bytes[start];
    let mut offset = start + 1;
    let value_start = offset;
    let mut owned = None::<String>;

    while offset < bytes.len() {
        if bytes[offset] == quote {
            if bytes.get(offset + 1) == Some(&quote) {
                let buf = owned.get_or_insert_with(|| input[value_start..offset].to_string());
                buf.push(quote as char);
                offset += 2;
                let segment_start = offset;
                while offset < bytes.len() && bytes[offset] != quote {
                    offset += 1;
                }
                if let Some(buf) = owned.as_mut() {
                    buf.push_str(&input[segment_start..offset]);
                }
                continue;
            }

            let value = match owned {
                Some(value) => Cow::Owned(value),
                None => Cow::Borrowed(&input[value_start..offset]),
            };
            let end = offset + 1;
            push(tokens, TokenKind::StringLiteral(value), start, end);
            return Ok(end);
        }
        offset += 1;
    }

    Err(XmlError::xpath("unterminated string literal"))
}

fn scan_number<'expr>(
    input: &'expr str,
    start: usize,
    tokens: &mut Vec<Token<'expr>>,
) -> XmlResult<usize> {
    let bytes = input.as_bytes();
    let mut offset = start;

    while bytes
        .get(offset)
        .is_some_and(|current| current.is_ascii_digit())
    {
        offset += 1;
    }

    let mut has_dot = false;
    if bytes.get(offset) == Some(&b'.') && bytes.get(offset + 1) != Some(&b'.') {
        has_dot = true;
        offset += 1;
        while bytes
            .get(offset)
            .is_some_and(|current| current.is_ascii_digit())
        {
            offset += 1;
        }
    }

    let mut has_exponent = false;
    if matches!(bytes.get(offset), Some(b'e' | b'E')) {
        let exp = offset;
        let mut cursor = exp + 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let digits_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|current| current.is_ascii_digit())
        {
            cursor += 1;
        }
        if cursor > digits_start {
            has_exponent = true;
            offset = cursor;
        }
    }

    if offset == start || &input[start..offset] == "." {
        return Err(XmlError::xpath("invalid numeric literal"));
    }

    let lexeme = &input[start..offset];
    let kind = if has_exponent {
        TokenKind::DoubleLiteral(lexeme)
    } else if has_dot {
        TokenKind::DecimalLiteral(lexeme)
    } else {
        TokenKind::IntegerLiteral(lexeme)
    };
    push(tokens, kind, start, offset);
    Ok(offset)
}

fn scan_name<'expr>(input: &'expr str, start: usize, tokens: &mut Vec<Token<'expr>>) -> usize {
    let mut offset = start;
    while offset < input.len() && is_name_char(input, offset) {
        let ch = input[offset..]
            .chars()
            .next()
            .expect("valid name character");
        offset += ch.len_utf8();
    }
    push(
        tokens,
        TokenKind::Name(&input[start..offset]),
        start,
        offset,
    );
    offset
}

fn is_name_start(input: &str, offset: usize) -> bool {
    let Some(ch) = input[offset..].chars().next() else {
        return false;
    };
    ch == '_' || ch.is_ascii_alphabetic() || (ch as u32) >= 0x80
}

fn is_name_char(input: &str, offset: usize) -> bool {
    let Some(ch) = input[offset..].chars().next() else {
        return false;
    };
    ch == '_' || ch == '-' || ch == '.' || ch.is_ascii_alphanumeric() || (ch as u32) >= 0x80
}

fn skip_ascii_whitespace(bytes: &[u8], offset: usize) -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is guaranteed on x86_64 targets.
        return unsafe { skip_ascii_whitespace_sse2(bytes, offset) };
    }

    #[cfg(target_arch = "x86")]
    {
        if std::is_x86_feature_detected!("sse2") {
            // SAFETY: Runtime feature detection confirms SSE2 support.
            return unsafe { skip_ascii_whitespace_sse2(bytes, offset) };
        }
    }

    #[allow(unreachable_code)]
    skip_ascii_whitespace_scalar(bytes, offset)
}

fn skip_ascii_whitespace_scalar(bytes: &[u8], mut offset: usize) -> usize {
    while bytes.get(offset).is_some_and(|b| is_ascii_whitespace(*b)) {
        offset += 1;
    }
    offset
}

fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn skip_ascii_whitespace_sse2(bytes: &[u8], mut offset: usize) -> usize {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{
        __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{
        __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8,
    };

    let spaces = _mm_set1_epi8(b' ' as i8);
    let tabs = _mm_set1_epi8(b'\t' as i8);
    let carriage_returns = _mm_set1_epi8(b'\r' as i8);
    let newlines = _mm_set1_epi8(b'\n' as i8);

    while offset + 16 <= bytes.len() {
        let chunk = _mm_loadu_si128(bytes.as_ptr().add(offset) as *const __m128i);
        let space_mask = _mm_cmpeq_epi8(chunk, spaces);
        let tab_mask = _mm_cmpeq_epi8(chunk, tabs);
        let cr_mask = _mm_cmpeq_epi8(chunk, carriage_returns);
        let nl_mask = _mm_cmpeq_epi8(chunk, newlines);
        let ws_mask = _mm_or_si128(
            _mm_or_si128(space_mask, tab_mask),
            _mm_or_si128(cr_mask, nl_mask),
        );
        let mask = _mm_movemask_epi8(ws_mask) as u32;

        if mask == 0xffff {
            offset += 16;
            continue;
        }

        return offset + ((!mask) & 0xffff).trailing_zeros() as usize;
    }

    skip_ascii_whitespace_scalar(bytes, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literals_borrow_without_escapes() {
        let tokens = tokenize("'abc'").unwrap();
        match &tokens[0].kind {
            TokenKind::StringLiteral(Cow::Borrowed(value)) => assert_eq!(*value, "abc"),
            other => panic!("expected borrowed string literal, got {other:?}"),
        }
    }

    #[test]
    fn string_literals_allocate_for_doubled_quotes() {
        let tokens = tokenize("'can''t'").unwrap();
        match &tokens[0].kind {
            TokenKind::StringLiteral(Cow::Owned(value)) => assert_eq!(value, "can't"),
            other => panic!("expected owned string literal, got {other:?}"),
        }
    }

    #[test]
    fn nested_comments_are_trivia() {
        let tokens = tokenize("1 (: outer (: inner :) done :) + 2").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::IntegerLiteral("1")));
        assert!(matches!(tokens[1].kind, TokenKind::Plus));
        assert!(matches!(tokens[2].kind, TokenKind::IntegerLiteral("2")));
    }

    #[test]
    fn simd_and_scalar_whitespace_scans_match() {
        let input = b" \n\t\r \t abc";
        assert_eq!(
            skip_ascii_whitespace(input, 0),
            skip_ascii_whitespace_scalar(input, 0)
        );
    }
}
