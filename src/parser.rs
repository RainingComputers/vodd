use crate::bitcode;

const MAGIC: u32 = 0x0723_0203;
const MEMORY_ALIGNED: u32 = 0x2;
const DEBUG_SOURCE: u32 = 35;
const DEBUG_LINE: u32 = 103;
const DEBUG_NO_LINE: u32 = 104;
const DEBUG_SETS: &[&str] = &[
    "OpenCL.DebugInfo.100",
    "NonSemantic.Shader.DebugInfo.100",
    "NonSemantic.Shader.DebugInfo.200",
    "SPIRV.debug",
];

struct Reader<'w, I: Iterator<Item = u32>> {
    words: &'w mut I,
    remaining: usize,
}

impl<'w, I: Iterator<Item = u32>> Reader<'w, I> {
    fn from_iter(words: &'w mut I, word_count: usize) -> Reader<'w, I> {
        Reader { words, remaining: word_count - 1 }
    }

    fn try_word(&mut self) -> Option<u32> {
        if self.remaining == 0 {
            return None;
        }

        let value = self.words.next()?;
        self.remaining -= 1;

        Some(value)
    }

    fn word(&mut self) -> Result<u32, bitcode::Error> {
        self.try_word().ok_or(bitcode::Error::Truncated)
    }

    fn rest(&mut self) -> Vec<u32> {
        std::iter::from_fn(|| self.try_word()).collect()
    }

    fn string(&mut self) -> String {
        let mut bytes = Vec::new();

        while let Some(word) = self.try_word() {
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

    fn finish(mut self) -> Result<(), bitcode::Error> {
        while self.remaining > 0 {
            self.words
                .next()
                .ok_or(bitcode::Error::IncompleteInstruction)?;
            self.remaining -= 1;
        }

        Ok(())
    }
}

pub fn parse(bytes: &[u8]) -> Result<bitcode::Module, bitcode::Error> {
    if !bytes.len().is_multiple_of(4) {
        return Err(bitcode::Error::UnalignedLength);
    }

    parse_inner(
        bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
    )
}

fn parse_inner(words: impl IntoIterator<Item = u32>) -> Result<bitcode::Module, bitcode::Error> {
    let mut words = words.into_iter();

    let mut header = [0u32; 5];
    for slot in &mut header {
        *slot = words.next().ok_or(bitcode::Error::NotSpirv)?;
    }

    if header[0] != MAGIC {
        return Err(bitcode::Error::NotSpirv);
    }

    let bound = header[3] as usize;

    let mut module = bitcode::ModuleBuilder::from_bound(bound);

    let mut dec_groups: Vec<(bitcode::Id, Vec<bitcode::Id>)> = Vec::new();
    let mut pending_function: Option<bitcode::Function> = None;
    let mut pending_block: Option<bitcode::Block> = None;
    let mut debug_sets: Vec<bitcode::Id> = Vec::new();
    let mut debug_files: Vec<Option<bitcode::Id>> = vec![None; bound];
    let mut current_line: Option<bitcode::Location> = None;

    while let Some(header) = words.next() {
        let word_count = (header >> 16) as usize;
        let opcode = (header & 0xFFFF) as u16;

        if word_count == 0 {
            return Err(bitcode::Error::ZeroWordCount);
        }

        let mut reader = Reader::from_iter(&mut words, word_count);

        match opcode {
            14 => {
                let addressing = reader.word()?;
                module.set_addressing(match addressing {
                    1 => bitcode::Addressing::Physical32,
                    2 => bitcode::Addressing::Physical64,
                    _ => bitcode::Addressing::Logical,
                });
            }
            11 => {
                let result = reader.word()?;
                let name = reader.string();

                if name == "OpenCL.std" {
                    module.set_opencl_std(result);
                } else if DEBUG_SETS.contains(&name.as_str()) {
                    debug_sets.push(result);
                }
            }
            12 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let set = reader.word()?;
                let instruction = reader.word()?;
                let operands = reader.rest();

                if debug_sets.contains(&set) {
                    match instruction {
                        DEBUG_SOURCE => {
                            if let (Some(file), Some(slot)) =
                                (operands.first(), debug_files.get_mut(result as usize))
                            {
                                *slot = Some(*file);
                            }
                        }
                        DEBUG_LINE => {
                            current_line = debug_location(&module, &debug_files, &operands)?;
                        }
                        DEBUG_NO_LINE => current_line = None,
                        _ => {}
                    }
                } else if let Some(block) = pending_block.as_mut() {
                    block.push(
                        bitcode::Instruction::ExtInst {
                            result,
                            result_type,
                            set,
                            instruction,
                            operands,
                        },
                        current_line,
                    );
                }
            }
            7 => {
                let result = reader.word()?;
                module.set_string(result, reader.string())?;
            }
            8 => {
                let file = reader.word()?;
                let line = reader.word()?;
                let column = reader.word()?;
                current_line = Some(bitcode::Location { file, line, column });
            }
            317 => current_line = None,
            15 => {
                let _model = reader.word()?;
                let function = reader.word()?;
                let name = reader.string();
                module.push_entry_point(bitcode::EntryPoint {
                    function,
                    name,
                    required_local_size: None,
                });
            }
            16 => {
                let function = reader.word()?;
                if reader.word()? == 17 {
                    let x = reader.word()?;
                    let y = reader.word()?;
                    let z = reader.word()?;
                    module.set_required_local_size(function, [x, y, z]);
                }
            }
            71 => {
                let target = reader.word()?;
                let decoration = parse_decoration(&mut reader)?;
                module.set_decoration(target, decoration)?;
            }
            72 => {
                let target = reader.word()?;
                let _member = reader.word()?;
                let decoration = parse_decoration(&mut reader)?;
                module.set_decoration(target, decoration)?;
            }
            73 => {}
            74 => {
                let group = reader.word()?;
                dec_groups.push((group, reader.rest()));
            }
            19 => {
                let result = reader.word()?;
                module.set_type(result, bitcode::Type::Void)?;
            }
            20 => {
                let result = reader.word()?;
                module.set_type(result, bitcode::Type::Bool)?;
            }
            21 => {
                let result = reader.word()?;
                let width = reader.word()?;
                let _signedness = reader.word()?;
                module.set_type(result, bitcode::Type::Int { width })?;
            }
            22 => {
                let result = reader.word()?;
                let width = reader.word()?;
                module.set_type(result, bitcode::Type::Float { width })?;
            }
            23 => {
                let result = reader.word()?;
                let component_type = reader.word()?;
                let count = reader.word()?;
                module.set_type(result, bitcode::Type::Vector { component_type, count })?;
            }
            28 => {
                let result = reader.word()?;
                let element_type = reader.word()?;
                let length = reader.word()?;
                let count = match module.constant(length).map(|entry| &entry.kind) {
                    Ok(bitcode::ConstantKind::Scalar { bits }) => *bits,
                    _ => 0,
                };
                module.set_type(result, bitcode::Type::Array { element_type, count })?;
            }
            30 => {
                let result = reader.word()?;
                let member_types = reader.rest();
                module.set_type(result, bitcode::Type::Struct { member_types })?;
            }
            32 => {
                let result = reader.word()?;
                let storage = bitcode::StorageClass::from_word(reader.word()?)?;
                let pointee_type = reader.word()?;
                module.set_type(result, bitcode::Type::Pointer { storage, pointee_type })?;
            }
            33 => {
                let result = reader.word()?;
                let return_type = reader.word()?;
                let parameter_types = reader.rest();
                module.set_type(
                    result,
                    bitcode::Type::Function { return_type, parameter_types },
                )?;
            }
            41 | 48 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::True)?;
            }
            42 | 49 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::False)?;
            }
            43 | 50 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let mut bits = u64::from(reader.word()?);
                if let Some(high) = reader.try_word() {
                    bits |= u64::from(high) << 32;
                }
                module.set_constant(result, result_type, bitcode::ConstantKind::Scalar { bits })?;
            }
            44 | 51 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let constituents = reader.rest();
                module.set_constant(
                    result,
                    result_type,
                    bitcode::ConstantKind::Composite { members: constituents },
                )?;
            }
            46 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::Null)?;
            }
            1 if pending_block.is_none() => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::Null)?;
            }
            54 => {
                let return_type = reader.word()?;
                let result = reader.word()?;
                let _control = reader.word()?;
                let _function_type = reader.word()?;
                pending_function = Some(bitcode::Function {
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
                    function
                        .parameters
                        .push(bitcode::Parameter { result, result_type });
                }
            }
            56 => {
                if let Some(mut function) = pending_function.take() {
                    if let Some(block) = pending_block.take() {
                        function.block_of_label[block.label as usize] = Some(function.blocks.len());
                        function.blocks.push(block);
                    }
                    module.push_function(function)?;
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
                pending_block = Some(bitcode::Block::new(label));
            }
            59 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let storage = bitcode::StorageClass::from_word(reader.word()?)?;
                let initializer = reader.try_word();

                if let Some(block) = pending_block.as_mut() {
                    block.push(
                        bitcode::Instruction::Variable { result, result_type, initializer },
                        current_line,
                    );
                } else {
                    module.push_variable(bitcode::Variable {
                        result,
                        result_type,
                        storage,
                        initializer,
                    })?;
                }
            }
            _ => {
                if let Some(block) = pending_block.as_mut() {
                    let instruction = parse_body(opcode, &mut reader, &module)?;
                    let ends_block = terminates(&instruction);

                    block.push(instruction, current_line);

                    if ends_block {
                        current_line = None;
                    }
                }
            }
        }

        reader.finish()?;
    }

    set_decoration_groups(&mut module, dec_groups)?;
    set_layouts(&mut module)?;

    Ok(module.finalize())
}

