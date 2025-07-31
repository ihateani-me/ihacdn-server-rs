use std::{net::IpAddr, sync::Arc};

use crate::{config::IhaCdnConfig, state::CDNData};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PlausibleEvent {
    name: String,
    url: String,
    props: Option<serde_json::Value>,
    domain: Option<String>,
    referrer: Option<String>,
    interactive: bool,
}

// Actual notifier code
pub fn report_to_plausible(
    final_url: impl Into<String>,
    cdn_data: &CDNData,
    config: &Arc<IhaCdnConfig>,
    ip_address: Vec<IpAddr>,
    referrer: Option<String>,
    user_agent: Option<String>,
) {
    if !config.plausible.is_enabled() {
        return;
    }

    let psb_domain = match &config.plausible.domain {
        Some(url) => {
            if url.is_empty() {
                tracing::warn!("Plausible domain is empty. Skipping tracking.");
                return;
            }
            url.to_string()
        }
        None => {
            tracing::warn!("Plausible domain is not set. Skipping tracking.");
            return;
        }
    };
    let psb_endpoint = config.plausible.endpoint_url();

    let kind = match cdn_data {
        CDNData::Short { .. } => "short",
        CDNData::File { .. } => "file",
        CDNData::Code { .. } => "code",
    };
    let is_admin_upload = cdn_data.is_admin();

    let event = PlausibleEvent {
        name: "pageview".to_string(),
        url: final_url.into(),
        props: Some(serde_json::json!({
            "kind": kind,
            "is_admin_upload": is_admin_upload,
        })),
        domain: Some(psb_domain.clone()),
        referrer,
        interactive: false,
    };

    let user_agent: String = user_agent.unwrap_or_else(|| {
        "ihacdn-rs/0.1.0 (+https://github.com/ihateani-me/ihacdn-server-rs)".to_string()
    });

    tokio::spawn(async move {
        let ip_addresses = ip_address
            .iter()
            .map(|ip| ip.to_string())
            .collect::<Vec<String>>()
            .join(", ");

        tracing::debug!("Reporting to Plausible: {:?}", &event);
        tracing::debug!("Using Plausible endpoint: {}", psb_endpoint);
        tracing::debug!("IP Address: {}", ip_addresses);
        let body_data = match serde_json::to_string(&event) {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to serialize Plausible event: {}", e);
                return;
            }
        };
        // post to discord webhook
        match reqwest::Client::new()
            .post(psb_endpoint)
            .body(body_data)
            .header("Content-Type", "application/json")
            .header("User-Agent", user_agent)
            .header("X-Forwarded-For", ip_addresses.clone())
            .header("X-Forwarded-Plausible-For", ip_addresses)
            .send()
            .await
        {
            Ok(_) => {
                tracing::info!("Discord notification sent successfully.");
            }
            Err(e) => {
                tracing::error!("Failed to send Discord notification: {}", e);
            }
        }
    });
}
