//! `include`, `require` and their `_once` forms.

use std::collections::HashSet;
use std::path::PathBuf;

use php_compiler::bytecode::{Instruction, Operand};
use php_compiler::compile;
use php_parser::{ParseOptions, parse};
use php_value::Value;

use crate::Output;
use crate::machine::{Exec, Flow, Machine, Stop};

type Included = HashSet<Box<[u8]>>;

impl<O: Output> Machine<'_, O> {
    /// Runs the file named by `a` and writes its return value to `result`.
    pub(crate) fn include_file(&mut self, ins: &Instruction) -> Exec {
        let named = self.read(ins.a)?;
        let path = self.stringify(&named)?;
        let once = ins.flags & 1 != 0;
        let required = ins.flags & 2 != 0;
        let name = match ins.flags {
            1 => "include_once",
            2 => "require",
            3 => "require_once",
            _ => "include",
        };
        let resolved = resolve_path(&self.file, path.as_bytes());
        let key: Box<[u8]> = resolved.to_string_lossy().as_bytes().into();
        if once && self.included.contains(&key) {
            return self.write(ins.result, Value::Bool(true));
        }
        let source = match std::fs::read(&resolved) {
            Ok(bytes) => bytes,
            Err(_error) => {
                return self.missing_include(name, path.as_bytes(), required, ins.result);
            }
        };
        self.included.insert(key);
        let value = self.run_included(&source, resolved.to_string_lossy().as_bytes())?;
        self.write(ins.result, value)
    }

    fn run_included(&mut self, source: &[u8], file: &[u8]) -> Exec<Value> {
        let Ok(parsed) = parse(source, ParseOptions::default()) else {
            return Err(self.throw("ParseError", b"parse failed".to_vec()));
        };
        let program = match parsed.result {
            Ok(program) => program,
            Err(error) => return Err(self.throw("ParseError", error.to_string().into_bytes())),
        };
        let compiled = match compile(&program, file) {
            Ok(compiled) => compiled,
            Err(error) => return Err(self.throw("Error", error.to_string().into_bytes())),
        };
        if let Some(fatal) = compiled.diagnostics.fatal {
            return Err(self.throw("ParseError", fatal.message));
        }
        let Some(unit) = compiled.unit else {
            return Err(self.throw("Error", b"include failed".to_vec()));
        };
        self.absorb_classes(&unit)?;
        let mut output = Vec::new();
        let mut nested = Machine::new(&unit, file, &mut output);
        nested.included.clone_from(&self.included);
        nested.constants = std::rc::Rc::clone(&self.constants);
        let result = nested.run_value();
        self.write_output(&output);
        match result {
            Ok((value, included)) => {
                self.included = included;
                Ok(value)
            }
            Err(stop) => Err(stop),
        }
    }

    fn missing_include(
        &mut self,
        name: &str,
        path: &[u8],
        required: bool,
        result: Operand,
    ) -> Exec {
        let mut open = name.as_bytes().to_vec();
        open.push(b'(');
        open.extend_from_slice(path);
        open.extend_from_slice(b"): Failed to open stream: No such file or directory");
        self.display(b"Warning", &open);
        if required {
            let mut message = b"Failed opening required '".to_vec();
            message.extend_from_slice(path);
            message.extend_from_slice(b"' (include_path='.:/usr/local/lib/php')");
            return Err(self.throw("Error", message));
        }
        let mut opening = name.as_bytes().to_vec();
        opening.extend_from_slice(b"(): Failed opening '");
        opening.extend_from_slice(path);
        opening.extend_from_slice(b"' for inclusion (include_path='.:/usr/local/lib/php')");
        self.display(b"Warning", &opening);
        self.write(result, Value::Bool(false))
    }

    /// Runs this machine to completion and returns the main code's return value.
    pub(crate) fn run_value(mut self) -> Result<(Value, Included), Stop> {
        let main = self.take_frame(0);
        self.frames.push(main);
        loop {
            match self.step() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Finished) => {
                    let included = self.included;
                    return Ok((self.return_value, included));
                }
                Err(Stop::Thrown(thrown)) => {
                    self.flush();
                    match self.catch_thrown(&thrown) {
                        Ok(true) => {}
                        Ok(false) => return Err(Stop::Thrown(thrown)),
                        Err(stop) => return Err(stop),
                    }
                }
                Err(stop) => return Err(stop),
            }
        }
    }
}

pub(crate) fn resolve_path(current_file: &[u8], path: &[u8]) -> PathBuf {
    let requested = PathBuf::from(String::from_utf8_lossy(path).as_ref());
    if requested.is_absolute() {
        return requested;
    }
    let current = PathBuf::from(String::from_utf8_lossy(current_file).as_ref());
    match current.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(requested),
        _ => requested,
    }
}
