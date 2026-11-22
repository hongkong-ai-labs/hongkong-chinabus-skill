//! Immigration Department land-boundary queue times.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::api::{BorderPoint, BorderReport, Client, now_hkt};

const IMMD_R: &str = "https://secure1.info.gov.hk/immd/mobileapps/2bb9ae17/data/CPQueueTimeR.json";
const IMMD_V: &str = "https://secure1.info.gov.hk/immd/mobileapps/2bb9ae17/data/CPQueueTimeV.json";

/// Land control points in the order the ImmD app lists them.
const CONTROL_POINTS: [(&str, &str, &str); 9] = [
    ("HGP", "Huanggang", "皇崗"),
    ("HYW", "Heung Yuen Wai", "香園圍"),
    ("HZM", "Hong Kong-Zhuhai-Macao Bridge", "港珠澳大橋"),
    ("LMC", "Lok Ma Chau", "落馬洲"),
    ("LSC", "Lok Ma Chau Spur Line", "落馬洲支線"),
    ("LWS", "Lo Wu", "羅湖"),
    ("MKT", "Man Kam To", "文錦渡"),
    ("SBC", "Shenzhen Bay", "深圳灣"),
    ("STK", "Sha Tau Kok", "沙頭角"),
];

pub async fn borders(client: &Client) -> Result<BorderReport> {
    let (residents, visitors) = tokio::try_join!(
        client.get("immd-r", IMMD_R, Duration::from_secs(120)),
        client.get("immd-v", IMMD_V, Duration::from_secs(120))
    )?;
    let residents: Value =
        serde_json::from_str(&residents).context("decoding resident border queues")?;
    let visitors: Value =
        serde_json::from_str(&visitors).context("decoding visitor border queues")?;
    let mut points = Vec::new();
    for (code, name, name_zh) in CONTROL_POINTS {
        let Some(resident) = residents.get(code) else {
            continue;
        };
        let visitor = visitors.get(code).unwrap_or(&Value::Null);
        points.push(BorderPoint {
            code: (*code).to_string(),
            name: (*name).to_string(),
            name_zh: (*name_zh).to_string(),
            resident_arrival: queue_label(resident.get("arrQueue")),
            resident_departure: queue_label(resident.get("depQueue")),
            visitor_arrival: queue_label(visitor.get("arrQueue")),
            visitor_departure: queue_label(visitor.get("depQueue")),
        });
    }
    Ok(BorderReport {
        fetched_at: now_hkt(),
        points,
    })
}

/// Turn an ImmD queue integer into a readable band.
pub fn queue_label(value: Option<&Value>) -> String {
    let Some(value) = value.and_then(Value::as_i64) else {
        return "unknown".to_string();
    };
    match value {
        0 => "normal (<15 min)".to_string(),
        1 => "busy (15-30 min)".to_string(),
        2 => "very busy (30-60 min)".to_string(),
        3 => "congested (>60 min)".to_string(),
        99 => "closed / not in service".to_string(),
        other => format!("queue band {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_queue_bands() {
        assert_eq!(queue_label(Some(&Value::from(0))), "normal (<15 min)");
        assert_eq!(
            queue_label(Some(&Value::from(99))),
            "closed / not in service"
        );
        assert_eq!(queue_label(None), "unknown");
    }
}
