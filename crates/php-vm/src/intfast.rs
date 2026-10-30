//! Counted integer loops: a register interpreter for fused `while` bodies.

use php_compiler::bytecode::{Instruction, Opcode, Operand, OperandKind};
use php_value::{PhpArray, Slot, Value};

use crate::machine::{Exec, Machine};
use crate::{Output, VmError};

const REGS: usize = 16;

#[derive(Clone, Copy)]
enum Bin {
    Add,
    Sub,
    Mul,
    Mod,
}

#[derive(Clone, Copy)]
enum FastOp {
    Bin {
        op: Bin,
        dst: u8,
        left: u8,
        right: u8,
        pc: usize,
    },
    Move {
        dst: u8,
        src: u8,
    },
    Inc {
        dst: u8,
        pc: usize,
    },
}

#[derive(Clone, Copy)]
struct Counted {
    ivar: Operand,
    limit: Operand,
    inclusive: bool,
    exit: usize,
    close: usize,
}

struct RegMap {
    ops: [Operand; REGS],
    write: [bool; REGS],
    held: [Option<i64>; REGS],
    used: usize,
}

impl RegMap {
    fn new() -> Self {
        Self {
            ops: [Operand::UNUSED; REGS],
            write: [false; REGS],
            held: [None; REGS],
            used: 0,
        }
    }

    fn intern(&mut self, operand: Operand, write: bool) -> Option<u8> {
        if operand.is_unused() {
            return None;
        }
        if let Some(index) = self.ops[..self.used]
            .iter()
            .position(|held| *held == operand)
        {
            if write {
                self.write[index] = true;
            }
            return u8::try_from(index).ok();
        }
        if self.used >= REGS {
            return None;
        }
        let index = self.used;
        self.ops[index] = operand;
        self.write[index] = write;
        self.used += 1;
        u8::try_from(index).ok()
    }

    fn intern_held(&mut self, operand: Operand, value: i64) -> Option<u8> {
        let index = self.intern(operand, false)?;
        self.held[index as usize] = Some(value);
        Some(index)
    }
}

