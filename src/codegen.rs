
// Public compiler backend facade.
//
// The bootstrap compiler emits textual LLVM IR first. Keeping this facade small
// means the backend can later switch to an LLVM API binding without changing
// the command-line driver or front-end.
pub use crate::llvm_backend::LLVMBackend;

pub struct CodeGenerator;

impl CodeGenerator {
    pub fn new() -> Self { Self }
    pub fn emit(&self, program: &crate::ast::Program) -> Result<String, crate::errors::FusionError> {
        LLVMBackend::new().emit_program(program)
    }
}
