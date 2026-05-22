// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::io::Write;

use crate::{Identifier, Token};
use bumpalo::{
    Bump,
    collections::{CollectIn, Vec},
};

fn lex<'a>(
    src: &'a [u8],
    arena: &'a Bump,
    posix_strict: bool,
    gnu_strict: bool,
) -> Vec<'a, Token<'a>> {
    Token::lex(src, arena, posix_strict, gnu_strict)
        .collect_in::<Result<Vec<_>, _>>(arena)
        .unwrap()
}

#[test]
fn lexer_test_newlines_non_posix() {
    let mixed = " \t \n \t\n\n \\\n \t";
    let arena = Bump::new();
    let mut str = Vec::new_in(&arena);
    for tok in ["BEGIN", "{", "else", "do", "&&", "||", "?", ":", ","] {
        write!(str, "{tok}{mixed}").unwrap();
    }
    str.push(b'}');
    assert_eq!(
        &lex(&str, &arena, false, false),
        &[
            Token::BeginPattern,
            Token::Newline,
            Token::Newline,
            Token::OpenBrace,
            Token::Else,
            Token::Do,
            Token::BooleanAnd,
            Token::BooleanOr,
            Token::QuestionMark,
            Token::Colon,
            Token::Comma,
            Token::ClosedBrace
        ]
    );
}

#[test]
#[should_panic]
fn lexer_test_newlines_posix() {
    let mixed = " \t \n \t\n\n \\\n \t";
    let arena = Bump::new();
    let mut str = Vec::new_in(&arena);
    for tok in ["BEGIN", "{", "else", "do", "&&", "||", "?", ":", ","] {
        write!(str, "{tok}{mixed}").unwrap();
    }
    str.push(b'}');
    lex(&str, &arena, true, false);
}

#[test]
fn lexer_test_collapsible_delimiters() {
    let arena = Bump::new();
    let str = b";\\\n;\n\n\n\n;;\n\\\n\n";
    assert_eq!(
        &lex(str, &arena, false, false),
        &[
            Token::Semicolon,
            Token::Semicolon,
            Token::Newline,
            Token::Semicolon,
            Token::Semicolon,
            Token::Newline,
            Token::Newline,
        ]
    );
}

#[test]
fn lexer_test_multiline() {
    let arena = Bump::new();
    let str = b"\"aaaa\\\nbbbb\", /ccc\\\nd/";
    assert_eq!(
        &lex(str, &arena, false, false),
        &[
            Token::String(b"aaaabbbb".into()),
            Token::Comma,
            Token::Regex(b"cccd".into())
        ]
    );
}

#[test]
fn lexer_test_uu_extensions() {
    let arena = Bump::new();
    assert_eq!(
        lex(b"@concurrent", &arena, false, true),
        &[Token::IndirectCall(Identifier {
            namespace: None,
            literal: "concurrent"
        })]
    );
}

#[test]
fn lexer_test_gnu_pattern() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"BEGINFILE ENDFILE", &arena, true, false),
        &[
            Token::Identifier(Identifier {
                namespace: None,
                literal: "BEGINFILE"
            }),
            Token::Identifier(Identifier {
                namespace: None,
                literal: "ENDFILE"
            })
        ]
    );
}

#[test]
fn lexer_test_floats() {
    let arena = Bump::new();
    let str = b"1 20. 0. .3 2e4 -3.e2 5e+1 2.1e-3";
    assert_eq!(
        &lex(str, &arena, false, false),
        &[
            Token::Number(1.),
            Token::Number(20.),
            Token::Number(0.),
            Token::Number(0.3),
            Token::Number(2e4),
            Token::Number(-3e2),
            Token::Number(5e1),
            Token::Number(2.1e-3)
        ]
    );
}

#[test]
fn lexer_test_directive_escaping() {
    let arena = Bump::new();
    let str = br#" @include "aa\"a\ta" @nsinclude "b\"\nb" "#;
    assert_eq!(
        &lex(str, &arena, false, false),
        &[
            Token::IncludeDirective,
            Token::String(b"aa\"a\ta".into()),
            Token::NsIncludeDirective,
            Token::String(b"b\"\nb".into())
        ]
    );
}

