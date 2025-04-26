use std::sync::Arc;

use crate::state::SharedState;

pub async fn purge_task(state: Arc<SharedState>) -> Result<(), Box<dyn std::error::Error>> {
    // Perform the purge task
    tracing::info!("Running purge task...");

    Ok(())
}
