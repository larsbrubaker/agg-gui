//! Where fire-and-forget work runs. The crate has no async runtime, so code that starts work it
//! does not await (a remote folder's load, a shared download, a copy run by
//! `WritableFolder::copy_in`) is handed a [`Spawner`] and gives the work to it. An app implements
//! it over its own executor, which it pumps every frame; a test spawns onto its own and drives
//! it. Work that fails after its starter returned reports the failure through `report_fault`, so
//! the failure is never lost with nobody awaiting it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A local (not `Send`) unit of work that answers nothing.
pub type LocalTask = Pin<Box<dyn Future<Output = ()>>>;

/// Runs work nobody awaits.
pub trait Spawner {
    /// Starts `task`. It may run up to its first suspension before this returns, and is driven
    /// to completion by whoever pumps the spawner's executor.
    fn spawn_task(&self, task: LocalTask);

    /// Reports that `error` ended work nobody awaits.
    fn report_fault(&self, error: Arc<dyn std::error::Error + Send + Sync>);
}
