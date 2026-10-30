//! Rewrites same-unit calls of tiny integer leaves into the arithmetic they return.

use crate::bytecode::{Code, Literal, Opcode, Operand, OperandKind, Unit};

/// A same-unit function whose body is `return $param + <int>`.
#[derive(Debug, Clone, Copy)]
struct Leaf {
    addend: i64,
}

/// Replaces qualifying `Invoke` instructions in every code object of `unit`.
pub(super) fn rewrite(unit: &mut Unit) {
    let leaves: Vec<(Box<[u8]>, Leaf)> = unit
        .functions
        .iter()
        .filter_map(|function| leaf(&function.code).map(|kind| (function.name.clone(), kind)))
        .collect();
    if leaves.is_empty() {
        return;
    }
    rewrite_code(&mut unit.main, &leaves);
    for function in &mut unit.functions {
        rewrite_code(&mut function.code, &leaves);
    }
}

fn leaf(code: &Code) -> Option<Leaf> {
    if code.is_generator || code.by_ref || !code.handlers.is_empty() || !code.finallys.is_empty() {
        return None;
    }
    if code.params.len() != 1 || code.params[0].by_ref || code.params[0].variadic {
        return None;
    }
    let param = Operand::slot(code.params[0].slot)?;
    let first = code.instructions.first()?;
    if first.opcode != Opcode::Add || first.a != param {
        return None;
    }
    if first.b.kind() != OperandKind::Literal {
        return None;
    }
    let Literal::Int(addend) = *code.literals.get(first.b.index() as usize)? else {
        return None;
    };
    let second = code.instructions.get(1)?;
    if second.opcode != Opcode::Return || second.a != first.result {
        return None;
    }
    match code.instructions.get(2) {
        None => {}
        Some(trailing) if trailing.opcode == Opcode::Return && trailing.a.is_unused() => {}
        Some(_) => return None,
    }
    if code.instructions.len() > 3 {
        return None;
    }
    Some(Leaf { addend })
}

fn rewrite_code(code: &mut Code, leaves: &[(Box<[u8]>, Leaf)]) {
    let mut extras = Vec::new();
    for ins in &mut code.instructions {
        if ins.opcode != Opcode::Invoke {
            continue;
        }
        let argc = ins.flags & 3;
        if argc != 1 || ins.a.kind() != OperandKind::Literal {
            continue;
        }
        let Some(Literal::String(name)) = code.literals.get(ins.a.index() as usize) else {
            continue;
        };
        let Some(leaf) = leaves
            .iter()
            .find(|(declared, _)| declared.eq_ignore_ascii_case(name))
            .map(|(_, leaf)| *leaf)
        else {
            continue;
        };
        let Some(addend) = intern_int(&code.literals, &mut extras, leaf.addend) else {
            continue;
        };
        ins.opcode = Opcode::Add;
        ins.flags = 0;
        ins.a = ins.b;
        ins.b = addend;
        ins.c = Operand::UNUSED;
    }
    code.literals.extend(extras);
}

fn intern_int(literals: &[Literal], extras: &mut Vec<Literal>, value: i64) -> Option<Operand> {
    if let Some(index) = literals
        .iter()
        .position(|literal| matches!(literal, Literal::Int(n) if *n == value))
    {
        return Operand::literal(u32::try_from(index).ok()?);
    }
    if let Some(index) = extras
        .iter()
        .position(|literal| matches!(literal, Literal::Int(n) if *n == value))
    {
        let at = literals.len().checked_add(index)?;
        return Operand::literal(u32::try_from(at).ok()?);
    }
    let at = literals.len().checked_add(extras.len())?;
    let operand = Operand::literal(u32::try_from(at).ok()?)?;
    extras.push(Literal::Int(value));
    Some(operand)
}

#[cfg(test)]
mod tests {
    use super::{Leaf, intern_int, leaf};
    use crate::bytecode::{Code, Instruction, Literal, Opcode, Operand, Parameter};

    fn operand(operand: Option<Operand>) -> Operand {
        operand.expect("operand")
    }

    #[test]
    fn intern_int_reuses_and_appends() {
        let literals = vec![Literal::Int(1)];
        let mut extras = Vec::new();
        assert_eq!(intern_int(&literals, &mut extras, 1), Operand::literal(0));
        assert!(extras.is_empty());
        assert_eq!(intern_int(&literals, &mut extras, 2), Operand::literal(1));
        assert_eq!(extras, vec![Literal::Int(2)]);
        assert_eq!(intern_int(&literals, &mut extras, 2), Operand::literal(1));
        assert_eq!(extras.len(), 1);
    }

    #[test]
    fn leaf_matches_add_of_one() {
        let mut add = Instruction::new(Opcode::Add);
        add.a = operand(Operand::slot(0));
        add.b = operand(Operand::literal(0));
        add.result = operand(Operand::temp(0));
        let mut ret = Instruction::new(Opcode::Return);
        ret.a = operand(Operand::temp(0));
        let code = Code {
            instructions: vec![add, ret, Instruction::new(Opcode::Return)],
            literals: vec![Literal::Int(1)],
            slots: vec![b"n".to_vec().into_boxed_slice()],
            temporaries: 1,
            params: vec![Parameter {
                slot: 0,
                by_ref: false,
                variadic: false,
                line: 1,
            }],
            ..Code::default()
        };
        assert_eq!(leaf(&code).map(|Leaf { addend }| addend), Some(1));
    }

    #[test]
    fn by_ref_or_side_effect_is_not_a_leaf() {
        let mut add = Instruction::new(Opcode::Add);
        add.a = operand(Operand::slot(0));
        add.b = operand(Operand::literal(0));
        add.result = operand(Operand::temp(0));
        let mut echo = Instruction::new(Opcode::Echo);
        echo.a = operand(Operand::slot(0));
        let code = Code {
            instructions: vec![echo, add],
            literals: vec![Literal::Int(1)],
            params: vec![Parameter {
                slot: 0,
                by_ref: true,
                variadic: false,
                line: 1,
            }],
            ..Code::default()
        };
        assert!(leaf(&code).is_none());
    }
}
