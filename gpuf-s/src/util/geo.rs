use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};
use tracing::{debug, warn};

#[derive(Debug, Clone, Default)]
pub struct GeoLocation {
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeoEndpointResponse {
    country: Option<String>,
    #[serde(alias = "regionName", alias = "province")]
    region: Option<String>,
    city: Option<String>,
    #[serde(alias = "lat")]
    latitude: Option<f64>,
    #[serde(alias = "lon", alias = "lng")]
    longitude: Option<f64>,
}

pub fn normalize_public_ip_from_device(device_ip: Option<u32>, remote_addr: IpAddr) -> String {
    device_ip
        .filter(|ip| *ip != 0)
        .map(Ipv4Addr::from)
        .map(IpAddr::V4)
        .filter(|ip| is_public_ip(*ip))
        .unwrap_or(remote_addr)
        .to_string()
}

pub async fn lookup_geo(ip: &str) -> GeoLocation {
    let Ok(parsed) = ip.parse::<IpAddr>() else {
        return GeoLocation {
            source: Some("invalid".to_string()),
            ..GeoLocation::default()
        };
    };

    if !is_public_ip(parsed) {
        return GeoLocation {
            source: Some("private".to_string()),
            ..GeoLocation::default()
        };
    }

    let Some(endpoint) = std::env::var("GPUF_GEOIP_ENDPOINT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return GeoLocation {
            source: Some("unconfigured".to_string()),
            ..GeoLocation::default()
        };
    };

    let url = endpoint
        .replace("{ip}", ip)
        .trim_end_matches('/')
        .to_string();
    let url = if url == endpoint.trim_end_matches('/') && !endpoint.contains("{ip}") {
        format!("{}/{}", url, ip)
    } else {
        url
    };

    match reqwest::get(&url).await {
        Ok(resp) => match resp.error_for_status() {
            Ok(resp) => match resp.json::<GeoEndpointResponse>().await {
                Ok(geo) => GeoLocation {
                    country: geo.country,
                    region: geo.region,
                    city: geo.city,
                    latitude: geo.latitude,
                    longitude: geo.longitude,
                    source: Some("endpoint".to_string()),
                },
                Err(e) => {
                    warn!("Failed to parse geo endpoint response: {}", e);
                    GeoLocation {
                        source: Some("error".to_string()),
                        ..GeoLocation::default()
                    }
                }
            },
            Err(e) => {
                warn!("Geo endpoint returned an error: {}", e);
                GeoLocation {
                    source: Some("error".to_string()),
                    ..GeoLocation::default()
                }
            }
        },
        Err(e) => {
            debug!("Geo endpoint lookup failed: {}", e);
            GeoLocation {
                source: Some("error".to_string()),
                ..GeoLocation::default()
            }
        }
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified())
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}