fn parse_decoration<I: Iterator<Item = u32>>(
    reader: &mut Reader<'_, I>,
) -> Result<bitcode::Decoration, bitcode::Error> {
    Ok(match reader.word()? {
        1 => bitcode::Decoration::SpecId(reader.word()?),
        10 => bitcode::Decoration::CPacked,
        11 => bitcode::Decoration::BuiltIn(bitcode::Builtin::from_word(reader.word()?)?),
        28 => bitcode::Decoration::SaturatedConversion,
        39 => bitcode::Decoration::FPRoundingMode(match reader.word()? {
            1 => bitcode::RoundingMode::TowardZero,
            2 => bitcode::RoundingMode::TowardPositive,
            3 => bitcode::RoundingMode::TowardNegative,
            _ => bitcode::RoundingMode::ToNearestEven,
        }),
        44 => bitcode::Decoration::Alignment(reader.word()?),
        _ => bitcode::Decoration::Ignored,
    })
}

fn unary_op(opcode: u16) -> Option<bitcode::UnaryOp> {
    Some(match opcode {
        126 => bitcode::UnaryOp::SNegate,
        127 => bitcode::UnaryOp::FNegate,
        168 => bitcode::UnaryOp::LogicalNot,
        200 => bitcode::UnaryOp::Not,
        _ => return None,
    })
}

