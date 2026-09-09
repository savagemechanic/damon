//! Tiny verified composite procedures. Instructions are compact data; expanded
//! actions still pass through the ordinary capability, policy, and tool path.
use crate::{
    capability::CapabilityGraph,
    codec::{write_u16, write_u32, write_u64, Reader},
    data::DamonData,
    reason::{Dependency, Plan},
    storage,
    types::{CapabilityId, EntityId},
};
use std::{collections::HashMap, io, io::Write};

const MAX_PROCEDURES: usize = 1024;
const MAX_INSTRUCTIONS: usize = 8192;
const MAX_PROCEDURE_INSTRUCTIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Call = 1,
    Store = 2,
    CompareSuccess = 3,
    JumpIfFalse = 4,
    Jump = 5,
    Return = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub op: Op,
    pub a: u32,
    pub b: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Procedure {
    pub feature: u64,
    pub instruction_start: u32,
    pub instruction_len: u16,
    pub successes: u32,
    pub failures: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ProcedureTable {
    pub procedures: Vec<Procedure>,
    pub instructions: Vec<Instruction>,
    index: HashMap<u64, usize>,
}

impl ProcedureTable {
    pub fn remember_verified(&mut self, feature: u64, plan: &Plan) -> io::Result<()> {
        if plan.actions.len() < 2 || plan.actions.len() > 64 {
            return Ok(());
        }
        if self.index.contains_key(&feature) {
            self.observe(feature, true);
            return Ok(());
        }
        let mut instructions = Vec::new();
        for (step, action) in plan.actions.iter().enumerate() {
            if let Some(dependency) = plan.dependencies.iter().find(|row| row.step == step) {
                if dependency.success_required {
                    instructions.push(Instruction {
                        op: Op::CompareSuccess,
                        a: dependency.previous as u32,
                        b: 0,
                    });
                    instructions.push(Instruction {
                        op: Op::JumpIfFalse,
                        a: 0,
                        b: 0,
                    });
                }
            }
            instructions.push(Instruction {
                op: Op::Call,
                a: action.capability.0,
                b: 0,
            });
            instructions.push(Instruction {
                op: Op::Store,
                a: step as u32,
                b: 0,
            });
        }
        instructions.push(Instruction {
            op: Op::Return,
            a: 0,
            b: 0,
        });
        let end = instructions.len() as u32 - 1;
        for instruction in &mut instructions {
            if instruction.op == Op::JumpIfFalse {
                instruction.a = end;
            }
        }
        validate_instructions(&instructions, None)?;
        if let Some(index) = self.index.get(&feature).copied() {
            self.remove(index);
        }
        while self.procedures.len() >= MAX_PROCEDURES
            || self.instructions.len() + instructions.len() > MAX_INSTRUCTIONS
        {
            let Some(index) = self
                .procedures
                .iter()
                .enumerate()
                .min_by_key(|(_, procedure)| {
                    (
                        procedure.successes.saturating_add(procedure.failures),
                        procedure.feature,
                    )
                })
                .map(|(index, _)| index)
            else {
                return Err(storage::invalid("procedure instruction limit reached"));
            };
            self.remove(index);
        }
        let start = self.instructions.len() as u32;
        self.instructions.extend(instructions);
        self.procedures.push(Procedure {
            feature,
            instruction_start: start,
            instruction_len: (self.instructions.len() as u32 - start) as u16,
            successes: 1,
            failures: 0,
        });
        self.rebuild_index();
        Ok(())
    }

    pub fn plan(
        &self,
        feature: u64,
        target: EntityId,
        data: &DamonData,
    ) -> Result<Option<Plan>, String> {
        let Some(index) = self.index.get(&feature).copied() else {
            return Ok(None);
        };
        let instructions = self.instructions_for(index);
        validate_instructions(instructions, Some(&data.capabilities)).map_err(|e| e.to_string())?;
        let mut actions = Vec::new();
        let mut dependencies = Vec::new();
        let mut pending_condition = None;
        for instruction in instructions {
            match instruction.op {
                Op::CompareSuccess => pending_condition = Some(instruction.a as usize),
                Op::Call => {
                    let step = actions.len();
                    actions.push(crate::tools::action_for_capability(
                        CapabilityId(instruction.a),
                        target,
                        data,
                    )?);
                    if let Some(previous) = pending_condition.take() {
                        if previous >= step {
                            return Err(
                                "procedure condition references an unavailable result".into()
                            );
                        }
                        dependencies.push(Dependency {
                            step,
                            previous,
                            success_required: true,
                        });
                    }
                }
                Op::Return => break,
                Op::Store | Op::JumpIfFalse | Op::Jump => {}
            }
        }
        Ok(Some(Plan {
            actions,
            dependencies,
        }))
    }

    pub fn observe(&mut self, feature: u64, success: bool) {
        if let Some(index) = self.index.get(&feature).copied() {
            let row = &mut self.procedures[index];
            if success {
                row.successes = row.successes.saturating_add(1);
            } else {
                row.failures = row.failures.saturating_add(1);
            }
        }
    }

    fn instructions_for(&self, index: usize) -> &[Instruction] {
        let row = &self.procedures[index];
        let start = row.instruction_start as usize;
        &self.instructions[start..start + row.instruction_len as usize]
    }

    fn remove(&mut self, index: usize) {
        let mut procedures = Vec::with_capacity(self.procedures.len().saturating_sub(1));
        let mut instructions = Vec::new();
        for old_index in 0..self.procedures.len() {
            if old_index == index {
                continue;
            }
            let mut row = self.procedures[old_index].clone();
            row.instruction_start = instructions.len() as u32;
            instructions.extend_from_slice(self.instructions_for(old_index));
            procedures.push(row);
        }
        self.procedures = procedures;
        self.instructions = instructions;
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (index, row) in self.procedures.iter().enumerate() {
            self.index.insert(row.feature, index);
        }
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        write_u32(writer, self.instructions.len() as u32)?;
        for instruction in &self.instructions {
            writer.write_all(&[instruction.op as u8])?;
            write_u32(writer, instruction.a)?;
            write_u32(writer, instruction.b)?;
        }
        write_u32(writer, self.procedures.len() as u32)?;
        let mut rows = self.procedures.iter().collect::<Vec<_>>();
        rows.sort_by_key(|row| row.feature);
        for row in rows {
            write_u64(writer, row.feature)?;
            write_u32(writer, row.instruction_start)?;
            write_u16(writer, row.instruction_len)?;
            write_u32(writer, row.successes)?;
            write_u32(writer, row.failures)?;
        }
        Ok(())
    }

    pub(crate) fn decode(
        reader: &mut Reader<'_>,
        capabilities: &CapabilityGraph,
    ) -> io::Result<Self> {
        let instruction_count = reader.u32()? as usize;
        if instruction_count > MAX_INSTRUCTIONS {
            return Err(storage::invalid("too many procedure instructions"));
        }
        let mut instructions = Vec::with_capacity(instruction_count);
        for _ in 0..instruction_count {
            instructions.push(Instruction {
                op: parse_op(reader.u8()?)?,
                a: reader.u32()?,
                b: reader.u32()?,
            });
        }
        let count = reader.u32()? as usize;
        if count > MAX_PROCEDURES {
            return Err(storage::invalid("too many procedures"));
        }
        let mut table = Self {
            procedures: Vec::with_capacity(count),
            instructions,
            index: HashMap::new(),
        };
        for _ in 0..count {
            let row = Procedure {
                feature: reader.u64()?,
                instruction_start: reader.u32()?,
                instruction_len: reader.u16()?,
                successes: reader.u32()?,
                failures: reader.u32()?,
            };
            let start = row.instruction_start as usize;
            if row.instruction_len == 0
                || row.instruction_len as usize > MAX_PROCEDURE_INSTRUCTIONS
                || start > table.instructions.len()
                || row.instruction_len as usize > table.instructions.len() - start
            {
                return Err(storage::invalid("invalid procedure instruction slice"));
            }
            validate_instructions(
                &table.instructions[start..start + row.instruction_len as usize],
                Some(capabilities),
            )?;
            table.procedures.push(row);
        }
        table.rebuild_index();
        if table.index.len() != table.procedures.len() {
            return Err(storage::invalid("duplicate procedure feature"));
        }
        Ok(table)
    }
}

fn validate_instructions(
    instructions: &[Instruction],
    capabilities: Option<&CapabilityGraph>,
) -> io::Result<()> {
    if instructions.is_empty()
        || instructions.len() > MAX_PROCEDURE_INSTRUCTIONS
        || instructions.last().is_none_or(|row| row.op != Op::Return)
    {
        return Err(storage::invalid("procedure must end with a bounded return"));
    }
    for instruction in instructions {
        match instruction.op {
            Op::Call => {
                if instruction.a == 0
                    || capabilities.is_some_and(|graph| {
                        graph.capability(CapabilityId(instruction.a)).is_none()
                    })
                {
                    return Err(storage::invalid("procedure calls an unknown capability"));
                }
            }
            Op::Store | Op::CompareSuccess if instruction.a >= 64 => {
                return Err(storage::invalid("procedure result slot exceeds 63"));
            }
            Op::JumpIfFalse | Op::Jump if instruction.a as usize >= instructions.len() => {
                return Err(storage::invalid("procedure jump is out of bounds"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_op(value: u8) -> io::Result<Op> {
    Ok(match value {
        1 => Op::Call,
        2 => Op::Store,
        3 => Op::CompareSuccess,
        4 => Op::JumpIfFalse,
        5 => Op::Jump,
        6 => Op::Return,
        _ => return Err(storage::invalid("unknown procedure opcode")),
    })
}
