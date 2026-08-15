pub type Id = u32;

const MAGIC: u32 = 0x0723_0203;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NotSpirv,
    UnalignedLength,
    Truncated,
    ZeroWordCount,
    IdOutOfRange(Id),
    NotAType(Id),
    NotAFunction(Id),
    NotABlock(Id),
    UnsupportedType(Id),
    NoSuchEntryPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressing {
    Logical,
    Physical32,
    Physical64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageClass {
    UniformConstant,
    Input,
    Workgroup,
    CrossWorkgroup,
    Private,
    Function,
    Generic,
    Other(u32),
}

impl StorageClass {
    fn from_word(word: u32) -> StorageClass {
        match word {
            0 => StorageClass::UniformConstant,
            1 => StorageClass::Input,
            4 => StorageClass::Workgroup,
            5 => StorageClass::CrossWorkgroup,
            6 => StorageClass::Private,
            7 => StorageClass::Function,
            8 => StorageClass::Generic,
            other => StorageClass::Other(other), // TODO: do we really need this? Will this ever be encountered in the real world? can we return a result and error here?
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    GlobalInvocationId,
    LocalInvocationId,
    WorkgroupId,
    NumWorkgroups,
    WorkgroupSize,
    GlobalOffset,
    Other(u32),
}

impl Builtin {
    fn from_word(word: u32) -> Builtin {
        match word {
            24 => Builtin::NumWorkgroups,
            25 => Builtin::WorkgroupSize,
            26 => Builtin::WorkgroupId,
            27 => Builtin::LocalInvocationId,
            28 => Builtin::GlobalInvocationId,
            33 => Builtin::GlobalOffset,
            other => Builtin::Other(other), // TODO: do we really need this? Will this ever be encountered in the real world? can we return a result and error here?
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingMode {
    ToNearestEven,
    TowardZero,
    TowardPositive,
    TowardNegative,
}

#[derive(Debug, Clone)]
pub enum Type {
    Void,
    Bool,
    Int {
        width: u32,
    },
    Float {
        width: u32,
    },
    Vector {
        component: Id,
        count: u32,
    },
    Array {
        element: Id,
        count: u64,
    },
    Struct {
        members: Vec<Id>,
    },
    Pointer {
        storage: StorageClass,
        pointee: Id,
    },
    Function {
        return_type: Id,
        parameters: Vec<Id>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub size: usize,
    pub alignment: usize,
    pub member_offsets: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Decorations {
    pub builtin: Option<Builtin>,
    pub specialization_id: Option<u32>,
    pub alignment: Option<u32>,
    pub packed: bool,
    pub saturated_conversion: bool,
    pub rounding_mode: Option<RoundingMode>,
}

#[derive(Debug, Clone)]
pub enum ConstantKind {
    Scalar { bits: u64 },
    True,
    False,
    Composite { constituents: Vec<Id> },
    Null,
}

#[derive(Debug, Clone)]
pub struct Constant {
    pub result_type: Id,
    pub kind: ConstantKind,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub result: Id,
    pub result_type: Id,
    pub storage: StorageClass,
    pub initializer: Option<Id>,
}

#[derive(Debug, Clone)]
pub struct EntryPoint {
    pub function: Id,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    SNegate,
    FNegate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    IAdd,
    ISub,
    IMul,
    UMod,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,
    FMod,
    ShiftLeftLogical,
    ShiftRightArithmetic,
    ULessThan,
    SLessThan,
    INotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertOp {
    FToS,
    FToU,
    SConvert,
    UConvert,
}

#[derive(Debug, Clone)]
pub enum Instruction {
    Nop,
    Undef {
        result: Id,
        result_type: Id,
    },
    Variable {
        result: Id,
        result_type: Id,
        initializer: Option<Id>,
    },
    Load {
        result: Id,
        result_type: Id,
        pointer: Id,
    },
    Store {
        pointer: Id,
        object: Id,
    },
    AccessChain {
        result: Id,
        result_type: Id,
        base: Id,
        indices: Vec<Id>,
        element: bool,
    },
    Unary {
        result: Id,
        result_type: Id,
        operation: UnaryOp,
        operand: Id,
    },
    Binary {
        result: Id,
        result_type: Id,
        operation: BinaryOp,
        lhs: Id,
        rhs: Id,
    },
    Convert {
        result: Id,
        result_type: Id,
        operation: ConvertOp,
        operand: Id,
    },
    Select {
        result: Id,
        result_type: Id,
        condition: Id,
        true_value: Id,
        false_value: Id,
    },
    CopyObject {
        result: Id,
        result_type: Id,
        operand: Id,
    },
    CompositeConstruct {
        result: Id,
        result_type: Id,
        constituents: Vec<Id>,
    },
    CompositeExtract {
        result: Id,
        result_type: Id,
        composite: Id,
        indices: Vec<u32>,
    },
    VectorExtractDynamic {
        result: Id,
        result_type: Id,
        vector: Id,
        index: Id,
    },
    VectorInsertDynamic {
        result: Id,
        result_type: Id,
        vector: Id,
        component: Id,
        index: Id,
    },
    VectorTimesScalar {
        result: Id,
        result_type: Id,
        vector: Id,
        scalar: Id,
    },
    AtomicCounter {
        result: Id,
        result_type: Id,
        pointer: Id,
        increment: bool,
    },
    Phi {
        result: Id,
        result_type: Id,
        pairs: Vec<(Id, Id)>,
    },
    FunctionCall {
        result: Id,
        result_type: Id,
        function: Id,
        arguments: Vec<Id>,
    },
    Branch {
        target: Id,
    },
    BranchConditional {
        condition: Id,
        true_target: Id,
        false_target: Id,
    },
    Switch {
        selector: Id,
        default_target: Id,
        cases: Vec<(u64, Id)>,
    },
    Barrier {
        execution_scope: Id,
        memory_semantics: Id,
    },
    Return,
    ReturnValue {
        value: Id,
    },
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub label: Id,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub result: Id,
    pub result_type: Id,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub result: Id,
    pub return_type: Id,
    pub parameters: Vec<Parameter>,
    pub blocks: Vec<Block>,
    pub block_of_label: Vec<Option<usize>>,
}

impl Function {
    pub fn block_index(&self, label: Id) -> Result<usize, Error> {
        match self.block_of_label.get(label as usize) {
            Some(slot) => slot.ok_or(Error::NotABlock(label)),
            None => Err(Error::IdOutOfRange(label)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub addressing: Addressing,
    pub bound: usize,
    types: Vec<Option<Type>>,
    layouts: Vec<Option<Layout>>,
    constants: Vec<Option<Constant>>,
    decorations: Vec<Decorations>,
    pub variables: Vec<Variable>,
    pub functions: Vec<Function>,
    pub entry_points: Vec<EntryPoint>,
    function_of_id: Vec<Option<usize>>,
    variable_of_id: Vec<Option<usize>>,
}

fn slot_of<T>(table: &[Option<T>], id: Id) -> Result<Option<&T>, Error> {
    match table.get(id as usize) {
        Some(slot) => Ok(slot.as_ref()),
        None => Err(Error::IdOutOfRange(id)),
    }
}

impl Module {
    pub fn type_of(&self, id: Id) -> Result<&Type, Error> {
        slot_of(&self.types, id)?.ok_or(Error::NotAType(id))
    }

    pub fn layout_of(&self, id: Id) -> Result<&Layout, Error> {
        slot_of(&self.layouts, id)?.ok_or(Error::NotAType(id))
    }

    pub fn constant_of(&self, id: Id) -> Result<Option<&Constant>, Error> {
        slot_of(&self.constants, id)
    }

    pub fn decorations_of(&self, id: Id) -> Result<&Decorations, Error> {
        self.decorations
            .get(id as usize)
            .ok_or(Error::IdOutOfRange(id))
    }

    pub fn builtin_of(&self, id: Id) -> Result<Option<Builtin>, Error> {
        Ok(self.decorations_of(id)?.builtin)
    }

    pub fn rounding_mode_of(&self, id: Id) -> Result<Option<RoundingMode>, Error> {
        Ok(self.decorations_of(id)?.rounding_mode)
    }

    pub fn is_saturated_conversion(&self, id: Id) -> Result<bool, Error> {
        Ok(self.decorations_of(id)?.saturated_conversion)
    }

    pub fn is_packed(&self, id: Id) -> Result<bool, Error> {
        Ok(self.decorations_of(id)?.packed)
    }

    pub fn variable_of(&self, id: Id) -> Result<Option<&Variable>, Error> {
        let index = slot_of(&self.variable_of_id, id)?;
        Ok(index.and_then(|position| self.variables.get(*position)))
    }

    pub fn function_index(&self, id: Id) -> Result<usize, Error> {
        slot_of(&self.function_of_id, id)?
            .copied()
            .ok_or(Error::NotAFunction(id))
    }

    pub fn entry_index(&self, name: &str) -> Result<usize, Error> {
        for entry in &self.entry_points {
            if entry.name == name {
                return self.function_index(entry.function);
            }
        }

        Err(Error::NoSuchEntryPoint)
    }

    pub fn scalar_width(&self, id: Id) -> Result<u32, Error> {
        match self.type_of(id)? {
            Type::Int { width } => Ok(*width),
            Type::Float { width } => Ok(*width),
            Type::Bool => Ok(8),
            _ => Err(Error::UnsupportedType(id)),
        }
    }

    pub fn component_type(&self, id: Id) -> Result<Id, Error> {
        match self.type_of(id)? {
            Type::Vector { component, .. } => Ok(*component),
            _ => Ok(id),
        }
    }

    pub fn component_count(&self, id: Id) -> Result<usize, Error> {
        match self.type_of(id)? {
            Type::Vector { count, .. } => Ok(*count as usize),
            _ => Ok(1),
        }
    }

    pub fn member_type(&self, id: Id, index: usize) -> Result<Id, Error> {
        match self.type_of(id)? {
            Type::Struct { members } => members.get(index).copied().ok_or(Error::NotAType(id)),
            Type::Vector { component, .. } => Ok(*component),
            Type::Array { element, .. } => Ok(*element),
            _ => Err(Error::UnsupportedType(id)),
        }
    }

    pub fn member_offset(&self, id: Id, index: usize) -> Result<usize, Error> {
        let layout = self.layout_of(id)?;
        match self.type_of(id)? {
            Type::Struct { .. } => layout
                .member_offsets
                .get(index)
                .copied()
                .ok_or(Error::NotAType(id)),
            Type::Vector { component, .. } => Ok(index * self.layout_of(*component)?.size),
            Type::Array { element, .. } => Ok(index * self.layout_of(*element)?.size),
            _ => Err(Error::UnsupportedType(id)),
        }
    }

    pub fn pointee_of(&self, id: Id) -> Result<Id, Error> {
        match self.type_of(id)? {
            Type::Pointer { pointee, .. } => Ok(*pointee),
            _ => Err(Error::UnsupportedType(id)),
        }
    }

    pub fn storage_of(&self, id: Id) -> Result<StorageClass, Error> {
        match self.type_of(id)? {
            Type::Pointer { storage, .. } => Ok(*storage),
            _ => Err(Error::UnsupportedType(id)),
        }
    }
}

struct Reader<'w> {
    words: &'w [u32],
    position: usize,
}

impl<'w> Reader<'w> {
    fn remaining(&self) -> usize {
        self.words.len() - self.position
    }

    fn word(&mut self) -> Result<u32, Error> {
        let value = *self.words.get(self.position).ok_or(Error::Truncated)?;
        self.position += 1;
        Ok(value)
    }

    fn rest(&mut self) -> Vec<u32> {
        let tail = self.words[self.position..].to_vec();
        self.position = self.words.len();
        tail
    }

    fn string(&mut self) -> String {
        let mut bytes = Vec::new();

        while self.position < self.words.len() {
            let word = self.words[self.position];
            self.position += 1;

            for shift in 0..4 {
                let byte = ((word >> (shift * 8)) & 0xFF) as u8;
                if byte == 0 {
                    return String::from_utf8_lossy(&bytes).into_owned();
                }

                bytes.push(byte);
            }
        }

        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return value;
    }

    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + alignment - remainder
    }
}

pub fn parse_bytes(bytes: &[u8]) -> Result<Module, Error> {
    if bytes.len() % 4 != 0 {
        return Err(Error::UnalignedLength);
    }

    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        words.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    parse(&words)
}

pub fn parse(words: &[u32]) -> Result<Module, Error> {
    if words.len() < 5 || words[0] != MAGIC {
        return Err(Error::NotSpirv);
    }
    let bound = words[3] as usize;

    let mut module = Module {
        addressing: Addressing::Physical64,
        bound,
        types: vec![None; bound],
        layouts: vec![None; bound],
        constants: vec![None; bound],
        decorations: vec![Decorations::default(); bound],
        variables: Vec::new(),
        functions: Vec::new(),
        entry_points: Vec::new(),
        function_of_id: vec![None; bound],
        variable_of_id: vec![None; bound],
    };

    let mut groups: Vec<(Id, Vec<Id>)> = Vec::new();
    let mut pending_function: Option<Function> = None;
    let mut pending_block: Option<Block> = None;
    let mut position = 5;

    while position < words.len() {
        let header = words[position];
        let word_count = (header >> 16) as usize;
        let opcode = (header & 0xFFFF) as u16;
        if word_count == 0 {
            return Err(Error::ZeroWordCount);
        }
        if position + word_count > words.len() {
            return Err(Error::Truncated);
        }
        let mut reader = Reader {
            words: &words[position + 1..position + word_count],
            position: 0,
        };
        position += word_count;

        match opcode {
            14 => {
                let addressing = reader.word()?;
                module.addressing = match addressing {
                    1 => Addressing::Physical32,
                    2 => Addressing::Physical64,
                    _ => Addressing::Logical,
                };
            }
            15 => {
                let _model = reader.word()?;
                let function = reader.word()?;
                let name = reader.string();
                module.entry_points.push(EntryPoint { function, name });
            }
            71 => {
                let target = reader.word()?;
                apply_decoration(&mut module, target, &mut reader)?;
            }
            72 => {
                let target = reader.word()?;
                let _member = reader.word()?;
                apply_decoration(&mut module, target, &mut reader)?;
            }
            73 => {}
            74 => {
                let group = reader.word()?;
                groups.push((group, reader.rest()));
            }
            19 => {
                let result = reader.word()?;
                set_type(&mut module, result, Type::Void)?;
            }
            20 => {
                let result = reader.word()?;
                set_type(&mut module, result, Type::Bool)?;
            }
            21 => {
                let result = reader.word()?;
                let width = reader.word()?;
                let _signedness = reader.word()?;
                set_type(&mut module, result, Type::Int { width })?;
            }
            22 => {
                let result = reader.word()?;
                let width = reader.word()?;
                set_type(&mut module, result, Type::Float { width })?;
            }
            23 => {
                let result = reader.word()?;
                let component = reader.word()?;
                let count = reader.word()?;
                set_type(&mut module, result, Type::Vector { component, count })?;
            }
            28 => {
                let result = reader.word()?;
                let element = reader.word()?;
                let length = reader.word()?;
                let count = match module.constant_of(length)?.map(|entry| &entry.kind) {
                    Some(ConstantKind::Scalar { bits }) => *bits,
                    _ => 0,
                };
                set_type(&mut module, result, Type::Array { element, count })?;
            }
            30 => {
                let result = reader.word()?;
                let members = reader.rest();
                set_type(&mut module, result, Type::Struct { members })?;
            }
            32 => {
                let result = reader.word()?;
                let storage = StorageClass::from_word(reader.word()?);
                let pointee = reader.word()?;
                set_type(&mut module, result, Type::Pointer { storage, pointee })?;
            }
            33 => {
                let result = reader.word()?;
                let return_type = reader.word()?;
                let parameters = reader.rest();
                set_type(
                    &mut module,
                    result,
                    Type::Function {
                        return_type,
                        parameters,
                    },
                )?;
            }
            41 | 48 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                set_constant(&mut module, result, result_type, ConstantKind::True);
            }
            42 | 49 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                set_constant(&mut module, result, result_type, ConstantKind::False);
            }
            43 | 50 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let mut bits: u64 = reader.word()? as u64;
                if reader.remaining() > 0 {
                    bits |= (reader.word()? as u64) << 32;
                }
                set_constant(
                    &mut module,
                    result,
                    result_type,
                    ConstantKind::Scalar { bits },
                );
            }
            44 | 51 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let constituents = reader.rest();
                set_constant(
                    &mut module,
                    result,
                    result_type,
                    ConstantKind::Composite { constituents },
                );
            }
            46 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                set_constant(&mut module, result, result_type, ConstantKind::Null);
            }
            54 => {
                let return_type = reader.word()?;
                let result = reader.word()?;
                let _control = reader.word()?;
                let _function_type = reader.word()?;
                pending_function = Some(Function {
                    result,
                    return_type,
                    parameters: Vec::new(),
                    blocks: Vec::new(),
                    block_of_label: vec![None; bound],
                });
            }
            55 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                if let Some(function) = pending_function.as_mut() {
                    function.parameters.push(Parameter {
                        result,
                        result_type,
                    });
                }
            }
            56 => {
                if let Some(mut function) = pending_function.take() {
                    if let Some(block) = pending_block.take() {
                        function.block_of_label[block.label as usize] = Some(function.blocks.len());
                        function.blocks.push(block);
                    }
                    module.function_of_id[function.result as usize] = Some(module.functions.len());
                    module.functions.push(function);
                }
            }
            248 => {
                let label = reader.word()?;
                if let Some(function) = pending_function.as_mut()
                    && let Some(block) = pending_block.take()
                {
                    function.block_of_label[block.label as usize] = Some(function.blocks.len());
                    function.blocks.push(block);
                }
                pending_block = Some(Block {
                    label,
                    instructions: Vec::new(),
                });
            }
            59 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let storage = StorageClass::from_word(reader.word()?);
                let initializer = if reader.remaining() > 0 {
                    Some(reader.word()?)
                } else {
                    None
                };
                if pending_block.is_some() {
                    push_instruction(
                        &mut pending_block,
                        Instruction::Variable {
                            result,
                            result_type,
                            initializer,
                        },
                    );
                } else {
                    module.variable_of_id[result as usize] = Some(module.variables.len());
                    module.variables.push(Variable {
                        result,
                        result_type,
                        storage,
                        initializer,
                    });
                }
            }
            _ => {
                if pending_block.is_some() {
                    let instruction = decode_body(opcode, &mut reader)?;
                    push_instruction(&mut pending_block, instruction);
                }
            }
        }
    }

    for (group, targets) in groups {
        let source = module.decorations[group as usize].clone();
        for target in targets {
            if (target as usize) < module.decorations.len() {
                module.decorations[target as usize] = source.clone();
            }
        }
    }

    compute_layouts(&mut module)?;
    Ok(module)
}

fn push_instruction(pending_block: &mut Option<Block>, instruction: Instruction) {
    if let Some(block) = pending_block.as_mut() {
        block.instructions.push(instruction);
    }
}

fn set_type(module: &mut Module, id: Id, value: Type) -> Result<(), Error> {
    if (id as usize) >= module.types.len() {
        return Err(Error::IdOutOfRange(id));
    }
    module.types[id as usize] = Some(value);
    Ok(())
}

fn set_constant(module: &mut Module, id: Id, result_type: Id, kind: ConstantKind) {
    if (id as usize) < module.constants.len() {
        module.constants[id as usize] = Some(Constant { result_type, kind });
    }
}

fn apply_decoration(module: &mut Module, target: Id, reader: &mut Reader) -> Result<(), Error> {
    if (target as usize) >= module.decorations.len() {
        return Ok(());
    }
    let decoration = reader.word()?;
    let slot = &mut module.decorations[target as usize];
    match decoration {
        11 => slot.builtin = Some(Builtin::from_word(reader.word()?)),
        1 => slot.specialization_id = Some(reader.word()?),
        44 => slot.alignment = Some(reader.word()?),
        10 => slot.packed = true,
        39 => {
            slot.rounding_mode = Some(match reader.word()? {
                1 => RoundingMode::TowardZero,
                2 => RoundingMode::TowardPositive,
                3 => RoundingMode::TowardNegative,
                _ => RoundingMode::ToNearestEven,
            })
        }
        28 => slot.saturated_conversion = true,
        _ => {}
    }
    Ok(())
}

fn decode_body(opcode: u16, reader: &mut Reader) -> Result<Instruction, Error> {
    Ok(match opcode {
        1 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            Instruction::Undef {
                result,
                result_type,
            }
        }
        61 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pointer = reader.word()?;
            Instruction::Load {
                result,
                result_type,
                pointer,
            }
        }
        62 => {
            let pointer = reader.word()?;
            let object = reader.word()?;
            Instruction::Store { pointer, object }
        }
        65 | 66 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            Instruction::AccessChain {
                result,
                result_type,
                base,
                indices: reader.rest(),
                element: false,
            }
        }
        67 | 70 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            Instruction::AccessChain {
                result,
                result_type,
                base,
                indices: reader.rest(),
                element: true,
            }
        }
        126 | 127 | 200 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            let operation = match opcode {
                126 => UnaryOp::SNegate,
                127 => UnaryOp::FNegate,
                _ => UnaryOp::Not,
            };
            Instruction::Unary {
                result,
                result_type,
                operation,
                operand,
            }
        }
        128 | 129 | 130 | 131 | 132 | 133 | 136 | 137 | 140 | 141 | 171 | 176 | 177 | 195 | 196 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let lhs = reader.word()?;
            let rhs = reader.word()?;
            let operation = match opcode {
                128 => BinaryOp::IAdd,
                129 => BinaryOp::FAdd,
                130 => BinaryOp::ISub,
                131 => BinaryOp::FSub,
                132 => BinaryOp::IMul,
                133 => BinaryOp::FMul,
                136 => BinaryOp::FDiv,
                137 => BinaryOp::UMod,
                140 => BinaryOp::FRem,
                141 => BinaryOp::FMod,
                171 => BinaryOp::INotEqual,
                176 => BinaryOp::ULessThan,
                177 => BinaryOp::SLessThan,
                195 => BinaryOp::ShiftRightArithmetic,
                _ => BinaryOp::ShiftLeftLogical,
            };
            Instruction::Binary {
                result,
                result_type,
                operation,
                lhs,
                rhs,
            }
        }
        109 | 110 | 113 | 114 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            let operation = match opcode {
                109 => ConvertOp::FToU,
                110 => ConvertOp::FToS,
                113 => ConvertOp::UConvert,
                _ => ConvertOp::SConvert,
            };
            Instruction::Convert {
                result,
                result_type,
                operation,
                operand,
            }
        }
        169 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let condition = reader.word()?;
            let true_value = reader.word()?;
            let false_value = reader.word()?;
            Instruction::Select {
                result,
                result_type,
                condition,
                true_value,
                false_value,
            }
        }
        83 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            Instruction::CopyObject {
                result,
                result_type,
                operand,
            }
        }
        80 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            Instruction::CompositeConstruct {
                result,
                result_type,
                constituents: reader.rest(),
            }
        }
        81 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let composite = reader.word()?;
            Instruction::CompositeExtract {
                result,
                result_type,
                composite,
                indices: reader.rest(),
            }
        }
        77 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let index = reader.word()?;
            Instruction::VectorExtractDynamic {
                result,
                result_type,
                vector,
                index,
            }
        }
        78 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let component = reader.word()?;
            let index = reader.word()?;
            Instruction::VectorInsertDynamic {
                result,
                result_type,
                vector,
                component,
                index,
            }
        }
        142 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let scalar = reader.word()?;
            Instruction::VectorTimesScalar {
                result,
                result_type,
                vector,
                scalar,
            }
        }
        232 | 233 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pointer = reader.word()?;
            Instruction::AtomicCounter {
                result,
                result_type,
                pointer,
                increment: opcode == 232,
            }
        }
        245 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operands = reader.rest();
            let mut pairs = Vec::new();
            for pair in operands.chunks_exact(2) {
                pairs.push((pair[0], pair[1]));
            }
            Instruction::Phi {
                result,
                result_type,
                pairs,
            }
        }
        57 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let function = reader.word()?;
            Instruction::FunctionCall {
                result,
                result_type,
                function,
                arguments: reader.rest(),
            }
        }
        249 => Instruction::Branch {
            target: reader.word()?,
        },
        250 => {
            let condition = reader.word()?;
            let true_target = reader.word()?;
            let false_target = reader.word()?;
            Instruction::BranchConditional {
                condition,
                true_target,
                false_target,
            }
        }
        251 => {
            let selector = reader.word()?;
            let default_target = reader.word()?;
            let operands = reader.rest();
            let mut cases = Vec::new();
            for pair in operands.chunks_exact(2) {
                cases.push((pair[0] as u64, pair[1]));
            }
            Instruction::Switch {
                selector,
                default_target,
                cases,
            }
        }
        224 => {
            let execution_scope = reader.word()?;
            let _memory_scope = reader.word()?;
            let memory_semantics = reader.word()?;
            Instruction::Barrier {
                execution_scope,
                memory_semantics,
            }
        }
        253 => Instruction::Return,
        254 => Instruction::ReturnValue {
            value: reader.word()?,
        },
        255 => Instruction::Unreachable,
        _ => Instruction::Nop,
    })
}

