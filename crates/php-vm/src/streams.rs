//! `file_exists`, `file_get_contents`, `file_put_contents`, `is_file` and `is_dir`.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::includes::resolve_path;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_file_exists(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("file_exists", args)?;
        let path = self.string_arg("file_exists", &value)?;
        let resolved = resolve_path(&self.file, &path);
        self.write(result, Value::Bool(resolved.exists()))
    }

    pub(crate) fn native_file_get_contents(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("file_get_contents", args)?;
        let path = self.string_arg("file_get_contents", &value)?;
        let resolved = resolve_path(&self.file, &path);
        if let Ok(bytes) = std::fs::read(&resolved) {
            self.write(result, Value::String(PhpString::new(&bytes)))
        } else {
            self.failed_stream("file_get_contents", path.as_slice(), &resolved);
            self.write(result, Value::Bool(false))
        }
    }

    pub(crate) fn native_file_put_contents(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("file_put_contents", args.len(), 2, args));
        }
        let path = self.string_arg("file_put_contents", &args[0].slot.value())?;
        let data = self.stringify(&args[1].slot.value())?;
        let resolved = resolve_path(&self.file, &path);
        if std::fs::write(&resolved, data.as_bytes()).is_ok() {
            let n = i64::try_from(data.as_bytes().len()).unwrap_or(i64::MAX);
            self.write(result, Value::Int(n))
        } else {
            self.failed_stream("file_put_contents", path.as_slice(), &resolved);
            self.write(result, Value::Bool(false))
        }
    }

    pub(crate) fn native_is_file(&mut self, args: &[Argument], result: Operand) -> Exec {
        self.native_path_kind("is_file", args, result, std::path::Path::is_file)
    }

    pub(crate) fn native_is_dir(&mut self, args: &[Argument], result: Operand) -> Exec {
        self.native_path_kind("is_dir", args, result, std::path::Path::is_dir)
    }

    fn native_path_kind(
        &mut self,
        name: &'static str,
        args: &[Argument],
        result: Operand,
        test: fn(&std::path::Path) -> bool,
    ) -> Exec {
        if args.len() != 1 {
            return Err(if args.is_empty() {
                self.too_few(name, 0, 1, args)
            } else {
                self.too_many(name, args.len(), 1, args)
            });
        }
        let path = self.typed_string(name, 1, "filename", &args[0].slot.value(), args)?;
        let resolved = resolve_path(&self.file, &path);
        self.write(result, Value::Bool(test(&resolved)))
    }

    fn failed_stream(&mut self, name: &str, path: &[u8], resolved: &std::path::Path) {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"(");
        message.extend_from_slice(path);
        message.extend_from_slice(b"): Failed to open stream: No such file or directory");
        let _ = resolved;
        self.display(b"Warning", &message);
    }
}

#[cfg(test)]
mod tests {
    use super::super::includes::resolve_path;
    use std::path::Path;

    #[test]
    fn relative_paths_join_the_current_file_directory() {
        let path = resolve_path(b"/tmp/main.php", b"data.txt");
        assert_eq!(path, Path::new("/tmp/data.txt"));
    }
}
