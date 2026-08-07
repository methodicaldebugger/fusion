//contents of errors.rs
#[derive(Debug)]
pub enum FusionError {
    UnknownVariable(String),
    TypeMismatch {
        expected:String,
        found:String,
    },
    InvalidOperation {
        left:String,
        operator:String,
        right:String,
    }
}
impl std::fmt::Display for FusionError {
    fn fmt(&self,f:&mut std::fmt::Formatter)->std::fmt::Result
    {
        match self {
            FusionError::UnknownVariable(name)=>
                write!(f,"Unknown variable '{}'",name),
            FusionError::TypeMismatch {
                expected,found
            }=>
                write!(f,"Type mismatch: expected {}, found {}",expected,found),
            FusionError::InvalidOperation {
                left,
                operator,
                right
            }=>
                write!(f,"Invalid operation: {} {} {}",left,operator,right),
        }
    }
}