fn binary_op(opcode: u16) -> Option<bitcode::BinaryOp> {
    Some(match opcode {
        128 => bitcode::BinaryOp::IAdd,
        129 => bitcode::BinaryOp::FAdd,
        130 => bitcode::BinaryOp::ISub,
        131 => bitcode::BinaryOp::FSub,
        132 => bitcode::BinaryOp::IMul,
        133 => bitcode::BinaryOp::FMul,
        134 => bitcode::BinaryOp::UDiv,
        135 => bitcode::BinaryOp::SDiv,
        136 => bitcode::BinaryOp::FDiv,
        137 => bitcode::BinaryOp::UMod,
        138 => bitcode::BinaryOp::SRem,
        139 => bitcode::BinaryOp::SMod,
        140 => bitcode::BinaryOp::FRem,
        141 => bitcode::BinaryOp::FMod,
        164 => bitcode::BinaryOp::LogicalEqual,
        165 => bitcode::BinaryOp::LogicalNotEqual,
        166 => bitcode::BinaryOp::LogicalOr,
        167 => bitcode::BinaryOp::LogicalAnd,
        170 => bitcode::BinaryOp::IEqual,
        171 => bitcode::BinaryOp::INotEqual,
        172 => bitcode::BinaryOp::UGreaterThan,
        173 => bitcode::BinaryOp::SGreaterThan,
        174 => bitcode::BinaryOp::UGreaterThanEqual,
        175 => bitcode::BinaryOp::SGreaterThanEqual,
        176 => bitcode::BinaryOp::ULessThan,
        177 => bitcode::BinaryOp::SLessThan,
        178 => bitcode::BinaryOp::ULessThanEqual,
        179 => bitcode::BinaryOp::SLessThanEqual,
        180 => bitcode::BinaryOp::FOrdEqual,
        181 => bitcode::BinaryOp::FUnordEqual,
        182 => bitcode::BinaryOp::FOrdNotEqual,
        183 => bitcode::BinaryOp::FUnordNotEqual,
        184 => bitcode::BinaryOp::FOrdLessThan,
        185 => bitcode::BinaryOp::FUnordLessThan,
        186 => bitcode::BinaryOp::FOrdGreaterThan,
        187 => bitcode::BinaryOp::FUnordGreaterThan,
        188 => bitcode::BinaryOp::FOrdLessThanEqual,
        189 => bitcode::BinaryOp::FUnordLessThanEqual,
        190 => bitcode::BinaryOp::FOrdGreaterThanEqual,
        191 => bitcode::BinaryOp::FUnordGreaterThanEqual,
        194 => bitcode::BinaryOp::ShiftRightLogical,
        195 => bitcode::BinaryOp::ShiftRightArithmetic,
        196 => bitcode::BinaryOp::ShiftLeftLogical,
        197 => bitcode::BinaryOp::BitwiseOr,
        198 => bitcode::BinaryOp::BitwiseXor,
        199 => bitcode::BinaryOp::BitwiseAnd,
        _ => return None,
    })
}