fn compute_layouts(module: &mut Module) -> Result<(), Error> {
    for id in 0..module.bound {
        compute_layout(module, id as Id)?;
    }
    Ok(())
}

fn compute_layout(module: &mut Module, id: Id) -> Result<(), Error> {
    if module.layouts[id as usize].is_some() {
        return Ok(());
    }
    let described = match slot_of(&module.types, id)?.cloned() {
        Some(found) => found,
        None => return Ok(()),
    };

    let layout = match described {
        Type::Void => Layout {
            size: 0,
            alignment: 1,
            member_offsets: Vec::new(),
        },
        Type::Bool => Layout {
            size: 1,
            alignment: 1,
            member_offsets: Vec::new(),
        },
        Type::Int { width } | Type::Float { width } => {
            let size = (width as usize).div_ceil(8);
            Layout {
                size,
                alignment: size,
                member_offsets: Vec::new(),
            }
        }
        Type::Pointer { .. } => {
            let size = if module.addressing == Addressing::Physical32 {
                4
            } else {
                8
            };
            Layout {
                size,
                alignment: size,
                member_offsets: Vec::new(),
            }
        }
        Type::Vector { component, count } => {
            compute_layout(module, component)?;
            let element = module.layout_of(component)?.size;
            let lanes = if count == 3 { 4 } else { count as usize };
            Layout {
                size: element * lanes,
                alignment: element * lanes,
                member_offsets: Vec::new(),
            }
        }
        Type::Array { element, count } => {
            compute_layout(module, element)?;
            let entry = module.layout_of(element)?.clone();
            Layout {
                size: entry.size * count as usize,
                alignment: entry.alignment,
                member_offsets: Vec::new(),
            }
        }
        Type::Struct { members } => {
            let packed = module.is_packed(id)?;
            let mut offset = 0;
            let mut alignment = 1;
            let mut member_offsets = Vec::with_capacity(members.len());
            for member in &members {
                compute_layout(module, *member)?;
                let entry = module.layout_of(*member)?.clone();
                let member_alignment = if packed { 1 } else { entry.alignment };
                offset = align_up(offset, member_alignment);
                member_offsets.push(offset);
                offset += entry.size;
                if member_alignment > alignment {
                    alignment = member_alignment;
                }
            }
            Layout {
                size: align_up(offset, alignment),
                alignment,
                member_offsets,
            }
        }
        Type::Function { .. } => Layout {
            size: 0,
            alignment: 1,
            member_offsets: Vec::new(),
        },
    };

    module.layouts[id as usize] = Some(layout);
    Ok(())
}
