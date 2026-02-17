use winnow::combinator::{alt, opt};
use winnow::error::ContextError;
use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Location};
use winnow::token::{any, take_till, take_while};

use crate::error::CompileError;
use crate::scanner::token::{Span, Token, TokenKind, keyword_kind};

type Input<'a> = LocatingSlice<&'a str>;

fn shebang<'a>(input: &mut Input<'a>) -> ModalResult<()> {
    ("#!", take_till(0.., '\n'), opt('\n'))
        .void()
        .parse_next(input)
}

fn whitespace_and_comments<'a>(input: &mut Input<'a>) -> ModalResult<()> {
    loop {
        let before = input.current_token_start();
        take_while(0.., |c: char| {
            c == ' ' || c == '\t' || c == '\r' || c == '\n'
        })
        .void()
        .parse_next(input)?;

        if input.starts_with("//") {
            take_while(0.., |c: char| c != '\n')
                .void()
                .parse_next(input)?;
        } else if input.current_token_start() == before {
            break;
        }
    }
    Ok(())
}

fn string_literal<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    let start = input.current_token_start();
    '"'.parse_next(input)?;
    let mut s = String::new();
    loop {
        let c = any
            .parse_next(input)
            .map_err(|_: winnow::error::ErrMode<ContextError>| {
                winnow::error::ErrMode::Cut(ContextError::new())
            })?;
        match c {
            '"' => break,
            '\\' => {
                let esc =
                    any.parse_next(input)
                        .map_err(|_: winnow::error::ErrMode<ContextError>| {
                            winnow::error::ErrMode::Cut(ContextError::new())
                        })?;
                match esc {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    '\\' => s.push('\\'),
                    '"' => s.push('"'),
                    other => {
                        s.push('\\');
                        s.push(other);
                    }
                }
            }
            other => s.push(other),
        }
    }
    let end = input.current_token_start();
    let span = Span::new(start, end - start);
    Ok(Token::new(TokenKind::String, s, span))
}

fn number_literal<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    let start = input.current_token_start();
    let whole: &str = take_while(1.., |c: char| c.is_ascii_digit()).parse_next(input)?;
    let mut lexeme = whole.to_string();

    let checkpoint = input.checkpoint();
    let dot_result: Result<char, winnow::error::ErrMode<ContextError>> = '.'.parse_next(input);
    if dot_result.is_ok() {
        match take_while::<_, _, ContextError>(1.., |c: char| c.is_ascii_digit()).parse_next(input)
        {
            Ok(frac) => {
                lexeme.push('.');
                lexeme.push_str(frac);
            }
            Err(_) => {
                input.reset(&checkpoint);
            }
        }
    }

    let end = input.current_token_start();
    Ok(Token::new(
        TokenKind::Number,
        lexeme,
        Span::new(start, end - start),
    ))
}

fn identifier_or_keyword<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    let start = input.current_token_start();
    let first: char = any
        .verify(|c: &char| c.is_ascii_alphabetic() || *c == '_')
        .parse_next(input)?;
    let rest: &str =
        take_while(0.., |c: char| c.is_ascii_alphanumeric() || c == '_').parse_next(input)?;
    let end = input.current_token_start();
    let mut lexeme = String::with_capacity(1 + rest.len());
    lexeme.push(first);
    lexeme.push_str(rest);
    let kind = keyword_kind(&lexeme).unwrap_or(TokenKind::Identifier);
    Ok(Token::new(kind, lexeme, Span::new(start, end - start)))
}

fn two_char_token<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    let start = input.current_token_start();
    let (kind, lexeme) = alt((
        "!=".value((TokenKind::BangEqual, "!=")),
        "==".value((TokenKind::EqualEqual, "==")),
        ">=".value((TokenKind::GreaterEqual, ">=")),
        "<=".value((TokenKind::LessEqual, "<=")),
    ))
    .parse_next(input)?;
    Ok(Token::new(kind, lexeme, Span::new(start, 2)))
}

fn single_char_token<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    let start = input.current_token_start();
    let c = any
        .verify(|c: &char| "(){}.,;-+/*!=<>".contains(*c))
        .parse_next(input)?;
    let kind = match c {
        '(' => TokenKind::LeftParen,
        ')' => TokenKind::RightParen,
        '{' => TokenKind::LeftBrace,
        '}' => TokenKind::RightBrace,
        ',' => TokenKind::Comma,
        '.' => TokenKind::Dot,
        '-' => TokenKind::Minus,
        '+' => TokenKind::Plus,
        ';' => TokenKind::Semicolon,
        '/' => TokenKind::Slash,
        '*' => TokenKind::Star,
        '!' => TokenKind::Bang,
        '=' => TokenKind::Equal,
        '<' => TokenKind::Less,
        '>' => TokenKind::Greater,
        _ => unreachable!("verify guarantees valid char"),
    };
    Ok(Token::new(kind, c.to_string(), Span::new(start, 1)))
}

fn scan_token<'a>(input: &mut Input<'a>) -> ModalResult<Token> {
    alt((
        string_literal,
        number_literal,
        identifier_or_keyword,
        two_char_token,
        single_char_token,
    ))
    .parse_next(input)
}