fn convert_op(opcode: u16) -> Option<bitcode::ConvertOp> {
    Some(match opcode {
        109 => bitcode::ConvertOp::FToU,
        110 => bitcode::ConvertOp::FToS,
        111 => bitcode::ConvertOp::SToF,
        112 => bitcode::ConvertOp::UToF,
        113 => bitcode::ConvertOp::UConvert,
        114 => bitcode::ConvertOp::SConvert,
        115 => bitcode::ConvertOp::FConvert,
        117 | 120 | 121 | 122 | 124 => bitcode::ConvertOp::Bitcast,
        _ => return None,
    })
}

fn atomic_op(opcode: u16) -> Option<bitcode::AtomicOp> {
    Some(match opcode {
        227 => bitcode::AtomicOp::Load,
        228 => bitcode::AtomicOp::Store,
        229 => bitcode::AtomicOp::Exchange,
        230 => bitcode::AtomicOp::CompareExchange,
        232 => bitcode::AtomicOp::Increment,
        233 => bitcode::AtomicOp::Decrement,
        234 => bitcode::AtomicOp::Add,
        235 => bitcode::AtomicOp::Sub,
        236 => bitcode::AtomicOp::SignedMin,
        237 => bitcode::AtomicOp::UnsignedMin,
        238 => bitcode::AtomicOp::SignedMax,
        239 => bitcode::AtomicOp::UnsignedMax,
        240 => bitcode::AtomicOp::And,
        241 => bitcode::AtomicOp::Or,
        242 => bitcode::AtomicOp::Xor,
        _ => return None,
    })
}