impl<O: Output> Machine<'_, O> {
    /// Runs a counted integer, array-fill or foreach-sum loop when the bytecode matches.
    pub(crate) fn try_fast_loop(&mut self) -> Exec<bool> {
        if self.try_counted_loop()? {
            return Ok(true);
        }
        self.try_foreach_sum()
    }

    fn try_counted_loop(&mut self) -> Exec<bool> {
        let Some(shape) = self.counted_shape()? else {
            return Ok(false);
        };
        let body = self.body_between(shape)?;
        if let Some(array) = fill_array(&body, shape.ivar) {
            return self.run_array_fill(array, shape);
        }
        let Some(plan) = self.plan_int_body(&body, shape)? else {
            return Ok(false);
        };
        self.run_int_loop(&plan, shape)
    }

    fn counted_shape(&self) -> Exec<Option<Counted>> {
        let frame = self.frame()?;
        let code = &self.codes[frame.code].code;
        let pc = frame.pc;
        let Some(head) = code.instructions.get(pc).copied() else {
            return Ok(None);
        };
        let inclusive = match head.opcode {
            Opcode::JumpIfNotLess => false,
            Opcode::JumpIfNotLessEqual => true,
            _ => return Ok(None),
        };
        if head.c.kind() != OperandKind::Target {
            return Ok(None);
        }
        let exit = head.c.index() as usize;
        let ivar = head.a;
        let Some(close) =
            code.instructions
                .iter()
                .enumerate()
                .skip(pc + 1)
                .find_map(|(index, ins)| {
                    (ins.opcode == Opcode::IncJump
                        && ins.a == ivar
                        && ins.b.kind() == OperandKind::Target
                        && ins.b.index() as usize == pc)
                        .then_some(index)
                })
        else {
            return Ok(None);
        };
        Ok(Some(Counted {
            ivar,
            limit: head.b,
            inclusive,
            exit,
            close,
        }))
    }

    fn body_between(&self, shape: Counted) -> Exec<Vec<(usize, Instruction)>> {
        let frame = self.frame()?;
        let code = &self.codes[frame.code].code;
        let start = frame.pc + 1;
        Ok(code
            .instructions
            .iter()
            .enumerate()
            .skip(start)
            .take(shape.close.saturating_sub(start))
            .map(|(pc, ins)| (pc, *ins))
            .collect())
    }

    fn plan_int_body(
        &self,
        body: &[(usize, Instruction)],
        shape: Counted,
    ) -> Exec<Option<IntPlan>> {
        let writes = written_operands(body, shape.ivar);
        let mut consts: Vec<(Operand, i64)> = Vec::new();
        let mut kept = Vec::new();
        let mut skip_until = None;
        for &(pc, ins) in body {
            if skip_until.is_some_and(|until| pc < until) {
                continue;
            }
            skip_until = None;
            if let Some(until) = self.fold_jump(&ins, &consts, &writes) {
                skip_until = Some(until);
                continue;
            }
            if let Some(value) = self.hoist_const(&ins, &writes, &consts)? {
                if !ins.result.is_unused() {
                    consts.push((ins.result, value));
                }
                continue;
            }
            if ins.opcode == Opcode::Nop {
                continue;
            }
            if is_int_op(ins.opcode) {
                kept.push((pc, ins));
                continue;
            }
            return Ok(None);
        }
        let mut map = RegMap::new();
        let ivar = map.intern(shape.ivar, true);
        let limit = map.intern(shape.limit, false);
        let (Some(ivar), Some(limit)) = (ivar, limit) else {
            return Ok(None);
        };
        for &(operand, value) in &consts {
            if map.intern_held(operand, value).is_none() {
                return Ok(None);
            }
        }
        let mut ops = Vec::with_capacity(kept.len());
        for &(pc, ins) in &kept {
            let Some(op) = encode_fast(&mut map, pc, ins, &consts) else {
                return Ok(None);
            };
            ops.push(op);
        }
        Ok(Some(IntPlan {
            map,
            ops,
            ivar,
            limit,
        }))
    }

    fn fold_jump(
        &self,
        ins: &Instruction,
        consts: &[(Operand, i64)],
        writes: &[Operand],
    ) -> Option<usize> {
        match ins.opcode {
            Opcode::Jump if ins.a.kind() == OperandKind::Target => Some(ins.a.index() as usize),
            Opcode::JumpIfFalse | Opcode::JumpIfTrue => {
                let value = const_of(ins.a, consts).or(self.peek_int(ins.a).ok().flatten())?;
                if writes.contains(&ins.a) && const_of(ins.a, consts).is_none() {
                    return None;
                }
                let take = if ins.opcode == Opcode::JumpIfFalse {
                    value == 0
                } else {
                    value != 0
                };
                if take && ins.b.kind() == OperandKind::Target {
                    Some(ins.b.index() as usize)
                } else {
                    Some(0)
                }
            }
            _ => None,
        }
    }

    fn hoist_const(
        &self,
        ins: &Instruction,
        writes: &[Operand],
        consts: &[(Operand, i64)],
    ) -> Exec<Option<i64>> {
        if !is_hoistable(ins.opcode) {
            return Ok(None);
        }
        let reads = read_operands(ins);
        if reads
            .iter()
            .any(|read| writes.contains(read) && const_of(*read, consts).is_none())
        {
            return Ok(None);
        }
        Ok(match ins.opcode {
            Opcode::Strlen => self.bytes_of(ins.a).map(|text| len_i64(text.len())),
            Opcode::Strpos => match (self.bytes_of(ins.a), self.bytes_of(ins.b)) {
                (Some(hay), Some(needle)) => {
                    Some(crate::strings::strpos(hay, needle, 0).unwrap_or(0))
                }
                _ => None,
            },
            Opcode::StrContains => match (self.bytes_of(ins.a), self.bytes_of(ins.b)) {
                (Some(hay), Some(needle)) => Some(i64::from(crate::strings::contains(hay, needle))),
                _ => None,
            },
            Opcode::Count => match self.operand_value(ins.a)? {
                Some(Value::Array(array)) => Some(len_i64(array.len())),
                _ => None,
            },
            Opcode::Copy => const_of(ins.a, consts).or(self.peek_int(ins.a).ok().flatten()),
            _ => None,
        })
    }

    fn run_int_loop(&mut self, plan: &IntPlan, shape: Counted) -> Exec<bool> {
        let Some(mut regs) = self.load_regs(&plan.map)? else {
            return Ok(false);
        };
        let iv = plan.ivar as usize;
        let lim = plan.limit as usize;
        while if shape.inclusive {
            regs[iv] <= regs[lim]
        } else {
            regs[iv] < regs[lim]
        } {
            for op in &plan.ops {
                match *op {
                    FastOp::Bin {
                        op,
                        dst,
                        left,
                        right,
                        pc,
                    } => match apply(op, regs[left as usize], regs[right as usize]) {
                        Some(value) => regs[dst as usize] = value,
                        None => return self.fail_at(&plan.map, &regs, pc),
                    },
                    FastOp::Move { dst, src } => {
                        regs[dst as usize] = regs[src as usize];
                    }
                    FastOp::Inc { dst, pc } => match regs[dst as usize].checked_add(1) {
                        Some(value) => regs[dst as usize] = value,
                        None => return self.fail_at(&plan.map, &regs, pc),
                    },
                }
            }
            match regs[iv].checked_add(1) {
                Some(value) => regs[iv] = value,
                None => return self.fail_at(&plan.map, &regs, shape.close),
            }
        }
        self.store_regs(&plan.map, &regs)?;
        self.frame_mut()?.pc = shape.exit;
        Ok(true)
    }

    fn run_array_fill(&mut self, array: Operand, shape: Counted) -> Exec<bool> {
        let Some(mut i) = self.peek_int(shape.ivar)? else {
            return Ok(false);
        };
        let Some(limit) = self.peek_int(shape.limit)? else {
            return Ok(false);
        };
        let outcome = self.with_value_mut(array, |container| match container {
            Value::Array(held) => fill_ints(held, &mut i, limit, shape.inclusive),
            Value::Null => {
                let mut held = PhpArray::new();
                fill_ints(&mut held, &mut i, limit, shape.inclusive)?;
                *container = Value::Array(held);
                Ok(())
            }
            _ => Err(FillErr::Miss),
        })?;
        match outcome {
            Ok(()) => {
                self.write(shape.ivar, Value::Int(i))?;
                self.frame_mut()?.pc = shape.exit;
                Ok(true)
            }
            Err(FillErr::Miss) => Ok(false),
            Err(FillErr::Full) => Err(VmError::Unsupported("this many array items").into()),
        }
    }

    fn try_foreach_sum(&mut self) -> Exec<bool> {
        let Some(plan) = self.foreach_sum_shape()? else {
            return Ok(false);
        };
        let Some(mut total) = self.peek_int(plan.total)? else {
            return Ok(false);
        };
        let summed = {
            let Some(Value::Array(array)) = self.operand_value(plan.array)? else {
                return Ok(false);
            };
            let mut last = None;
            for (_, slot) in array.iter() {
                let Slot::Value(Value::Int(number)) = slot else {
                    return Ok(false);
                };
                let Some(next) = total.checked_add(*number) else {
                    return Ok(false);
                };
                total = next;
                last = Some(*number);
            }
            last
        };
        if let Some(last) = summed {
            self.write(plan.value, Value::Int(last))?;
        }
        self.write(plan.total, Value::Int(total))?;
        let cursor = i64::try_from(self.array_len(plan.array)?).unwrap_or(i64::MAX);
        self.write(plan.cursor, Value::Int(cursor))?;
        self.write(plan.has, Value::Bool(false))?;
        self.frame_mut()?.pc = plan.exit;
        Ok(true)
    }

    fn foreach_sum_shape(&self) -> Exec<Option<ForeachSum>> {
        let frame = self.frame()?;
        let code = &self.codes[frame.code].code;
        let pc = frame.pc;
        let Some(ready) = code.instructions.get(pc).copied() else {
            return Ok(None);
        };
        if ready.opcode != Opcode::IterReady {
            return Ok(None);
        }
        let Some(skip) = code.instructions.get(pc + 1).copied() else {
            return Ok(None);
        };
        let Some(pick) = code.instructions.get(pc + 2).copied() else {
            return Ok(None);
        };
        let Some(add) = code.instructions.get(pc + 3).copied() else {
            return Ok(None);
        };
        let Some(step) = code.instructions.get(pc + 4).copied() else {
            return Ok(None);
        };
        let Some(back) = code.instructions.get(pc + 5).copied() else {
            return Ok(None);
        };
        if skip.opcode != Opcode::JumpIfFalse
            || skip.a != ready.result
            || skip.b.kind() != OperandKind::Target
            || pick.opcode != Opcode::IterPick
            || pick.flags != 0
            || pick.a != ready.a
            || pick.b != ready.b
            || add.opcode != Opcode::AddAssign
            || add.b != pick.result
            || back.opcode != Opcode::Jump
            || back.a.kind() != OperandKind::Target
            || back.a.index() as usize != pc
        {
            return Ok(None);
        }
        let step_ok = match step.opcode {
            Opcode::Add if step.a == ready.b && step.result == ready.b => {
                self.peek_int(step.b)?.is_some_and(|n| n == 1)
            }
            Opcode::PreIncrement if step.a == ready.b => true,
            _ => false,
        };
        if !step_ok {
            return Ok(None);
        }
        Ok(Some(ForeachSum {
            array: ready.a,
            cursor: ready.b,
            has: ready.result,
            value: pick.result,
            total: add.a,
            exit: skip.b.index() as usize,
        }))
    }

    fn array_len(&self, operand: Operand) -> Exec<usize> {
        Ok(match self.operand_value(operand)? {
            Some(Value::Array(array)) => array.len(),
            _ => 0,
        })
    }

    fn load_regs(&self, map: &RegMap) -> Exec<Option<[i64; REGS]>> {
        let mut regs = [0i64; REGS];
        for ((reg, held), operand) in regs
            .iter_mut()
            .zip(map.held.iter())
            .zip(map.ops.iter())
            .take(map.used)
        {
            if let Some(value) = *held {
                *reg = value;
                continue;
            }
            let Some(value) = self.peek_int(*operand)? else {
                return Ok(None);
            };
            *reg = value;
        }
        Ok(Some(regs))
    }

    fn store_regs(&mut self, map: &RegMap, regs: &[i64; REGS]) -> Exec {
        for ((&write, &operand), &value) in map
            .write
            .iter()
            .zip(map.ops.iter())
            .zip(regs.iter())
            .take(map.used)
        {
            if write && operand.kind() != OperandKind::Literal {
                self.write(operand, Value::Int(value))?;
            }
        }
        Ok(())
    }

    fn fail_at(&mut self, map: &RegMap, regs: &[i64; REGS], pc: usize) -> Exec<bool> {
        self.store_regs(map, regs)?;
        self.frame_mut()?.pc = pc;
        Ok(false)
    }

    fn bytes_of(&self, operand: Operand) -> Option<&[u8]> {
        match self.operand_value(operand).ok()? {
            Some(Value::String(text)) => Some(text.as_bytes()),
            _ => None,
        }
    }
}

