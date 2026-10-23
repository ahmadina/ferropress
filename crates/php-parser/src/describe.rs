//! How syntax errors name the unexpected token and the tokens that would have been accepted.

use php_lexer::TokenKind;

/// The end of the input, as a token kind no scanned token has.
pub(crate) const END: TokenKind = TokenKind::Char(0);

/// What a syntax error calls a token kind.
enum Label {
    /// A kind with one spelling, quoted in messages (`"("`, `"echo"`).
    Spelling(&'static str),
    /// A kind with many spellings, named in words (`identifier`).
    Words(&'static str),
    /// A kind PHP gives no label, shown by its internal name.
    Internal(&'static str),
}

#[expect(
    clippy::too_many_lines,
    reason = "a flat table of every token kind's label reads best in one place"
)]
fn label(kind: TokenKind) -> Label {
    use TokenKind as K;
    let spelling = match kind {
        K::Char(_) => return Label::Spelling(""),
        K::Integer => return Label::Words("integer"),
        K::Float => return Label::Words("floating-point number"),
        K::Identifier => return Label::Words("identifier"),
        K::FullyQualifiedName => return Label::Words("fully qualified name"),
        K::RelativeName => return Label::Words("namespace-relative name"),
        K::QualifiedName => return Label::Words("namespaced name"),
        K::Variable => return Label::Words("variable"),
        K::InlineHtml => return Label::Internal("T_INLINE_HTML"),
        K::StringFragment => return Label::Words("string content"),
        K::ConstantString => return Label::Words("quoted string"),
        K::VariableName => return Label::Words("variable name"),
        K::OffsetNumber => return Label::Words("number"),
        K::Comment => return Label::Words("comment"),
        K::DocComment => return Label::Words("doc comment"),
        K::OpenTag => return Label::Words("open tag"),
        K::Whitespace => return Label::Words("whitespace"),
        K::StartHeredoc => return Label::Words("heredoc start"),
        K::EndHeredoc => return Label::Words("heredoc end"),
        K::BadCharacter => return Label::Words("invalid character"),
        K::Include => "include",
        K::IncludeOnce => "include_once",
        K::Eval => "eval",
        K::Require => "require",
        K::RequireOnce => "require_once",
        K::LogicalOr => "or",
        K::LogicalXor => "xor",
        K::LogicalAnd => "and",
        K::Print => "print",
        K::Yield => "yield",
        K::YieldFrom => "yield from",
        K::Instanceof => "instanceof",
        K::New => "new",
        K::Clone => "clone",
        K::Exit => "exit",
        K::If => "if",
        K::Elseif => "elseif",
        K::Else => "else",
        K::Endif => "endif",
        K::Echo => "echo",
        K::Do => "do",
        K::While => "while",
        K::Endwhile => "endwhile",
        K::For => "for",
        K::Endfor => "endfor",
        K::Foreach => "foreach",
        K::Endforeach => "endforeach",
        K::Declare => "declare",
        K::Enddeclare => "enddeclare",
        K::As => "as",
        K::Switch => "switch",
        K::Endswitch => "endswitch",
        K::Case => "case",
        K::Default => "default",
        K::Match => "match",
        K::Break => "break",
        K::Continue => "continue",
        K::Goto => "goto",
        K::Function => "function",
        K::Fn => "fn",
        K::Const => "const",
        K::Return => "return",
        K::Try => "try",
        K::Catch => "catch",
        K::Finally => "finally",
        K::Throw => "throw",
        K::Use => "use",
        K::Insteadof => "insteadof",
        K::Global => "global",
        K::Static => "static",
        K::Abstract => "abstract",
        K::Final => "final",
        K::Private => "private",
        K::Protected => "protected",
        K::Public => "public",
        K::PrivateSet => "private(set)",
        K::ProtectedSet => "protected(set)",
        K::PublicSet => "public(set)",
        K::Readonly => "readonly",
        K::Var => "var",
        K::Unset => "unset",
        K::Isset => "isset",
        K::Empty => "empty",
        K::HaltCompiler => "__halt_compiler",
        K::Class => "class",
        K::Trait => "trait",
        K::Interface => "interface",
        K::Enum => "enum",
        K::Extends => "extends",
        K::Implements => "implements",
        K::Namespace => "namespace",
        K::List => "list",
        K::Array => "array",
        K::Callable => "callable",
        K::LineMagic => "__LINE__",
        K::FileMagic => "__FILE__",
        K::DirMagic => "__DIR__",
        K::ClassMagic => "__CLASS__",
        K::TraitMagic => "__TRAIT__",
        K::MethodMagic => "__METHOD__",
        K::FunctionMagic => "__FUNCTION__",
        K::PropertyMagic => "__PROPERTY__",
        K::NamespaceMagic => "__NAMESPACE__",
        K::Attribute => "#[",
        K::PlusAssign => "+=",
        K::MinusAssign => "-=",
        K::MulAssign => "*=",
        K::DivAssign => "/=",
        K::ConcatAssign => ".=",
        K::ModAssign => "%=",
        K::AndAssign => "&=",
        K::OrAssign => "|=",
        K::XorAssign => "^=",
        K::ShiftLeftAssign => "<<=",
        K::ShiftRightAssign => ">>=",
        K::CoalesceAssign => "??=",
        K::BooleanOr => "||",
        K::BooleanAnd => "&&",
        K::Equal => "==",
        K::NotEqual => "!=",
        K::Identical => "===",
        K::NotIdentical => "!==",
        K::LessOrEqual => "<=",
        K::GreaterOrEqual => ">=",
        K::Spaceship => "<=>",
        K::ShiftLeft => "<<",
        K::ShiftRight => ">>",
        K::Increment => "++",
        K::Decrement => "--",
        K::IntCast => "(int)",
        K::FloatCast => "(float)",
        K::StringCast => "(string)",
        K::ArrayCast => "(array)",
        K::ObjectCast => "(object)",
        K::BoolCast => "(bool)",
        K::UnsetCast => "(unset)",
        K::VoidCast => "(void)",
        K::ObjectOperator => "->",
        K::NullsafeObjectOperator => "?->",
        K::DoubleArrow => "=>",
        K::OpenTagWithEcho => "<?=",
        K::CloseTag => "?>",
        K::DollarOpenCurlyBraces => "${",
        K::CurlyOpen => "{$",
        K::DoubleColon => "::",
        K::NsSeparator => "\\",
        K::Ellipsis => "...",
        K::Coalesce => "??",
        K::Pow => "**",
        K::PowAssign => "**=",
        K::Pipe => "|>",
        K::AmpersandFollowedByVarOrVararg | K::AmpersandNotFollowedByVarOrVararg => "&",
    };
    Label::Spelling(spelling)
}

/// The token kinds in the order the reference grammar numbers them, which is the order a syntax
/// error lists expected tokens in. The end of input comes first.
const GRAMMAR_ORDER: &[TokenKind] = {
    use TokenKind as K;
    &[
        END,
        K::Throw,
        K::Include,
        K::IncludeOnce,
        K::Require,
        K::RequireOnce,
        K::LogicalOr,
        K::LogicalXor,
        K::LogicalAnd,
        K::Print,
        K::Yield,
        K::DoubleArrow,
        K::YieldFrom,
        K::Char(b'='),
        K::PlusAssign,
        K::MinusAssign,
        K::MulAssign,
        K::DivAssign,
        K::ConcatAssign,
        K::ModAssign,
        K::AndAssign,
        K::OrAssign,
        K::XorAssign,
        K::ShiftLeftAssign,
        K::ShiftRightAssign,
        K::PowAssign,
        K::CoalesceAssign,
        K::Char(b'?'),
        K::Char(b':'),
        K::Coalesce,
        K::BooleanOr,
        K::BooleanAnd,
        K::Char(b'|'),
        K::Char(b'^'),
        K::AmpersandNotFollowedByVarOrVararg,
        K::AmpersandFollowedByVarOrVararg,
        K::Equal,
        K::NotEqual,
        K::Identical,
        K::NotIdentical,
        K::Spaceship,
        K::Char(b'<'),
        K::LessOrEqual,
        K::Char(b'>'),
        K::GreaterOrEqual,
        K::Pipe,
        K::Char(b'.'),
        K::ShiftLeft,
        K::ShiftRight,
        K::Char(b'+'),
        K::Char(b'-'),
        K::Char(b'*'),
        K::Char(b'/'),
        K::Char(b'%'),
        K::Char(b'!'),
        K::Instanceof,
        K::Char(b'~'),
        K::IntCast,
        K::FloatCast,
        K::StringCast,
        K::ArrayCast,
        K::ObjectCast,
        K::BoolCast,
        K::UnsetCast,
        K::Char(b'@'),
        K::Pow,
        K::Clone,
        K::Elseif,
        K::Else,
        K::Integer,
        K::Float,
        K::Identifier,
        K::FullyQualifiedName,
        K::RelativeName,
        K::QualifiedName,
        K::Variable,
        K::InlineHtml,
        K::StringFragment,
        K::ConstantString,
        K::VariableName,
        K::OffsetNumber,
        K::Eval,
        K::New,
        K::Exit,
        K::If,
        K::Endif,
        K::Echo,
        K::Do,
        K::While,
        K::Endwhile,
        K::For,
        K::Endfor,
        K::Foreach,
        K::Endforeach,
        K::Declare,
        K::Enddeclare,
        K::As,
        K::Switch,
        K::Endswitch,
        K::Case,
        K::Default,
        K::Match,
        K::Break,
        K::Continue,
        K::Goto,
        K::Function,
        K::Fn,
        K::Const,
        K::Return,
        K::Try,
        K::Catch,
        K::Finally,
        K::Use,
        K::Insteadof,
        K::Global,
        K::Static,
        K::Abstract,
        K::Final,
        K::Private,
        K::Protected,
        K::Public,
        K::PrivateSet,
        K::ProtectedSet,
        K::PublicSet,
        K::Readonly,
        K::Var,
        K::Unset,
        K::Isset,
        K::Empty,
        K::HaltCompiler,
        K::Class,
        K::Trait,
        K::Interface,
        K::Enum,
        K::Extends,
        K::Implements,
        K::Namespace,
        K::List,
        K::Array,
        K::Callable,
        K::LineMagic,
        K::FileMagic,
        K::DirMagic,
        K::ClassMagic,
        K::TraitMagic,
        K::MethodMagic,
        K::FunctionMagic,
        K::PropertyMagic,
        K::NamespaceMagic,
        K::Attribute,
        K::Increment,
        K::Decrement,
        K::VoidCast,
        K::ObjectOperator,
        K::NullsafeObjectOperator,
        K::Comment,
        K::DocComment,
        K::OpenTag,
        K::OpenTagWithEcho,
        K::CloseTag,
        K::Whitespace,
        K::StartHeredoc,
        K::EndHeredoc,
        K::DollarOpenCurlyBraces,
        K::CurlyOpen,
        K::DoubleColon,
        K::NsSeparator,
        K::Ellipsis,
        K::BadCharacter,
        K::Char(b','),
        K::Char(b']'),
        K::Char(b';'),
        K::Char(b'('),
        K::Char(b')'),
        K::Char(b'{'),
        K::Char(b'}'),
        K::Char(b'['),
        K::Char(b'`'),
        K::Char(b'"'),
        K::Char(b'$'),
    ]
};

/// Position of `kind` in [`GRAMMAR_ORDER`]; kinds the grammar never expects sort last.
fn rank(kind: TokenKind) -> usize {
    GRAMMAR_ORDER
        .iter()
        .position(|&candidate| candidate == kind)
        .unwrap_or(GRAMMAR_ORDER.len())
}

/// How an expected token is written in the `expecting` part of a syntax error.
fn expected_name(kind: TokenKind) -> String {
    match (kind, label(kind)) {
        (END, _) => "end of file".to_owned(),
        (TokenKind::AmpersandNotFollowedByVarOrVararg, _) => "token \"&\"".to_owned(),
        (TokenKind::Char(byte), _) => format!("\"{}\"", char::from(byte)),
        (_, Label::Spelling(text)) => format!("\"{text}\""),
        (_, Label::Words(text) | Label::Internal(text)) => text.to_owned(),
    }
}

/// The most expected tokens a syntax error lists; with more, it lists none.
const MAX_EXPECTED: usize = 4;

/// The message of a syntax error at a token of `kind` whose source text is `text`, given the
/// kinds that would have been accepted instead.
pub(crate) fn syntax_error(kind: TokenKind, text: &[u8], expected: &[TokenKind]) -> Vec<u8> {
    let mut message = b"syntax error, unexpected ".to_vec();
    let complete = describe_unexpected(kind, text, &mut message);
    if complete && !expected.is_empty() && expected.len() <= MAX_EXPECTED {
        let mut sorted = expected.to_vec();
        sorted.sort_by_key(|&kind| rank(kind));
        sorted.dedup();
        for (index, &kind) in sorted.iter().enumerate() {
            message.extend_from_slice(if index == 0 { b", expecting " } else { b" or " });
            message.extend_from_slice(expected_name(kind).as_bytes());
        }
    }
    message
}

/// Longest token text quoted in full; longer text is cut to [`QUOTED_PREFIX`] bytes and `...`.
const QUOTED_LIMIT: usize = 33;
const QUOTED_PREFIX: usize = 30;

/// Writes how a syntax error names the token it stopped at. Returns `false` when a NUL byte cut the
/// quoted text short: the reference message then ends with this description.
fn describe_unexpected(kind: TokenKind, text: &[u8], out: &mut Vec<u8>) -> bool {
    let words = match (kind, label(kind)) {
        (END, _) => {
            out.extend_from_slice(b"end of file");
            return true;
        }
        (TokenKind::Char(b'"'), _) => {
            out.extend_from_slice(b"double-quote mark");
            return true;
        }
        (TokenKind::Char(byte), _) => {
            out.extend_from_slice(b"token \"");
            out.push(byte);
            out.push(b'"');
            return true;
        }
        (TokenKind::BadCharacter, _) if text.len() == 1 => {
            out.extend_from_slice(format!("character 0x{:02X}", text[0]).as_bytes());
            return true;
        }
        (_, Label::Spelling(spelling)) => {
            out.extend_from_slice(b"token \"");
            out.extend_from_slice(spelling.as_bytes());
            out.push(b'"');
            return true;
        }
        (_, Label::Words(words) | Label::Internal(words)) => words,
    };
    let mut content = text
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(text, |end| &text[..end]);
    let words = match (kind, content.first()) {
        (TokenKind::ConstantString, Some(b'"')) => "double-quoted string",
        (TokenKind::ConstantString, Some(b'\'')) => "single-quoted string",
        _ => words,
    };
    if let [b'\'' | b'"', rest @ ..] = content {
        content = rest;
    }
    if let [rest @ .., b'\'' | b'"'] = content {
        content = rest;
    }
    out.extend_from_slice(words.as_bytes());
    out.extend_from_slice(b" \"");
    let shown = if content.len() > QUOTED_LIMIT {
        &content[..QUOTED_PREFIX]
    } else {
        content
    };
    let printed = until_nul(shown);
    out.extend_from_slice(printed);
    if content.len() > QUOTED_LIMIT {
        out.extend_from_slice(b"...");
    }
    out.push(b'"');
    printed.len() == shown.len()
}

/// The bytes before the first NUL, as C string formatting would print them.
fn until_nul(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .map_or(bytes, |end| &bytes[..end])
}

#[cfg(test)]
mod tests {
    use super::{END, GRAMMAR_ORDER, expected_name, rank, syntax_error};
    use php_lexer::TokenKind as K;

