//! Minimal error plumbing: a boxed error plus `.context()` for readable messages.

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub trait Context<T> {
    fn context(self, msg: impl std::fmt::Display) -> Result<T>;
}

impl<T, E: std::fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, msg: impl std::fmt::Display) -> Result<T> {
        self.map_err(|e| format!("{msg}: {e}").into())
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, msg: impl std::fmt::Display) -> Result<T> {
        self.ok_or_else(|| msg.to_string().into())
    }
}