struct IntPlan {
    map: RegMap,
    ops: Vec<FastOp>,
    ivar: u8,
    limit: u8,
}

struct ForeachSum {
    array: Operand,
    cursor: Operand,
    has: Operand,
    value: Operand,
    total: Operand,
    exit: usize,
}

enum FillErr {
    Miss,
    Full,
}

fn fill_ints(
    array: &mut PhpArray,
    i: &mut i64,
    limit: i64,
    inclusive: bool,
) -> Result<(), FillErr> {
    while if inclusive { *i <= limit } else { *i < limit } {
        array
            .append(Value::Int(*i))
            .map_err(|_full| FillErr::Full)?;
        *i = i.checked_add(1).ok_or(FillErr::Miss)?;
    }
    Ok(())
}

fn fill_array(body: &[(usize, Instruction)], ivar: Operand) -> Option<Operand> {
    let active: Vec<Instruction> = body
        .iter()
        .map(|(_, ins)| *ins)
        .filter(|ins| ins.opcode != Opcode::Nop)
        .collect();
    let [ins] = active.as_slice() else {
        return None;
    };
    (ins.opcode == Opcode::StoreDim && ins.b.is_unused() && ins.c == ivar).then_some(ins.a)
}

fn written_operands(body: &[(usize, Instruction)], ivar: Operand) -> Vec<Operand> {
    let mut writes = vec![ivar];
    for (_, ins) in body {
        for dest in dests(ins) {
            if !writes.contains(&dest) {
                writes.push(dest);
            }
        }
    }
    writes
}

