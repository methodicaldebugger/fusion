//contents of ir.rs

// ir.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone)]
pub struct IrProgram {
    pub functions: Vec<IrFunction>,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub id: FunctionId,
    pub name: String,
    pub parameters: Vec<ValueId>,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

#[derive(Debug, Clone)]
pub enum Instruction {
    ConstInt {
        destination: ValueId,
        value: i64,
    },

    ConstFloat {
        destination: ValueId,
        value: f64,
    },

    ConstBool {
        destination: ValueId,
        value: bool,
    },

    ConstString {
        destination: ValueId,
        value: String,
    },

    Move {
        destination: ValueId,
        source: ValueId,
    },

    Add {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Sub {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Mul {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Div {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Equal {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    NotEqual {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Less {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    LessEqual {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Greater {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    GreaterEqual {
        destination: ValueId,
        left: ValueId,
        right: ValueId,
    },

    Call {
        destination: Option<ValueId>,
        function: FunctionId,
        arguments: Vec<ValueId>,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Return {
        value: Option<ValueId>,
    },

    Jump {
        target: BlockId,
    },

    Branch {
        condition: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
}