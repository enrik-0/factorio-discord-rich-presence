//! State sources. None is mandatory: the merge uses whatever is available.

pub mod logfile;
pub mod modfile;
pub mod process;

pub use logfile::LogWatcher;
pub use modfile::ModFileWatcher;
pub use process::ProcessWatcher;
