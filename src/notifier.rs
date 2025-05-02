use std::{
    net::IpAddr,
    sync::{Arc, LazyLock},
};

use axum::http::{
    HeaderMap, HeaderValue,
    header::{self, GetAll},
};
use ipnet::IpNet;

use crate::{config::IhaCdnConfig, state::CDNData};

static CF_IPV4_BLOCKS: LazyLock<Vec<IpNet>> = LazyLock::new(|| {
    let blocked_ranges = vec![
        "173.245.48.0/20",
        "103.21.244.0/22",
        "103.22.200.0/22",
        "103.31.4.0/22",
        "141.101.64.0/18",
        "108.162.192.0/18",
        "190.93.240.0/20",
        "188.114.96.0/20",
        "197.234.240.0/22",
        "198.41.128.0/17",
        "162.158.0.0/15",
        "104.16.0.0/13",
        "104.24.0.0/14",
        "172.64.0.0/13",
        "131.0.72.0/22",
    ];

    let blocked_nets: Vec<IpNet> = blocked_ranges
        .iter()
        .filter_map(|range| range.parse().ok())
        .collect();

    blocked_nets
});

static CF_IPV6_BLOCKS: LazyLock<Vec<IpNet>> = LazyLock::new(|| {
    let blocked_ranges = vec![
        "2400:cb00::/32",
        "2606:4700::/32",
        "2803:f800::/32",
        "2405:b500::/32",
        "2405:8100::/32",
        "2a06:98c0::/29",
        "2c0f:f248::/32",
    ];

    let blocked_nets: Vec<IpNet> = blocked_ranges
        .iter()
        .filter_map(|range| range.parse().ok())
        .collect();

    blocked_nets
});

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
    ip_address.retain(|ip| !is_in_blocked_ranges(*ip));
    ip_address
}

fn parse_specific_headers(headers: &GetAll<HeaderValue>) -> Vec<IpAddr> {
    headers
        .iter()
        .filter_map(|v| {
            // parse into IpAddr
            match v.to_str() {
                Ok(v) => v.parse().ok(),
                Err(_) => None,
            }
        })
        .collect()
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_private()
                || ipv4.is_loopback()
                || ipv4.is_link_local()
                || ipv4.is_unspecified()
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_multicast()
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_multicast()
                || ipv6.is_unspecified()
                || ipv6.is_unicast_link_local()
                || ipv6.is_unique_local()
        }
    }
}

fn is_in_blocked_ranges(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => CF_IPV4_BLOCKS
            .iter()
            .any(|net| net.contains(&IpAddr::V4(ipv4))),
        IpAddr::V6(ipv6) => CF_IPV6_BLOCKS
            .iter()
            .any(|net| net.contains(&IpAddr::V6(ipv6))),
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
