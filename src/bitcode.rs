pub type Id = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnsupportedStorageClass(u32),
    UnsupportedBuiltin(u32),
    IdOutOfRange(Id),
    NotABlock(Id),
    NotAType(Id),
    NotAFunction(Id),
    NotAConstant(Id),
    UnsupportedType(Id),
    NotSpirv,
    UnalignedLength,
    Truncated,
    IncompleteInstruction,
    ZeroWordCount,
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
}

impl StorageClass {
    pub(crate) fn from_word(word: u32) -> Result<StorageClass, Error> {
        Ok(match word {
            0 => StorageClass::UniformConstant,
            1 => StorageClass::Input,
            4 => StorageClass::Workgroup,
            5 => StorageClass::CrossWorkgroup,
            6 => StorageClass::Private,
            7 => StorageClass::Function,
            8 => StorageClass::Generic,
            other => return Err(Error::UnsupportedStorageClass(other)),
        })
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
}

impl Builtin {
    pub(crate) fn from_word(word: u32) -> Result<Builtin, Error> {
        Ok(match word {
            24 => Builtin::NumWorkgroups,
            25 => Builtin::WorkgroupSize,
            26 => Builtin::WorkgroupId,
            27 => Builtin::LocalInvocationId,
            28 => Builtin::GlobalInvocationId,
            33 => Builtin::GlobalOffset,
            other => return Err(Error::UnsupportedBuiltin(other)),
        })
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
        component_type: Id,
        count: u32,
    },
    Array {
        element_type: Id,
        count: u64,
    },
    Struct {
        member_types: Vec<Id>,
    },
    Pointer {
        storage: StorageClass,
        pointee_type: Id,
    },
    Function {
        return_type: Id,
        parameter_types: Vec<Id>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decoration {
    SpecId(u32),
    CPacked,
    BuiltIn(Builtin),
    SaturatedConversion,
    FPRoundingMode(RoundingMode),
    Alignment(u32),
    Ignored,
}

#[derive(Debug, Clone)]
pub enum ConstantKind {
    Scalar { bits: u64 },
    True,
    False,
    Composite { members: Vec<Id> },
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
        members: Vec<Id>,
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
    MemoryBarrier {
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
    pub fn block(&self, label: Id) -> Result<&Block, Error> {
        let index = match self.block_of_label.get(label as usize) {
            Some(slot) => slot.ok_or(Error::NotABlock(label))?,
            None => return Err(Error::IdOutOfRange(label)),
        };

        self.blocks.get(index).ok_or(Error::NotABlock(label))
    }

    pub fn entry_label(&self) -> Result<Id, Error> {
        self.blocks
            .first()
            .map(|block| block.label)
            .ok_or(Error::NotAFunction(self.result))
    }
}

fn slot_of<T>(table: &[Option<T>], id: Id) -> Result<Option<&T>, Error> {
    match table.get(id as usize) {
        Some(slot) => Ok(slot.as_ref()),
        None => Err(Error::IdOutOfRange(id)),
    }
}

fn slot_mut<T>(table: &mut [T], id: Id) -> Result<&mut T, Error> {
    table.get_mut(id as usize).ok_or(Error::IdOutOfRange(id))
}

#[derive(Debug, Clone)]
pub struct Module {
    addressing: Addressing,
    bound: usize,
    types: Vec<Option<Type>>,
    layouts: Vec<Option<Layout>>,
    constants: Vec<Option<Constant>>,
    decorations: Vec<Decorations>,
    variables: Vec<Variable>,
    functions: Vec<Function>,
    entry_points: Vec<EntryPoint>,
    function_of_id: Vec<Option<usize>>,
    variable_of_id: Vec<Option<usize>>,
}

impl Module {
    pub fn bound(&self) -> usize {
        self.bound
    }

    pub fn type_(&self, type_id: Id) -> Result<&Type, Error> {
        slot_of(&self.types, type_id)?.ok_or(Error::NotAType(type_id))
    }

    pub fn layout(&self, type_id: Id) -> Result<&Layout, Error> {
        slot_of(&self.layouts, type_id)?.ok_or(Error::NotAType(type_id))
    }

    pub fn constant(&self, id: Id) -> Result<&Constant, Error> {
        slot_of(&self.constants, id)?.ok_or(Error::NotAConstant(id))
    }

    pub fn decorations(&self, id: Id) -> Result<&Decorations, Error> {
        self.decorations
            .get(id as usize)
            .ok_or(Error::IdOutOfRange(id))
    }

    pub fn variables(&self) -> &[Variable] {
        &self.variables
    }

    pub fn variable(&self, id: Id) -> Result<Option<&Variable>, Error> {
        let index = slot_of(&self.variable_of_id, id)?;
        Ok(index.and_then(|position| self.variables.get(*position)))
    }

    pub fn function(&self, id: Id) -> Result<&Function, Error> {
        let index = slot_of(&self.function_of_id, id)?
            .copied()
            .ok_or(Error::NotAFunction(id))?;

        self.functions.get(index).ok_or(Error::NotAFunction(id))
    }

    pub fn entry(&self, name: &str) -> Result<&Function, Error> {
        for entry in &self.entry_points {
            if entry.name == name {
                return self.function(entry.function);
            }
        }

        Err(Error::NoSuchEntryPoint)
    }

    pub fn scalar_width(&self, type_id: Id) -> Result<u32, Error> {
        match self.type_(type_id)? {
            Type::Int { width } => Ok(*width),
            Type::Float { width } => Ok(*width),
            Type::Bool => Ok(8),
            _ => Err(Error::UnsupportedType(type_id)),
        }
    }

    pub fn component_type(&self, type_id: Id) -> Result<Id, Error> {
        match self.type_(type_id)? {
            Type::Vector { component_type, .. } => Ok(*component_type),
            _ => Ok(type_id),
        }
    }

    pub fn component_count(&self, type_id: Id) -> Result<usize, Error> {
        match self.type_(type_id)? {
            Type::Vector { count, .. } => Ok(*count as usize),
            _ => Ok(1),
        }
    }

    pub fn member_type(&self, type_id: Id, index: usize) -> Result<Id, Error> {
        match self.type_(type_id)? {
            Type::Struct { member_types } => member_types
                .get(index)
                .copied()
                .ok_or(Error::NotAType(type_id)),
            Type::Vector { component_type, .. } => Ok(*component_type),
            Type::Array { element_type, .. } => Ok(*element_type),
            _ => Err(Error::UnsupportedType(type_id)),
        }
    }

    pub fn member_offset(&self, type_id: Id, index: usize) -> Result<usize, Error> {
        let layout = self.layout(type_id)?;

        match self.type_(type_id)? {
            Type::Struct { .. } => layout
                .member_offsets
                .get(index)
                .copied()
                .ok_or(Error::NotAType(type_id)),
            Type::Vector { component_type, .. } => Ok(index * self.layout(*component_type)?.size),
            Type::Array { element_type, .. } => Ok(index * self.layout(*element_type)?.size),
            _ => Err(Error::UnsupportedType(type_id)),
        }
    }

    pub fn pointee_type(&self, type_id: Id) -> Result<Id, Error> {
        match self.type_(type_id)? {
            Type::Pointer { pointee_type, .. } => Ok(*pointee_type),
            _ => Err(Error::UnsupportedType(type_id)),
        }
    }

    pub fn storage_class(&self, type_id: Id) -> Result<StorageClass, Error> {
        match self.type_(type_id)? {
            Type::Pointer { storage, .. } => Ok(*storage),
            _ => Err(Error::UnsupportedType(type_id)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleBuilder {
    module: Module,
}

impl ModuleBuilder {
    pub fn from_bound(bound: usize) -> ModuleBuilder {
        ModuleBuilder {
            module: Module {
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
            },
        }
    }

    pub fn finalize(self) -> Module {
        self.module
    }

    pub fn bound(&self) -> usize {
        self.module.bound
    }

    pub fn addressing(&self) -> Addressing {
        self.module.addressing
    }

    pub fn type_(&self, type_id: Id) -> Result<&Type, Error> {
        self.module.type_(type_id)
    }

    pub fn layout(&self, type_id: Id) -> Result<&Layout, Error> {
        self.module.layout(type_id)
    }

    pub fn constant(&self, id: Id) -> Result<&Constant, Error> {
        self.module.constant(id)
    }

    pub fn decorations(&self, id: Id) -> Result<&Decorations, Error> {
        self.module.decorations(id)
    }

    pub fn member_type(&self, type_id: Id, index: usize) -> Result<Id, Error> {
        self.module.member_type(type_id, index)
    }

    pub fn set_addressing(&mut self, addressing: Addressing) {
        self.module.addressing = addressing;
    }

    pub fn set_type(&mut self, type_id: Id, value: Type) -> Result<(), Error> {
        *slot_mut(&mut self.module.types, type_id)? = Some(value);

        Ok(())
    }

    pub fn set_layout(&mut self, type_id: Id, layout: Layout) -> Result<&Layout, Error> {
        Ok(slot_mut(&mut self.module.layouts, type_id)?.insert(layout))
    }

    pub fn set_constant(
        &mut self,
        id: Id,
        result_type: Id,
        kind: ConstantKind,
    ) -> Result<(), Error> {
        *slot_mut(&mut self.module.constants, id)? = Some(Constant { result_type, kind });

        Ok(())
    }

    pub fn set_decoration(&mut self, target: Id, decoration: Decoration) -> Result<(), Error> {
        let slot = slot_mut(&mut self.module.decorations, target)?;

        match decoration {
            Decoration::SpecId(id) => slot.specialization_id = Some(id),
            Decoration::CPacked => slot.packed = true,
            Decoration::BuiltIn(builtin) => slot.builtin = Some(builtin),
            Decoration::SaturatedConversion => slot.saturated_conversion = true,
            Decoration::FPRoundingMode(mode) => slot.rounding_mode = Some(mode),
            Decoration::Alignment(alignment) => slot.alignment = Some(alignment),
            Decoration::Ignored => {}
        }

        Ok(())
    }

    pub fn merge_decoration(&mut self, target: Id, source: &Decorations) -> Result<(), Error> {
        let slot = slot_mut(&mut self.module.decorations, target)?;

        slot.builtin = slot.builtin.or(source.builtin);
        slot.specialization_id = slot.specialization_id.or(source.specialization_id);
        slot.alignment = slot.alignment.or(source.alignment);
        slot.rounding_mode = slot.rounding_mode.or(source.rounding_mode);
        slot.packed |= source.packed;
        slot.saturated_conversion |= source.saturated_conversion;

        Ok(())
    }

    pub fn push_entry_point(&mut self, entry_point: EntryPoint) {
        self.module.entry_points.push(entry_point);
    }

    pub fn push_function(&mut self, function: Function) -> Result<(), Error> {
        *slot_mut(&mut self.module.function_of_id, function.result)? =
            Some(self.module.functions.len());
        self.module.functions.push(function);

        Ok(())
    }

    pub fn push_variable(&mut self, variable: Variable) -> Result<(), Error> {
        *slot_mut(&mut self.module.variable_of_id, variable.result)? =
            Some(self.module.variables.len());
        self.module.variables.push(variable);

        Ok(())
    }
}

pub(crate) fn align_up(value: usize, alignment: usize) -> usize {
    match alignment {
        0 => value,
        _ => value.div_ceil(alignment) * alignment,
    }
}