    fn message(kind: K, text: &[u8], expected: &[K]) -> String {
        String::from_utf8_lossy(&syntax_error(kind, text, expected)).into_owned()
    }

    #[test]
    fn grammar_order_lists_each_kind_once() {
        for (index, &kind) in GRAMMAR_ORDER.iter().enumerate() {
            assert_eq!(rank(kind), index, "{kind:?}");
        }
        assert_eq!(rank(K::Char(b'#')), GRAMMAR_ORDER.len());
        for &kind in K::NAMED {
            assert!(rank(kind) < GRAMMAR_ORDER.len(), "{kind:?}");
        }
    }

    #[test]
    fn keywords_and_operators_are_quoted_by_their_spelling() {
        let source = b"<?php include_once eval require require_once or xor print instanceof if do \
            while endwhile for endfor foreach endforeach declare enddeclare as switch endswitch \
            match break continue goto return try catch finally insteadof global static private \
            protected private(set) protected(set) var unset isset empty trait interface array \
            callable __LINE__ __FILE__ __DIR__ __CLASS__ __METHOD__ __FUNCTION__ __PROPERTY__ \
            __NAMESPACE__ *= /= |= ** (float) (array) (object) (unset) (void)";
        let Ok(lexed) = php_lexer::tokenize(source, php_lexer::LexerOptions::default()) else {
            panic!("source fits");
        };
        let mut checked = 0;
        for token in &lexed.tokens {
            if matches!(token.kind, K::OpenTag | K::Whitespace) {
                continue;
            }
            let text = token.text(source);
            let spelling = String::from_utf8_lossy(text).to_ascii_lowercase();
            let spelling = if spelling.starts_with("__") {
                spelling.to_ascii_uppercase()
            } else {
                spelling
            };
            assert_eq!(
                message(token.kind, text, &[]),
                format!("syntax error, unexpected token \"{spelling}\"")
            );
            checked += 1;
        }
        assert_eq!(checked, 61);
        assert_eq!(
            message(K::OpenTagWithEcho, b"<?=", &[]),
            "syntax error, unexpected token \"<?=\""
        );
        assert_eq!(
            message(K::CloseTag, b"?>", &[]),
            "syntax error, unexpected token \"?>\""
        );
        assert_eq!(
            message(K::VariableName, b"a", &[]),
            "syntax error, unexpected variable name \"a\""
        );
    }

