use std::sync::Arc;

use crate::state::{CDNData, PREFIX, SharedState};

pub async fn purge_task(state: Arc<SharedState>) -> Result<(), Box<dyn std::error::Error>> {
    // Perform the purge task
    tracing::info!("Running purge task...");

    if !state.config.retention.enable {
        tracing::info!("Retention is disabled, skipping purge task.");
        return Ok(());
    }

    let mut connection = state.make_connection().await?;

    let available_keys = redis::cmd("KEYS")
        .arg(format!("{PREFIX}*"))
        .query_async::<Vec<String>>(&mut connection)
        .await?;

    if !available_keys.is_empty() {
        tracing::info!("No keys to purge.");
        return Ok(());
    }

    tracing::info!("Purging {} keys", available_keys.len());
    let keys_metadata = redis::cmd("MGET")
        .arg(available_keys.clone())
        .query_async::<Vec<Option<String>>>(&mut connection)
        .await?;

    let mut keys_to_be_deleted = vec![];
    for (keys_meta, key) in keys_metadata.iter().zip(available_keys.iter()) {
        if let Some(value) = keys_meta {
            let serde_data = serde_json::from_str::<CDNData>(&value)?;
            // check file size
            if serde_data.is_expired(&state.config).await {
                keys_to_be_deleted.push((key.clone(), serde_data));
            }
        }
    }

    let bulk_delete: Vec<String> = keys_to_be_deleted
        .iter()
        .map(|(key, _)| key.clone())
        .collect();
    // delete files from disk first
    for (_, data) in keys_to_be_deleted {
        data.delete_file().await;
    }
    redis::cmd("DEL")
        .arg(bulk_delete)
        .query_async::<Vec<String>>(&mut connection)
        .await?;

    Ok(())
}