#[test]
fn lexer_test_ident_rules_non_posix() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"1a::a a::1a _a", &arena, false, false),
        &[
            Token::Number(1.),
            Token::Identifier(Identifier {
                namespace: Some("a"),
                literal: "a"
            }),
            Token::Identifier(Identifier {
                namespace: None,
                literal: "a"
            }),
            Token::Colon,
            Token::Colon,
            Token::Number(1.),
            Token::Identifier(Identifier {
                namespace: None,
                literal: "a"
            }),
            Token::Identifier(Identifier {
                namespace: None,
                literal: "_a"
            })
        ]
    );
}

#[test]
#[should_panic]
fn lexer_test_ident_rules_posix() {
    let arena = Bump::new();
    lex(b"@namespace \"foo\"; foo::a", &arena, true, false);
}

#[test]
fn lexer_test_general_tokens() {
    let arena = Bump::new();
    let str = br#"
        @load "lib1.so.1"
        BEGIN { print a + 1 }
        /2\..*/;
        END { $1 == foo::bar }
    "#;
    assert_eq!(
        &lex(str, &arena, false, false),
        &[
            Token::Newline,
            Token::LoadDirective,
            Token::String(b"lib1.so.1".into()),
            Token::Newline,
            Token::BeginPattern,
            Token::OpenBrace,
            Token::Print,
            Token::Identifier(Identifier {
                namespace: None,
                literal: "a"
            }),
            Token::Plus,
            Token::Number(1.),
            Token::ClosedBrace,
            Token::Newline,
            Token::Regex(b"2\\..*".into()),
            Token::Semicolon,
            Token::Newline,
            Token::EndPattern,
            Token::OpenBrace,
            Token::Record,
            Token::Number(1.),
            Token::EqualTo,
            Token::Identifier(Identifier {
                namespace: Some("foo"),
                literal: "bar"
            }),
            Token::ClosedBrace,
            Token::Newline
        ]
    );
}

#[test]
fn lexer_test_regex_ambiguity() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"1/=1 a/=1", &arena, false, false),
        &[
            Token::Number(1.),
            Token::SlashAssign,
            Token::Number(1.),
            Token::Identifier(Identifier {
                namespace: None,
                literal: "a"
            }),
            Token::SlashAssign,
            Token::Number(1.)
        ]
    );
}

#[test]
fn lexer_test_hex_escape() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\x41\"", &arena, false, false),
        &[Token::String(b"A".into())]
    );
}

#[test]
fn lexer_test_hex_escape_uppercase() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\x4F\"", &arena, false, false),
        &[Token::String(b"O".into())]
    );
}

#[test]
fn lexer_test_hex_escape_single_digit() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\x9\"", &arena, false, false),
        &[Token::String(b"\x09".into())]
    );
}

#[test]
fn lexer_test_hex_escape_posix_strict() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\x41\"", &arena, true, false),
        &[Token::String(b"x41".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_posix_strict() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u0041\"", &arena, true, false),
        &[Token::String(b"u0041".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_ascii() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u0041\"", &arena, false, false),
        &[Token::String(b"A".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_two_byte() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u00e9\"", &arena, false, false),
        &[Token::String("\u{00e9}".as_bytes().into())]
    );
}

#[test]
fn lexer_test_unicode_escape_three_byte() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u4e2d\"", &arena, false, false),
        &[Token::String("\u{4e2d}".as_bytes().into())]
    );
}

#[test]
fn lexer_test_unicode_escape_uppercase() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u004F\"", &arena, false, false),
        &[Token::String(b"O".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_single_digit() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u9\"", &arena, false, false),
        &[Token::String("\u{9}".as_bytes().into())]
    );
}

#[test]
fn lexer_test_hex_escape_no_digits() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\x\"", &arena, false, false),
        &[Token::String(b"x".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_no_digits() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u\"", &arena, false, false),
        &[Token::String(b"u".into())]
    );
}

#[test]
fn lexer_test_unicode_escape_eight_digits() {
    let arena = Bump::new();
    assert_eq!(
        &lex(b"\"\\u00000032\"", &arena, false, false),
        &[Token::String(b"2".into())]
    );
}