    #[test]
    fn unexpected_tokens_are_named_like_php() {
        assert_eq!(
            message(END, b"", &[]),
            "syntax error, unexpected end of file"
        );
        assert_eq!(
            message(K::Char(b';'), b"?>", &[]),
            "syntax error, unexpected token \";\""
        );
        assert_eq!(
            message(K::Char(b'"'), b"\"", &[]),
            "syntax error, unexpected double-quote mark"
        );
        assert_eq!(
            message(K::NsSeparator, b"\\", &[]),
            "syntax error, unexpected token \"\\\""
        );
        assert_eq!(
            message(K::AmpersandNotFollowedByVarOrVararg, b"&", &[]),
            "syntax error, unexpected token \"&\""
        );
        assert_eq!(
            message(K::Echo, b"<?=", &[]),
            "syntax error, unexpected token \"echo\""
        );
        assert_eq!(
            message(K::BadCharacter, b"\x01", &[]),
            "syntax error, unexpected character 0x01"
        );
        assert_eq!(
            message(K::Variable, b"$a", &[]),
            "syntax error, unexpected variable \"$a\""
        );
        assert_eq!(
            message(K::ConstantString, b"'it'", &[]),
            "syntax error, unexpected single-quoted string \"it\""
        );
        assert_eq!(
            message(K::ConstantString, b"\"it\"", &[]),
            "syntax error, unexpected double-quoted string \"it\""
        );
        assert_eq!(
            message(K::ConstantString, b"b'it'", &[]),
            "syntax error, unexpected quoted string \"b'it\""
        );
        assert_eq!(
            message(K::StartHeredoc, b"<<<\"A\"\n", &[]),
            "syntax error, unexpected heredoc start \"<<<\"A\""
        );
        assert_eq!(
            message(K::InlineHtml, b"x\0y", &[END]),
            "syntax error, unexpected T_INLINE_HTML \"x\""
        );
        assert_eq!(
            message(K::InlineHtml, &[b'x'; 40], &[END]),
            format!(
                "syntax error, unexpected T_INLINE_HTML \"{}...\", expecting end of file",
                "x".repeat(30)
            )
        );
        assert_eq!(
            message(K::InlineHtml, b"x\0y", &[]),
            "syntax error, unexpected T_INLINE_HTML \"x\""
        );
        let long = message(K::Identifier, b"abcdefghijklmnopqrstuvwxyz0123456789", &[]);
        assert_eq!(
            long,
            "syntax error, unexpected identifier \"abcdefghijklmnopqrstuvwxyz0123...\""
        );
        let exact = message(K::Identifier, b"abcdefghijklmnopqrstuvwxyz0123456", &[]);
        assert!(exact.ends_with("0123456\""));
    }

