//contents of hir.rs

pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
    pub enums: Vec<HirEnum>,
}