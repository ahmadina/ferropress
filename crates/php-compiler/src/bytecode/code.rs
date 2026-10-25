//! Code objects, compiled units and their disassembly.

use std::fmt::{self, Write as _};

use php_value::write_float;

use super::{Instruction, Literal, Operand, OperandKind};

/// One parameter of a function.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// The local slot the argument is bound to.
    pub slot: u32,
    /// Whether the argument is taken by reference.
    pub by_ref: bool,
    /// Whether it collects the remaining arguments.
    pub variadic: bool,
}

/// The compiled body of a function, method, closure or file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Code {
    /// The name `__FUNCTION__` shows: empty for a file's main code.
    pub name: Box<[u8]>,
    /// The instructions.
    pub instructions: Vec<Instruction>,
    /// The line of each instruction, in parallel with `instructions`.
    pub lines: Vec<u32>,
    /// The literal pool.
    pub literals: Vec<Literal>,
    /// The name of each local slot, without `$`.
    pub slots: Vec<Box<[u8]>>,
    /// How many temporaries the code uses.
    pub temporaries: u32,
    /// The parameters, in order.
    pub params: Vec<Parameter>,
    /// Whether the function returns by reference.
    pub by_ref: bool,
}

/// A function the file declares.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    /// The namespaced name as declared.
    pub name: Box<[u8]>,
    /// The line of `function`.
    pub line: u32,
    /// The body.
    pub code: Code,
}

/// What compiling one file produces.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unit {
    /// The file's main code.
    pub main: Code,
    /// The functions the file declares, in order.
    pub functions: Vec<FunctionDecl>,
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.main)?;
        for function in &self.functions {
            write!(f, "\n{}", function.code)?;
        }
        Ok(())
    }
}

/// Writes `value` as the disassembler shows literals.
fn write_literal(out: &mut String, value: &Literal) {
    match value {
        Literal::Null => out.push_str("null"),
        Literal::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Literal::Int(value) => {
            let _ = write!(out, "{value}");
        }
        Literal::Float(value) => {
            let mut text = Vec::new();
            write_float(&mut text, *value);
            out.push_str(&String::from_utf8_lossy(&text));
            if !text
                .iter()
                .any(|byte| matches!(byte, b'.' | b'E' | b'N' | b'I'))
            {
                out.push_str(".0");
            }
        }
        Literal::String(text) => {
            out.push('"');
            for &byte in text {
                match byte {
                    b'"' | b'\\' => {
                        out.push('\\');
                        out.push(char::from(byte));
                    }
                    b'\n' => out.push_str("\\n"),
                    0x20..=0x7e => out.push(char::from(byte)),
                    _ => {
                        let _ = write!(out, "\\x{byte:02x}");
                    }
                }
            }
            out.push('"');
        }
    }
}

impl Code {
    /// Writes `operand` as the disassembler shows it.
    fn write_operand(&self, out: &mut String, operand: Operand) {
        let index = operand.index() as usize;
        match operand.kind() {
            OperandKind::Unused => {}
            OperandKind::Slot => match self.slots.get(index) {
                Some(name) => {
                    out.push('$');
                    out.push_str(&String::from_utf8_lossy(name));
                }
                None => {
                    let _ = write!(out, "$#{index}");
                }
            },
            OperandKind::Temp => {
                let _ = write!(out, "T{index}");
            }
            OperandKind::Literal => match self.literals.get(index) {
                Some(value) => write_literal(out, value),
                None => {
                    let _ = write!(out, "L#{index}");
                }
            },
            OperandKind::Target => {
                let _ = write!(out, "@{index}");
            }
        }
    }