fn parse_atomic<I: Iterator<Item = u32>>(
    operation: bitcode::AtomicOp,
    reader: &mut Reader<'_, I>,
) -> Result<bitcode::Instruction, bitcode::Error> {
    let result = match operation {
        bitcode::AtomicOp::Store => None,
        _ => {
            let _result_type = reader.word()?;
            Some(reader.word()?)
        }
    };

    let pointer = reader.word()?;
    let _memory_scope = reader.word()?;
    let _memory_semantics = reader.word()?;

    let (value, comparator) = match operation {
        bitcode::AtomicOp::Load | bitcode::AtomicOp::Increment | bitcode::AtomicOp::Decrement => {
            (None, None)
        }
        bitcode::AtomicOp::CompareExchange => {
            let _unequal_semantics = reader.word()?;
            let value = reader.word()?;
            let comparator = reader.word()?;

            (Some(value), Some(comparator))
        }
        _ => (Some(reader.word()?), None),
    };

    Ok(bitcode::Instruction::Atomic { result, operation, pointer, value, comparator })
}

fn parse_body<I: Iterator<Item = u32>>(
    opcode: u16,
    reader: &mut Reader<'_, I>,
    module: &bitcode::ModuleBuilder,
) -> Result<bitcode::Instruction, bitcode::Error> {
    if let Some(operation) = unary_op(opcode) {
        let result_type = reader.word()?;
        let result = reader.word()?;
        let operand = reader.word()?;

        return Ok(bitcode::Instruction::Unary { result, result_type, operation, operand });
    }

    if let Some(operation) = binary_op(opcode) {
        let result_type = reader.word()?;
        let result = reader.word()?;
        let lhs = reader.word()?;
        let rhs = reader.word()?;

        return Ok(bitcode::Instruction::Binary { result, result_type, operation, lhs, rhs });
    }

    if let Some(operation) = atomic_op(opcode) {
        return parse_atomic(operation, reader);
    }

    if let Some(operation) = convert_op(opcode) {
        let result_type = reader.word()?;
        let result = reader.word()?;
        let operand = reader.word()?;

        return Ok(bitcode::Instruction::Convert { result, result_type, operation, operand });
    }

    Ok(match opcode {
        1 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            bitcode::Instruction::Undef { result, result_type }
        }
        61 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pointer = reader.word()?;
            let alignment = memory_alignment(reader);
            bitcode::Instruction::Load { result, result_type, pointer, alignment }
        }
        62 => {
            let pointer = reader.word()?;
            let object = reader.word()?;
            let alignment = memory_alignment(reader);
            bitcode::Instruction::Store { pointer, object, alignment }
        }
        65 | 66 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::AccessChain { result, result_type, base, indices, element: false }
        }
        67 | 70 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::AccessChain { result, result_type, base, indices, element: true }
        }
        169 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let condition = reader.word()?;
            let true_value = reader.word()?;
            let false_value = reader.word()?;
            bitcode::Instruction::Select { result, result_type, condition, true_value, false_value }
        }
        83 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            bitcode::Instruction::CopyObject { result, result_type, operand }
        }
        80 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let constituents = reader.rest();
            bitcode::Instruction::CompositeConstruct { result, result_type, members: constituents }
        }
        81 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let composite = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::CompositeExtract { result, result_type, composite, indices }
        }
        82 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let object = reader.word()?;
            let composite = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::CompositeInsert {
                result,
                result_type,
                object,
                composite,
                indices,
            }
        }
        79 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let first = reader.word()?;
            let second = reader.word()?;
            let components = reader.rest();
            bitcode::Instruction::VectorShuffle { result, result_type, first, second, components }
        }
        77 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let index = reader.word()?;
            bitcode::Instruction::VectorExtractDynamic { result, result_type, vector, index }
        }
        78 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let component = reader.word()?;
            let index = reader.word()?;
            bitcode::Instruction::VectorInsertDynamic {
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
            bitcode::Instruction::VectorTimesScalar { result, result_type, vector, scalar }
        }
        148 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let lhs = reader.word()?;
            let rhs = reader.word()?;
            bitcode::Instruction::Dot { result, result_type, lhs, rhs }
        }
        245 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pairs = reader
                .rest()
                .chunks_exact(2)
                .map(|pair| (pair[0], pair[1]))
                .collect();
            bitcode::Instruction::Phi { result, result_type, pairs }
        }
        57 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let function = reader.word()?;
            let arguments = reader.rest();
            bitcode::Instruction::FunctionCall { result, result_type, function, arguments }
        }
        249 => {
            let target = reader.word()?;
            bitcode::Instruction::Branch { target }
        }
        250 => {
            let condition = reader.word()?;
            let true_target = reader.word()?;
            let false_target = reader.word()?;
            bitcode::Instruction::BranchConditional { condition, true_target, false_target }
        }
        251 => {
            let selector = reader.word()?;
            let default_target = reader.word()?;
            let cases = reader
                .rest()
                .chunks_exact(2)
                .map(|pair| (u64::from(pair[0]), pair[1]))
                .collect();
            bitcode::Instruction::Switch { selector, default_target, cases }
        }
        224 => {
            let execution_scope = reader.word()?;
            let _memory_scope = reader.word()?;
            let memory_semantics = memory_semantics(module, reader.word()?)?;
            bitcode::Instruction::Barrier { execution_scope, memory_semantics }
        }
        225 => {
            let _memory_scope = reader.word()?;
            let memory_semantics = memory_semantics(module, reader.word()?)?;
            bitcode::Instruction::MemoryBarrier { memory_semantics }
        }
        253 => bitcode::Instruction::Return,
        254 => {
            let value = reader.word()?;
            bitcode::Instruction::ReturnValue { value }
        }
        255 => bitcode::Instruction::Unreachable,
        0 | 246 | 247 | 256 | 257 => bitcode::Instruction::Nop,
        unsupported => return Err(bitcode::Error::UnsupportedOpcode(unsupported)),
    })
}

