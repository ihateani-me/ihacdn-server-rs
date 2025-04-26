use std::{net::IpAddr, sync::Arc};

use axum::http::{
    HeaderMap, HeaderValue,
    header::{self, GetAll},
};

use crate::{config::IhaCdnConfig, state::CDNData};

pub fn extract_ip_address(headers: &HeaderMap) -> Vec<IpAddr> {
    // Get rightmost IP address from X-Forwarded-For header
    let x_forwarded_for: Vec<IpAddr> = parse_specific_headers(&headers.get_all("x-forwarded-for"));
    let forwarded: Vec<IpAddr> = parse_specific_headers(&headers.get_all(header::FORWARDED));
    let x_real_ip: Vec<IpAddr> = parse_specific_headers(&headers.get_all("x-real-ip"));
    let cf_connecting_ip: Vec<IpAddr> =
        parse_specific_headers(&headers.get_all("cf-connecting-ip"));
    let cf_connecting_ipv6: Vec<IpAddr> =
        parse_specific_headers(&headers.get_all("cf-connecting-ipv6"));

    let mut ip_address: Vec<IpAddr> = vec![];
    ip_address.extend(cf_connecting_ip);
    ip_address.extend(cf_connecting_ipv6);
    ip_address.extend(x_forwarded_for);
    ip_address.extend(forwarded);
    ip_address.extend(x_real_ip);

    ip_address.retain(|ip| !is_private_ip(*ip));
    ip_address
}

fn parse_specific_headers(headers: &GetAll<HeaderValue>) -> Vec<IpAddr> {
    headers
        .iter()
        .filter_map(|v| {
            // parse into IpAddr
            match v.to_str() {
                Ok(v) => match v.parse() {
                    Ok(v) => Some(v),
                    Err(_) => None,
                },
                Err(_) => None,
            }
        })
        .collect()
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            if ipv4.is_private()
                || ipv4.is_loopback()
                || ipv4.is_link_local()
                || ipv4.is_unspecified()
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_multicast()
            {
                true
            } else {
                false
            }
        }
        IpAddr::V6(ipv6) => {
            if ipv6.is_loopback()
                || ipv6.is_multicast()
                || ipv6.is_unspecified()
                || ipv6.is_unicast_link_local()
                || ipv6.is_unique_local()
            {
                true
            } else {
                false
            }
        }
    }
}

// Actual notifier code
pub fn notify_discord(
    final_url: impl Into<String>,
    cdn_data: CDNData,
    config: &Arc<IhaCdnConfig>,
    ip_address: Vec<IpAddr>,
) {
    if !config.notifier.enable {
        return;
    }

    let webhook_url = match &config.notifier.discord_webhook {
        Some(url) => {
            if url.is_empty() {
                tracing::warn!("Discord webhook URL is empty. Skipping notification.");
                return;
            }
            url.to_string()
        }
        None => {
            tracing::warn!("Discord webhook URL is not set. Skipping notification.");
            return;
        }
    };

    let final_url = final_url.into();
    tokio::spawn(async move {
        let ip_address = ip_address
            .iter()
            .map(|ip| ip.to_string())
            .collect::<Vec<String>>()
            .join(", ");
        let ip_address = if ip_address.is_empty() {
            "Unknown IP".to_string()
        } else {
            ip_address
        };
        let mut msg_contents = vec![format!("Uploader IPs: **{}**", ip_address)];
        match cdn_data {
            CDNData::Short { .. } => {
                msg_contents.push(format!("Short URL: **<{}>**", final_url));
            }
            _ => {
                msg_contents.push(format!("File: **<{}>**", final_url));
            }
        }
        let is_admin = if cdn_data.is_admin() { "Yes" } else { "No" };
        msg_contents.push(format!("Is Admin? **{}**", is_admin));

        let serde_data = serde_json::json!({
            "content": msg_contents.join("\n"),
            "avatar_url": "https://p.ihateani.me/static/img/favicon.png",
            "username": "ihaCDN Notificator",
            "tts": false,
        });

        let body_data = serde_json::to_string(&serde_data).unwrap();

        // post to discord webhook
        match reqwest::Client::new()
            .post(webhook_url)
            .body(body_data)
            .header("Content-Type", "application/json")
            .header(
                "User-Agent",
                "ihacdn-rs/0.1.0 (+https://github.com/ihateani-me/ihacdn-server-rs)",
            )
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