fn dests(ins: &Instruction) -> Vec<Operand> {
    match ins.opcode {
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Mod
        | Opcode::Copy
        | Opcode::Strlen
        | Opcode::Strpos
        | Opcode::StrContains
        | Opcode::Count => dest_result(ins),
        Opcode::AddAssign
        | Opcode::SubAssign
        | Opcode::MulAssign
        | Opcode::ModAssign
        | Opcode::Assign
        | Opcode::PreIncrement => {
            let mut dest = vec![ins.a];
            dest.extend(dest_result(ins));
            dest
        }
        _ => Vec::new(),
    }
}

fn dest_result(ins: &Instruction) -> Vec<Operand> {
    if ins.result.is_unused() {
        Vec::new()
    } else {
        vec![ins.result]
    }
}

fn read_operands(ins: &Instruction) -> Vec<Operand> {
    match ins.opcode {
        Opcode::Strlen | Opcode::Count | Opcode::Copy | Opcode::PreIncrement => filter_read(ins.a),
        Opcode::Assign => filter_read(ins.b),
        Opcode::Strpos
        | Opcode::StrContains
        | Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Mod
        | Opcode::AddAssign
        | Opcode::SubAssign
        | Opcode::MulAssign
        | Opcode::ModAssign => join_reads(ins.a, ins.b),
        _ => Vec::new(),
    }
}

