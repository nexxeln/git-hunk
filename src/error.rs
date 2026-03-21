use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    pub fn io(err: std::io::Error) -> Self {
        Self::new("io_error", err.to_string())
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(&ErrorEnvelope {
            error: self.clone(),
        })
        .expect("error should serialize")
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Serialize)]
struct ErrorEnvelope {
    error: AppError,
}