    #[test]
    fn expected_tokens_follow_grammar_order_up_to_four() {
        assert_eq!(
            message(K::Variable, b"$b", &[K::Char(b')')]),
            "syntax error, unexpected variable \"$b\", expecting \")\""
        );
        assert_eq!(
            message(K::Variable, b"$b", &[K::Char(b';'), K::Char(b',')]),
            "syntax error, unexpected variable \"$b\", expecting \",\" or \";\""
        );
        let four = [
            K::CurlyOpen,
            K::EndHeredoc,
            K::Variable,
            K::DollarOpenCurlyBraces,
        ];
        assert_eq!(
            message(END, b"", &four),
            "syntax error, unexpected end of file, expecting variable or heredoc end or \"${\" or \"{$\""
        );
        let five = [
            K::Char(b'('),
            K::Char(b')'),
            K::Char(b'['),
            K::Char(b']'),
            K::Char(b';'),
        ];
        assert_eq!(
            message(END, b"", &five),
            "syntax error, unexpected end of file"
        );
        assert_eq!(expected_name(K::NsSeparator), "\"\\\"");
        assert_eq!(
            expected_name(K::AmpersandNotFollowedByVarOrVararg),
            "token \"&\""
        );
        assert_eq!(expected_name(K::InlineHtml), "T_INLINE_HTML");
        assert_eq!(expected_name(END), "end of file");
        assert_eq!(expected_name(K::Readonly), "\"readonly\"");
    }
}