fn debug_location(
    module: &bitcode::ModuleBuilder,
    files: &[Option<bitcode::Id>],
    operands: &[bitcode::Id],
) -> Result<Option<bitcode::Location>, bitcode::Error> {
    let [source, line, _, column, _] = operands else {
        return Ok(None);
    };

    let Some(Some(file)) = files.get(*source as usize) else {
        return Ok(None);
    };

    let line = literal(module, *line)?;
    if line == 0 {
        return Ok(None);
    }

    Ok(Some(bitcode::Location {
        file: *file,
        line,
        column: literal(module, *column)?,
    }))
}

fn memory_alignment<I: Iterator<Item = u32>>(reader: &mut Reader<'_, I>) -> Option<u32> {
    let operands = reader.try_word()?;

    match operands & MEMORY_ALIGNED {
        0 => None,
        _ => reader.try_word(),
    }
}

fn memory_semantics(
    module: &bitcode::ModuleBuilder,
    id: bitcode::Id,
) -> Result<bitcode::MemorySemantics, bitcode::Error> {
    bitcode::MemorySemantics::from_word(literal(module, id)?)
}

fn literal(module: &bitcode::ModuleBuilder, id: bitcode::Id) -> Result<u32, bitcode::Error> {
    match &module.constant(id)?.kind {
        bitcode::ConstantKind::Scalar { bits } => Ok(*bits as u32),
        _ => Err(bitcode::Error::NotAConstant(id)),
    }
}

fn terminates(instruction: &bitcode::Instruction) -> bool {
    matches!(
        instruction,
        bitcode::Instruction::Branch { .. }
            | bitcode::Instruction::BranchConditional { .. }
            | bitcode::Instruction::Switch { .. }
            | bitcode::Instruction::Return
            | bitcode::Instruction::ReturnValue { .. }
            | bitcode::Instruction::Unreachable
    )
}

