//! Bytecode the compiler never produces stops the VM with an error instead of a guess.

use php_compiler::bytecode::{Code, FunctionDecl, Instruction, Literal, Opcode, Operand, Unit};
use php_vm::{Output, VmError, run};

fn op(
    opcode: Opcode,
    a: Option<Operand>,
    b: Option<Operand>,
    result: Option<Operand>,
) -> Instruction {
    let mut instruction = Instruction::new(opcode);
    instruction.a = a.unwrap_or(Operand::UNUSED);
    instruction.b = b.unwrap_or(Operand::UNUSED);
    instruction.result = result.unwrap_or(Operand::UNUSED);
    instruction
}

fn main_code(instructions: Vec<Instruction>, literals: Vec<Literal>) -> Unit {
    let lines = vec![1; instructions.len()];
    Unit {
        main: Code {
            instructions,
            lines,
            literals,
            slots: vec![b"a".to_vec().into_boxed_slice()],
            temporaries: 1,
            ..Code::default()
        },
        functions: Vec::new(),
        classes: Vec::new(),
    }
}

fn failure(unit: &Unit) -> Result<VmError, String> {
    let mut output = Vec::new();
    match run(unit, b"/t.php", &mut output) {
        Err(error) => Ok(error),
        Ok(status) => Err(format!("ran with status {status}: {output:?}")),
    }
}

fn malformed_cases() -> Vec<(Vec<Instruction>, &'static str)> {
    let slot = |index| Operand::slot(index);
    let temp = |index| Operand::temp(index);
    let literal = |index| Operand::literal(index);
    vec![
        (vec![], "ran past the last instruction"),
        (vec![op(Opcode::Echo, slot(5), None, None)], "no such slot"),
        (
            vec![op(Opcode::Echo, temp(5), None, None)],
            "no such temporary",
        ),
        (
            vec![op(Opcode::Echo, literal(5), None, None)],
            "no such literal",
        ),
        (
            vec![op(Opcode::Echo, None, None, None)],
            "operand holds no value",
        ),
        (
            vec![op(Opcode::Jump, literal(0), None, None)],
            "jump without a target",
        ),
        (
            vec![op(Opcode::Copy, literal(0), None, literal(0))],
            "operand cannot be written",
        ),
        (
            vec![op(Opcode::Copy, literal(0), None, temp(5))],
            "no such temporary",
        ),
        (
            vec![op(Opcode::Copy, literal(0), None, slot(5))],
            "no such slot",
        ),
        (
            vec![op(Opcode::Send, literal(0), None, None)],
            "argument sent outside a call",
        ),
        (
            vec![op(Opcode::SendVar, slot(0), None, None)],
            "argument sent outside a call",
        ),
        (
            vec![op(Opcode::Call, None, None, None)],
            "call without a prepared function",
        ),
        (
            vec![op(Opcode::InitCall, literal(0), None, None)],
            "function name is not a string",
        ),
        (
            vec![op(Opcode::MakeClosure, literal(0), None, None)],
            "closure index out of range",
        ),
        (
            vec![op(Opcode::MakeClosure, literal(1), None, None)],
            "closure index is not an integer",
        ),
        (
            vec![op(Opcode::Capture, literal(0), None, None)],
            "capture target is not a closure",
        ),
        (
            vec![{
                let mut init = op(Opcode::InitCall, literal(1), None, None);
                init.c = Operand::literal(0).unwrap_or(Operand::UNUSED);
                init
            }],
            "function name is not a string",
        ),
        (
            vec![op(Opcode::New, literal(0), None, temp(0))],
            "class name is not a string",
        ),
        (
            vec![op(Opcode::LoadProp, slot(0), literal(0), temp(0))],
            "property name is not a string",
        ),
        (
            vec![op(Opcode::StoreProp, slot(0), literal(0), None)],
            "property name is not a string",
        ),
        (
            vec![{
                let mut init = op(Opcode::InitMethodCall, slot(0), literal(0), None);
                init.c = Operand::literal(1).unwrap_or(Operand::UNUSED);
                init
            }],
            "method name is not a string",
        ),
        (
            vec![op(Opcode::Throw, None, None, None)],
            "operand holds no value",
        ),
    ]
}

#[test]
fn malformed_operands_are_errors() -> Result<(), String> {
    for (instructions, reason) in malformed_cases() {
        let unit = main_code(
            instructions,
            vec![Literal::Int(1), Literal::from(&b"f"[..])],
        );
        assert_eq!(failure(&unit)?, VmError::Malformed(reason), "{reason}");
    }
    Ok(())
}

#[test]
fn by_reference_arguments_need_variables() -> Result<(), String> {
    let mut unit = main_code(
        vec![
            op(Opcode::InitCall, Operand::literal(0), None, None),
            op(Opcode::SendVar, Operand::temp(0), None, None),
        ],
        vec![Literal::from(&b"f"[..])],
    );
    unit.functions.push(FunctionDecl {
        name: b"f".to_vec().into_boxed_slice(),
        line: 1,
        code: Code {
            name: b"f".to_vec().into_boxed_slice(),
            slots: vec![b"x".to_vec().into_boxed_slice()],
            params: vec![php_compiler::bytecode::Parameter {
                slot: 0,
                by_ref: true,
                variadic: false,
                line: 1,
            }],
            ..Code::default()
        },
    });
    assert_eq!(
        failure(&unit)?,
        VmError::Malformed("variable argument is not a slot")
    );
    unit.functions[0].code.params[0].slot = 3;
    unit.main.instructions[1] = op(Opcode::Call, None, None, None);
    assert_eq!(
        failure(&unit)?,
        VmError::Malformed("parameter slot out of range")
    );
    Ok(())
}

#[test]
fn output_sinks_receive_everything_printed() -> Result<(), String> {
    struct Chunks(Vec<Vec<u8>>);
    impl Output for Chunks {
        fn write(&mut self, bytes: &[u8]) {
            self.0.push(bytes.to_vec());
        }
    }
    let unit = main_code(
        vec![
            op(Opcode::Echo, Operand::literal(0), None, None),
            op(Opcode::Echo, Operand::literal(0), None, None),
            op(Opcode::Return, None, None, None),
        ],
        vec![Literal::from(&b"ab"[..])],
    );
    let mut chunks = Chunks(Vec::new());
    let status = run(&unit, b"/t.php", &mut chunks).map_err(|error| error.to_string())?;
    assert_eq!(status, 0);
    assert_eq!(chunks.0, [b"ab".to_vec(), b"ab".to_vec()]);
    Ok(())
}
