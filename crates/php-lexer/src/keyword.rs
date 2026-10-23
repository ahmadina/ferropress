//! Reserved words and cast names.

use crate::token::TokenKind as K;

/// The longest reserved word (`__halt_compiler`).
const LONGEST: usize = 15;

/// Lowercases `word` into a buffer when it is short enough to be a reserved word.
fn lowercase(word: &[u8]) -> Option<([u8; LONGEST], usize)> {
    if word.len() > LONGEST {
        return None;
    }
    let mut buffer = [0; LONGEST];
    for (slot, byte) in buffer.iter_mut().zip(word) {
        *slot = byte.to_ascii_lowercase();
    }
    Some((buffer, word.len()))
}

/// The token kind of a whole identifier, matched without regard to ASCII case; `None` for an
/// ordinary name. `enum`, `yield from` and `public(set)` need lookahead and are decided by the lexer.
pub(crate) fn reserved(word: &[u8]) -> Option<K> {
    let (buffer, length) = lowercase(word)?;
    Some(match &buffer[..length] {
        b"abstract" => K::Abstract,
        b"and" => K::LogicalAnd,
        b"array" => K::Array,
        b"as" => K::As,
        b"break" => K::Break,
        b"callable" => K::Callable,
        b"case" => K::Case,
        b"catch" => K::Catch,
        b"class" => K::Class,
        b"clone" => K::Clone,
        b"const" => K::Const,
        b"continue" => K::Continue,
        b"declare" => K::Declare,
        b"default" => K::Default,
        b"die" | b"exit" => K::Exit,
        b"do" => K::Do,
        b"echo" => K::Echo,
        b"else" => K::Else,
        b"elseif" => K::Elseif,
        b"empty" => K::Empty,
        b"enddeclare" => K::Enddeclare,
        b"endfor" => K::Endfor,
        b"endforeach" => K::Endforeach,
        b"endif" => K::Endif,
        b"endswitch" => K::Endswitch,
        b"endwhile" => K::Endwhile,
        b"eval" => K::Eval,
        b"extends" => K::Extends,
        b"final" => K::Final,
        b"finally" => K::Finally,
        b"fn" => K::Fn,
        b"for" => K::For,
        b"foreach" => K::Foreach,
        b"function" => K::Function,
        b"global" => K::Global,
        b"goto" => K::Goto,
        b"if" => K::If,
        b"implements" => K::Implements,
        b"include" => K::Include,
        b"include_once" => K::IncludeOnce,
        b"instanceof" => K::Instanceof,
        b"insteadof" => K::Insteadof,
        b"interface" => K::Interface,
        b"isset" => K::Isset,
        b"list" => K::List,
        b"match" => K::Match,
        b"namespace" => K::Namespace,
        b"new" => K::New,
        b"or" => K::LogicalOr,
        b"print" => K::Print,
        b"private" => K::Private,
        b"protected" => K::Protected,
        b"public" => K::Public,
        b"readonly" => K::Readonly,
        b"require" => K::Require,
        b"require_once" => K::RequireOnce,
        b"return" => K::Return,
        b"static" => K::Static,
        b"switch" => K::Switch,
        b"throw" => K::Throw,
        b"trait" => K::Trait,
        b"try" => K::Try,
        b"unset" => K::Unset,
        b"use" => K::Use,
        b"var" => K::Var,
        b"while" => K::While,
        b"xor" => K::LogicalXor,
        b"yield" => K::Yield,
        b"__class__" => K::ClassMagic,
        b"__dir__" => K::DirMagic,
        b"__file__" => K::FileMagic,
        b"__function__" => K::FunctionMagic,
        b"__halt_compiler" => K::HaltCompiler,
        b"__line__" => K::LineMagic,
        b"__method__" => K::MethodMagic,
        b"__namespace__" => K::NamespaceMagic,
        b"__property__" => K::PropertyMagic,
        b"__trait__" => K::TraitMagic,
        _ => return None,
    })
}

/// The `(set)` form of a visibility keyword.
pub(crate) fn with_set(visibility: K) -> Option<K> {
    match visibility {
        K::Public => Some(K::PublicSet),
        K::Protected => Some(K::ProtectedSet),
        K::Private => Some(K::PrivateSet),
        _ => None,
    }
}

/// The cast a type name in `( name )` denotes, matched without regard to ASCII case.
pub(crate) fn cast(name: &[u8]) -> Option<K> {
    let (buffer, length) = lowercase(name)?;
    Some(match &buffer[..length] {
        b"int" | b"integer" => K::IntCast,
        b"float" | b"double" | b"real" => K::FloatCast,
        b"string" | b"binary" => K::StringCast,
        b"array" => K::ArrayCast,
        b"object" => K::ObjectCast,
        b"bool" | b"boolean" => K::BoolCast,
        b"unset" => K::UnsetCast,
        b"void" => K::VoidCast,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{cast, reserved, with_set};
    use crate::token::TokenKind;

    #[test]
    fn reserved_words_ignore_case() {
        assert_eq!(reserved(b"FuNcTiOn"), Some(TokenKind::Function));
        assert_eq!(reserved(b"die"), Some(TokenKind::Exit));
        assert_eq!(reserved(b"__HALT_COMPILER"), Some(TokenKind::HaltCompiler));
        assert_eq!(reserved(b"__halt_compilers"), None);
        assert_eq!(reserved(b"enum"), None);
        assert_eq!(reserved(b"functions"), None);
        assert_eq!(reserved(b"\xc3\xa9"), None);
    }

    #[test]
    fn visibility_set_forms() {
        assert_eq!(with_set(TokenKind::Public), Some(TokenKind::PublicSet));
        assert_eq!(
            with_set(TokenKind::Protected),
            Some(TokenKind::ProtectedSet)
        );
        assert_eq!(with_set(TokenKind::Private), Some(TokenKind::PrivateSet));
        assert_eq!(with_set(TokenKind::Static), None);
    }

    #[test]
    fn casts() {
        assert_eq!(cast(b"INTEGER"), Some(TokenKind::IntCast));
        assert_eq!(cast(b"real"), Some(TokenKind::FloatCast));
        assert_eq!(cast(b"binary"), Some(TokenKind::StringCast));
        assert_eq!(cast(b"Boolean"), Some(TokenKind::BoolCast));
        assert_eq!(cast(b"void"), Some(TokenKind::VoidCast));
        assert_eq!(cast(b"array"), Some(TokenKind::ArrayCast));
        assert_eq!(cast(b"object"), Some(TokenKind::ObjectCast));
        assert_eq!(cast(b"unset"), Some(TokenKind::UnsetCast));
        assert_eq!(cast(b"mixed"), None);
        assert_eq!(cast(b"averyveryverylongname"), None);
    }
}