/// Scan all tokens from source, returning either a token list or scan errors.
pub fn scan_all(source: &str) -> Result<Vec<Token>, Vec<CompileError>> {
    let mut input = LocatingSlice::new(source);
    let _ = opt(shebang).parse_next(&mut input);
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    loop {
        if whitespace_and_comments(&mut input).is_err() {
            break;
        }
        if input.is_empty() {
            break;
        }
        match scan_token(&mut input) {
            Ok(token) => tokens.push(token),
            Err(_) => {
                let offset = input.current_token_start();
                let c = any::<_, ContextError>.parse_next(&mut input).ok();
                let ch = c.unwrap_or('?');
                errors.push(CompileError::scan(
                    format!("unexpected character '{ch}'"),
                    offset,
                    1,
                ));
            }
        }
    }

    let eof_offset = source.len();
    tokens.push(Token::new(TokenKind::Eof, "", Span::new(eof_offset, 0)));

    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_ok(source: &str) -> Vec<Token> {
        scan_all(source).expect("scan should succeed")
    }

    fn kinds(tokens: &[Token]) -> Vec<TokenKind> {
        tokens.iter().map(|t| t.kind).collect()
    }

    #[test]
    fn single_char_tokens() {
        let tokens = scan_ok("(){},.-+;/*");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::Comma,
                TokenKind::Dot,
                TokenKind::Minus,
                TokenKind::Plus,
                TokenKind::Semicolon,
                TokenKind::Slash,
                TokenKind::Star,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn two_char_tokens() {
        let tokens = scan_ok("!= == >= <=");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::BangEqual,
                TokenKind::EqualEqual,
                TokenKind::GreaterEqual,
                TokenKind::LessEqual,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn single_then_equal() {
        let tokens = scan_ok("! = < >");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Bang,
                TokenKind::Equal,
                TokenKind::Less,
                TokenKind::Greater,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_literal_test() {
        let tokens = scan_ok("\"hello world\"");
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, "hello world");
    }

    #[test]
    fn string_with_escapes() {
        let tokens = scan_ok("\"hello\\nworld\\t!\"");
        assert_eq!(tokens[0].lexeme, "hello\nworld\t!");
    }

    #[test]
    fn number_integer() {
        let tokens = scan_ok("42");
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].lexeme, "42");
    }

    #[test]
    fn number_decimal() {
        let tokens = scan_ok("3.14");
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].lexeme, "3.14");
    }

    #[test]
    fn number_no_trailing_dot() {
        let tokens = scan_ok("42.foo");
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].lexeme, "42");
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Identifier);
    }

    #[test]
    fn identifiers_and_keywords() {
        let tokens = scan_ok("var x = true");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Var,
                TokenKind::Identifier,
                TokenKind::Equal,
                TokenKind::True,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn all_keywords() {
        let source =
            "and class else false fun for if nil or print return super this true var while";
        let tokens = scan_ok(source);
        let expected = vec![
            TokenKind::And,
            TokenKind::Class,
            TokenKind::Else,
            TokenKind::False,
            TokenKind::Fun,
            TokenKind::For,
            TokenKind::If,
            TokenKind::Nil,
            TokenKind::Or,
            TokenKind::Print,
            TokenKind::Return,
            TokenKind::Super,
            TokenKind::This,
            TokenKind::True,
            TokenKind::Var,
            TokenKind::While,
            TokenKind::Eof,
        ];
        assert_eq!(kinds(&tokens), expected);
    }

    #[test]
    fn comments_ignored() {
        let tokens = scan_ok("var x // this is a comment\nvar y");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Var,
                TokenKind::Identifier,
                TokenKind::Var,
                TokenKind::Identifier,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn spans_are_correct() {
        let tokens = scan_ok("var x = 42;");
        assert_eq!(tokens[0].span, Span::new(0, 3)); // var
        assert_eq!(tokens[1].span, Span::new(4, 1)); // x
        assert_eq!(tokens[2].span, Span::new(6, 1)); // =
        assert_eq!(tokens[3].span, Span::new(8, 2)); // 42
        assert_eq!(tokens[4].span, Span::new(10, 1)); // ;
    }

    #[test]
    fn unexpected_character_error() {
        let result = scan_all("var x = @;");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].to_string().contains('@'));
    }

    #[test]
    fn unterminated_string_error() {
        let result = scan_all("\"unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn multiline_program() {
        let source = "var x = 1;\nvar y = 2;\nprint x + y;";
        let tokens = scan_ok(source);
        assert_eq!(tokens.len(), 16); // 15 tokens + EOF
    }

    use rstest::rstest;

    #[rstest]
    #[case("shebang only", "#!/usr/bin/env lox", &[TokenKind::Eof])]
    #[case(
        "shebang with newline and code",
        "#!/usr/bin/env lox\nprint 1;",
        &[TokenKind::Print, TokenKind::Number, TokenKind::Semicolon, TokenKind::Eof]
    )]
    #[case(
        "no shebang unaffected",
        "print 1;",
        &[TokenKind::Print, TokenKind::Number, TokenKind::Semicolon, TokenKind::Eof]
    )]
    #[case("shebang without trailing newline", "#!/usr/bin/env lox", &[TokenKind::Eof])]
    fn shebang_cases(#[case] _label: &str, #[case] source: &str, #[case] expected: &[TokenKind]) {
        let tokens = scan_ok(source);
        assert_eq!(kinds(&tokens), expected);
    }

    #[test]
    fn shebang_code_spans_are_after_shebang_line() {
        // `print` begins at byte 20, after "#!/usr/bin/env lox\n" (19 chars + newline = 20)
        let source = "#!/usr/bin/env lox\nprint 1;";
        let tokens = scan_ok(source);
        let print_span = tokens[0].span;
        assert_eq!(
            print_span.offset, 19,
            "print token should start after shebang line"
        );
    }
}
