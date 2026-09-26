//! Fuentes de estado. Ninguna es obligatoria: la fusión usa lo que haya.

pub mod logfile;
pub mod modfile;
pub mod process;

pub use logfile::LogWatcher;
pub use modfile::ModFileWatcher;
pub use process::ProcessWatcher;