fn filter_read(operand: Operand) -> Vec<Operand> {
    if operand.is_unused() || operand.kind() == OperandKind::Literal {
        Vec::new()
    } else {
        vec![operand]
    }
}

fn join_reads(left: Operand, right: Operand) -> Vec<Operand> {
    let mut reads = filter_read(left);
    reads.extend(filter_read(right));
    reads
}

fn is_int_op(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Mod
            | Opcode::AddAssign
            | Opcode::SubAssign
            | Opcode::MulAssign
            | Opcode::ModAssign
            | Opcode::Copy
            | Opcode::Assign
            | Opcode::PreIncrement
    )
}

fn is_hoistable(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::Strlen | Opcode::Strpos | Opcode::StrContains | Opcode::Count | Opcode::Copy
    )
}

fn encode_fast(
    map: &mut RegMap,
    pc: usize,
    ins: Instruction,
    consts: &[(Operand, i64)],
) -> Option<FastOp> {
    match ins.opcode {
        Opcode::Copy => {
            let src = map_const_or_op(map, ins.a, consts, false)?;
            let dst = map.intern(ins.result, true)?;
            Some(FastOp::Move { dst, src })
        }
        Opcode::Assign => {
            if !ins.result.is_unused() {
                return None;
            }
            let src = map_const_or_op(map, ins.b, consts, false)?;
            let dst = map.intern(ins.a, true)?;
            Some(FastOp::Move { dst, src })
        }
        Opcode::PreIncrement => {
            let dst = map.intern(ins.a, true)?;
            Some(FastOp::Inc { dst, pc })
        }
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Mod => {
            let left = map_const_or_op(map, ins.a, consts, false)?;
            let right = map_const_or_op(map, ins.b, consts, false)?;
            let dst = map.intern(ins.result, true)?;
            Some(FastOp::Bin {
                op: bin_of(ins.opcode)?,
                dst,
                left,
                right,
                pc,
            })
        }
        Opcode::AddAssign | Opcode::SubAssign | Opcode::MulAssign | Opcode::ModAssign => {
            if !ins.result.is_unused() {
                return None;
            }
            let left = map.intern(ins.a, true)?;
            let right = map_const_or_op(map, ins.b, consts, false)?;
            Some(FastOp::Bin {
                op: bin_of(ins.opcode)?,
                dst: left,
                left,
                right,
                pc,
            })
        }
        _ => None,
    }
}