fn set_decoration_groups(
    module: &mut bitcode::ModuleBuilder,
    groups: Vec<(bitcode::Id, Vec<bitcode::Id>)>,
) -> Result<(), bitcode::Error> {
    for (group, targets) in groups {
        let source = module.decorations(group)?.clone();
        for target in targets {
            module.merge_decoration(target, &source)?;
        }
    }

    Ok(())
}

fn set_layouts(module: &mut bitcode::ModuleBuilder) -> Result<(), bitcode::Error> {
    for type_id in 0..module.bound() {
        match calc_layout(module, type_id as bitcode::Id) {
            Ok(_) => (),
            Err(err) => match err {
                bitcode::Error::NotAType(_) => (),
                err => return Err(err),
            },
        }
    }

    Ok(())
}

fn calc_layout(
    module: &mut bitcode::ModuleBuilder,
    type_id: bitcode::Id,
) -> Result<&bitcode::Layout, bitcode::Error> {
    match module.layout(type_id) {
        Err(err) => match err {
            bitcode::Error::NotAType(_) => {
                let layout = calc_layout_inner(module, type_id)?;
                module.set_layout(type_id, layout)
            }
            err => Err(err),
        },
        Ok(_) => module.layout(type_id),
    }
}

fn calc_layout_inner(
    module: &mut bitcode::ModuleBuilder,
    type_id: bitcode::Id,
) -> Result<bitcode::Layout, bitcode::Error> {
    let declared = module.type_(type_id)?;

    Ok(match declared {
        bitcode::Type::Void => {
            bitcode::Layout { size: 0, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Function { .. } => {
            bitcode::Layout { size: 0, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Bool => {
            bitcode::Layout { size: 1, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Int { width } | bitcode::Type::Float { width } => {
            let size = (*width as usize).div_ceil(8);

            bitcode::Layout { size, alignment: size, member_offsets: Vec::new() }
        }
        bitcode::Type::Pointer { .. } => {
            let size = match module.addressing() {
                bitcode::Addressing::Physical32 => 4,
                _ => 8,
            };

            bitcode::Layout { size, alignment: size, member_offsets: Vec::new() }
        }
        bitcode::Type::Vector { component_type, count } => {
            let (component_type, count) = (*component_type, *count as usize);
            let lanes = if count == 3 { 4 } else { count };
            let element_type = calc_layout(module, component_type)?.size;

            bitcode::Layout {
                size: element_type * lanes,
                alignment: element_type * lanes,
                member_offsets: Vec::new(),
            }
        }
        bitcode::Type::Array { element_type, count } => {
            let (element_type, count) = (*element_type, *count as usize);
            let entry = calc_layout(module, element_type)?;

            bitcode::Layout {
                size: entry.size * count,
                alignment: entry.alignment,
                member_offsets: Vec::new(),
            }
        }
        bitcode::Type::Struct { member_types } => {
            let count = member_types.len();
            let packed = module.decorations(type_id)?.packed;

            let entries = (0..count)
                .map(|index| {
                    let member = module.member_type(type_id, index)?;
                    let entry = calc_layout(module, member)?;
                    Ok((entry.size, if packed { 1 } else { entry.alignment }))
                })
                .collect::<Result<Vec<_>, bitcode::Error>>()?;

            let alignment = entries
                .iter()
                .map(|(_, member_alignment)| *member_alignment)
                .max()
                .unwrap_or(1);

            let mut offset = 0;
            let member_offsets = entries
                .iter()
                .map(|(size, member_alignment)| {
                    let member_offset = bitcode::align_up(offset, *member_alignment);
                    offset = member_offset + size;
                    member_offset
                })
                .collect();

            bitcode::Layout {
                size: bitcode::align_up(offset, alignment),
                alignment,
                member_offsets,
            }
        }
    })
}