    /// The disassembly of instruction `index`, without its position and line.
    fn instruction_text(&self, instruction: &Instruction) -> String {
        let mut out = String::from(instruction.opcode.name());
        if instruction.flags != 0 {
            let _ = write!(out, "/{}", instruction.flags);
        }
        let operands: Vec<String> = [instruction.a, instruction.b, instruction.c]
            .into_iter()
            .filter(|operand| !operand.is_unused())
            .map(|operand| {
                let mut text = String::new();
                self.write_operand(&mut text, operand);
                text
            })
            .collect();
        if !instruction.result.is_unused() {
            out.push(' ');
            self.write_operand(&mut out, instruction.result);
            out.push_str(" =");
        }
        if !operands.is_empty() {
            out.push(' ');
            out.push_str(&operands.join(", "));
        }
        out
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = if self.name.is_empty() {
            "{main}".into()
        } else {
            String::from_utf8_lossy(&self.name)
        };
        write!(f, "code {name}")?;
        if self.by_ref {
            f.write_str(" &")?;
        }
        if !self.params.is_empty() {
            let params: Vec<String> = self
                .params
                .iter()
                .map(|param| {
                    let mut text = String::new();
                    if param.by_ref {
                        text.push('&');
                    }
                    if param.variadic {
                        text.push_str("...");
                    }
                    self.write_operand(
                        &mut text,
                        Operand::slot(param.slot).unwrap_or(Operand::UNUSED),
                    );
                    text
                })
                .collect();
            write!(f, "({})", params.join(", "))?;
        }
        writeln!(f, " temporaries={}", self.temporaries)?;
        for (index, instruction) in self.instructions.iter().enumerate() {
            let line = self.lines.get(index).copied().unwrap_or(0);
            writeln!(
                f,
                "{index:>4} {line:>4}  {}",
                self.instruction_text(instruction)
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Code, FunctionDecl, Parameter, Unit};
    use crate::bytecode::{Instruction, Literal, Opcode, Operand};

    fn operand(operand: Option<Operand>) -> Operand {
        operand.unwrap()
    }

    #[test]
    fn code_disassembles_with_lines_operands_and_literals() {
        let mut add = Instruction::new(Opcode::Add);
        add.a = operand(Operand::slot(0));
        add.b = operand(Operand::literal(0));
        add.result = operand(Operand::temp(0));
        let mut echo = Instruction::new(Opcode::Echo);
        echo.a = operand(Operand::temp(0));
        let mut jump = Instruction::new(Opcode::JumpIfFalse);
        jump.flags = 2;
        jump.a = operand(Operand::literal(1));
        jump.b = operand(Operand::target(0));
        let mut dangling = Instruction::new(Opcode::Copy);
        dangling.a = operand(Operand::slot(9));
        dangling.b = operand(Operand::literal(9));
        dangling.result = operand(Operand::temp(1));
        let code = Code {
            instructions: vec![add, echo, jump, dangling, Instruction::new(Opcode::Return)],
            lines: vec![1, 1, 2, 3],
            literals: vec![Literal::Int(1), Literal::Bool(false)],
            slots: vec![b"a".to_vec().into_boxed_slice()],
            temporaries: 2,
            ..Code::default()
        };
        assert_eq!(
            code.to_string(),
            "code {main} temporaries=2\n   0    1  Add T0 = $a, 1\n   1    1  Echo T0\n   2    \
             2  JumpIfFalse/2 false, @0\n   3    3  Copy T1 = $#9, L#9\n   4    0  Return\n"
        );
    }

    #[test]
    fn literals_show_their_type() {
        let literals = vec![
            Literal::Null,
            Literal::Bool(true),
            Literal::Float(1.0),
            Literal::Float(0.5),
            Literal::Float(1e100),
            Literal::Float(f64::NAN),
            Literal::from(&b"a\"\\\n\x01~"[..]),
        ];
        let instructions = (0..literals.len())
            .map(|index| {
                let mut echo = Instruction::new(Opcode::Echo);
                echo.a = operand(Operand::literal(u32::try_from(index).unwrap()));
                echo
            })
            .collect();
        let code = Code {
            instructions,
            literals,
            ..Code::default()
        };
        let shown: Vec<String> = code
            .to_string()
            .lines()
            .skip(1)
            .map(|line| line[11..].to_owned())
            .collect();
        assert_eq!(
            shown,
            [
                "Echo null",
                "Echo true",
                "Echo 1.0",
                "Echo 0.5",
                "Echo 1.0E+100",
                "Echo NAN",
                "Echo \"a\\\"\\\\\\n\\x01~\"",
            ]
        );
    }

    #[test]
    fn units_list_their_functions_after_the_main_code() {
        let function = Code {
            name: b"f".to_vec().into_boxed_slice(),
            slots: vec![
                b"a".to_vec().into_boxed_slice(),
                b"b".to_vec().into_boxed_slice(),
            ],
            params: vec![
                Parameter {
                    slot: 0,
                    by_ref: true,
                    variadic: false,
                },
                Parameter {
                    slot: 1,
                    by_ref: false,
                    variadic: true,
                },
            ],
            by_ref: true,
            ..Code::default()
        };
        let unit = Unit {
            main: Code::default(),
            functions: vec![FunctionDecl {
                name: b"f".to_vec().into_boxed_slice(),
                line: 1,
                code: function,
            }],
        };
        assert_eq!(
            unit.to_string(),
            "code {main} temporaries=0\n\ncode f &(&$a, ...$b) temporaries=0\n"
        );
    }
}
