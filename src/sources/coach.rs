//! GoByBus coach directory and live schedules.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde_json::Value;

use crate::api::{Client, CoachStation, CoachTrip};

const PAGE: &str =
    "https://www.gobybus.hk/route/en/hong-kong-and-shenzhen-bay-checkpoint-cross-border-bus/HKD";
const SCHEDULE: &str = "https://www.gobybus.hk/zh/getSchedule.php";

pub async fn directory(
    client: &Client,
) -> Result<(Vec<CoachStation>, HashMap<String, Vec<String>>)> {
    let html = client
        .get("gobybus-page", PAGE, Duration::from_secs(3600))
        .await?;
    let station_map = extract_object(&html, "const stationMap = ")
        .context("GoByBus page no longer contains stationMap")?;
    let route_map = extract_object(&html, "depStnRouteMap = ")
        .context("GoByBus page no longer contains depStnRouteMap")?;
    let mut stations = Vec::new();
    if let Some(object) = station_map.as_object() {
        for (id, station) in object {
            let name = station
                .get("enName")
                .or_else(|| station.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            stations.push(CoachStation {
                id: id.clone(),
                name: name.to_string(),
                address: station
                    .get("address")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(object) = route_map.as_object() {
        for (departure, destinations) in object {
            let list = destinations
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            adjacency.insert(departure.clone(), list);
        }
    }
    stations.sort_by(|left, right| left.name.cmp(&right.name));
    Ok((stations, adjacency))
}

pub async fn trips(
    client: &Client,
    departure: &str,
    arrival: &str,
    date: NaiveDate,
) -> Result<Vec<CoachTrip>> {
    let response = client
        .http
        .post(SCHEDULE)
        .form(&[
            ("dep_stn_id", departure),
            ("arr_stn_id", arrival),
            ("dep_date", &date.to_string()),
        ])
        .send()
        .await
        .context("posting GoByBus schedule request")?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        bail!("GoByBus schedule returned HTTP {status}");
    }
    parse_trips(&body)
}

fn parse_trips(body: &str) -> Result<Vec<CoachTrip>> {
    let value: Value = serde_json::from_str(body.trim()).context("decoding GoByBus schedule")?;
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        bail!("GoByBus schedule was not successful: {body}");
    }
    let trips = value
        .get("data")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| CoachTrip {
                    route_number: text(row, "route_number"),
                    from: text(row, "dep_stn_name"),
                    to: text(row, "arr_stn_name"),
                    depart: text(row, "dep_time"),
                    arrive: text(row, "arr_time"),
                    price: text(row, "adult_price"),
                    currency: text(row, "currency"),
                    seats_available: row
                        .get("avail_ticket")
                        .and_then(Value::as_str)
                        .and_then(|value| value.parse().ok()),
                    coach_type: match text(row, "car_type").as_str() {
                        "B" => "bus".to_string(),
                        "C" => "car".to_string(),
                        other => other.to_string(),
                    },
                    option_id: text(row, "opt_id"),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(trips)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn extract_object(html: &str, marker: &str) -> Option<Value> {
    let start = html.find(marker)? + marker.len();
    let bytes = html.as_bytes();
    let open = start + html[start..].find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let end = open + offset + 1;
                    return serde_json::from_str(&html[open..end]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_js_objects() {
        let html = r#"<script>const stationMap = {"1":{"enName":"A","address":"x"},"2":{"enName":"B"}};
            depStnRouteMap = {"1":["2"]};</script>"#;
        let stations = extract_object(html, "const stationMap = ").unwrap();
        assert_eq!(stations["1"]["enName"], "A");
        let routes = extract_object(html, "depStnRouteMap = ").unwrap();
        assert_eq!(routes["1"][0], "2");
    }

    #[test]
    fn parses_coach_trips() {
        let body = r#"{"success":true,"data":[{"route_number":"SW+1+2+08:05",
            "dep_stn_name":"Hong Kong","arr_stn_name":"Macao","dep_time":"08:05:00",
            "arr_time":"10:05:00","adult_price":"330","currency":"HK$",
            "avail_ticket":"4","car_type":"C","opt_id":"95"}]}"#;
        let trips = parse_trips(body).unwrap();
        assert_eq!(trips.len(), 1);
        assert_eq!(trips[0].price, "330");
        assert_eq!(trips[0].coach_type, "car");
    }
}
