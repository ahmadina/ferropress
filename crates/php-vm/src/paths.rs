//! `dirname` and `basename` for `/`-separated paths.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_dirname(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.is_empty() {
            return Err(self.too_few_at_least("dirname", 0, 1, args));
        }
        if args.len() > 2 {
            return Err(self.too_many_at_most("dirname", args.len(), 2, args));
        }
        let mut path = self.typed_string("dirname", 1, "path", &args[0].slot.value(), args)?;
        let levels = match args.get(1) {
            Some(arg) => self.typed_int("dirname", 2, "levels", &arg.slot.value(), args)?,
            None => 1,
        };
        if levels < 1 {
            return Err(self.throw_from(
                "ValueError",
                b"dirname(): Argument #2 ($levels) must be greater than or equal to 1".to_vec(),
                b"dirname",
                args.iter().map(|arg| arg.slot.value()).collect(),
            ));
        }
        let mut remaining = levels;
        while remaining > 0 {
            let next = dirname_once(&path);
            if next == path {
                break;
            }
            path = next;
            remaining -= 1;
        }
        self.write(result, Value::String(PhpString::new(&path)))
    }

    pub(crate) fn native_basename(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.is_empty() {
            return Err(self.too_few_at_least("basename", 0, 1, args));
        }
        if args.len() > 2 {
            return Err(self.too_many_at_most("basename", args.len(), 2, args));
        }
        let path = self.typed_string("basename", 1, "path", &args[0].slot.value(), args)?;
        let suffix = match args.get(1) {
            Some(arg) => self.typed_string("basename", 2, "suffix", &arg.slot.value(), args)?,
            None => Vec::new(),
        };
        self.write(
            result,
            Value::String(PhpString::new(&basename_of(&path, &suffix))),
        )
    }
}

fn dirname_once(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut end = path.len();
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return vec![b'/'];
    }
    let Some(slash) = path[..end].iter().rposition(|&byte| byte == b'/') else {
        return vec![b'.'];
    };
    let mut parent = slash;
    while parent > 0 && path[parent - 1] == b'/' {
        parent -= 1;
    }
    if parent == 0 {
        return vec![b'/'];
    }
    path[..parent].to_vec()
}

fn basename_of(path: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut end = path.len();
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return Vec::new();
    }
    let start = path[..end]
        .iter()
        .rposition(|&byte| byte == b'/')
        .map_or(0, |index| index + 1);
    let name = &path[start..end];
    if !suffix.is_empty() && suffix.len() < name.len() && name.ends_with(suffix) {
        name[..name.len() - suffix.len()].to_vec()
    } else {
        name.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{basename_of, dirname_once};

    #[test]
    fn dirname_follows_unix_separators() {
        assert_eq!(dirname_once(b""), b"");
        assert_eq!(dirname_once(b"/"), b"/");
        assert_eq!(dirname_once(b"foo"), b".");
        assert_eq!(dirname_once(b"/foo"), b"/");
        assert_eq!(dirname_once(b"/foo/bar"), b"/foo");
        assert_eq!(dirname_once(b"/foo/bar/"), b"/foo");
        assert_eq!(dirname_once(b"."), b".");
        assert_eq!(dirname_once(b"./a"), b".");
        assert_eq!(dirname_once(br"a\b\c"), b".");
    }

    #[test]
    fn basename_strips_a_shorter_suffix() {
        assert_eq!(basename_of(b"/foo/bar.txt", b""), b"bar.txt");
        assert_eq!(basename_of(b"/foo/bar.txt", b".txt"), b"bar");
        assert_eq!(basename_of(b"/foo/bar.txt", b".TXT"), b"bar.txt");
        assert_eq!(basename_of(b"/foo/bar/", b""), b"bar");
        assert_eq!(basename_of(b"/", b""), b"");
        assert_eq!(basename_of(b"bar.txt", b"bar.txt"), b"bar.txt");
        assert_eq!(basename_of(b"bar.txt", b"ar.txt"), b"b");
    }
}