fn map_const_or_op(
    map: &mut RegMap,
    operand: Operand,
    consts: &[(Operand, i64)],
    write: bool,
) -> Option<u8> {
    if const_of(operand, consts).is_some() || operand.kind() == OperandKind::Literal {
        return map.intern(operand, write);
    }
    map.intern(operand, write)
}

fn const_of(operand: Operand, consts: &[(Operand, i64)]) -> Option<i64> {
    consts
        .iter()
        .find(|(held, _)| *held == operand)
        .map(|(_, value)| *value)
}

fn bin_of(opcode: Opcode) -> Option<Bin> {
    Some(match opcode {
        Opcode::Add | Opcode::AddAssign => Bin::Add,
        Opcode::Sub | Opcode::SubAssign => Bin::Sub,
        Opcode::Mul | Opcode::MulAssign => Bin::Mul,
        Opcode::Mod | Opcode::ModAssign => Bin::Mod,
        _ => return None,
    })
}

fn apply(op: Bin, left: i64, right: i64) -> Option<i64> {
    match op {
        Bin::Add => left.checked_add(right),
        Bin::Sub => left.checked_sub(right),
        Bin::Mul => left.checked_mul(right),
        Bin::Mod if right != 0 => left.checked_rem(right),
        Bin::Mod => None,
    }
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Bin, apply, dests, fill_array, is_int_op, len_i64};
    use php_compiler::bytecode::{Instruction, Opcode, Operand};

    fn operand(operand: Option<Operand>) -> Operand {
        operand.expect("operand")
    }

    #[test]
    fn apply_covers_checked_integer_ops() {
        assert_eq!(apply(Bin::Add, 2, 3), Some(5));
        assert_eq!(apply(Bin::Sub, 2, 3), Some(-1));
        assert_eq!(apply(Bin::Mul, 4, 5), Some(20));
        assert_eq!(apply(Bin::Mod, 7, 3), Some(1));
        assert_eq!(apply(Bin::Mod, 7, 0), None);
        assert_eq!(apply(Bin::Add, i64::MAX, 1), None);
        assert_eq!(len_i64(3), 3);
    }

    #[test]
    fn fill_array_needs_a_single_append() {
        let mut store = Instruction::new(Opcode::StoreDim);
        store.a = operand(Operand::slot(0));
        store.c = operand(Operand::slot(1));
        assert_eq!(
            fill_array(&[(1, store)], operand(Operand::slot(1))),
            Some(operand(Operand::slot(0)))
        );
        store.b = operand(Operand::slot(2));
        assert!(fill_array(&[(1, store)], operand(Operand::slot(1))).is_none());
        assert!(is_int_op(Opcode::AddAssign));
        assert!(!is_int_op(Opcode::Echo));
        let mut add = Instruction::new(Opcode::AddAssign);
        add.a = operand(Operand::slot(0));
        add.b = operand(Operand::slot(1));
        assert_eq!(dests(&add), vec![operand(Operand::slot(0))]);
    }
}
