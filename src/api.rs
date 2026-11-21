//! Cross-border data clients.
//!
//! - **Immigration Department** queue times (official JSON, every 15 minutes).
//! - **GoByBus** coach station directory and live seat/schedule search.
//! - **Marine Department** cross-boundary ferry berthing records.
//! - **MTR / HSR** service information is published as timetables, not a live
//!   API; [`rail`] holds the reference data and official links.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;

const IMMD_R: &str = "https://secure1.info.gov.hk/immd/mobileapps/2bb9ae17/data/CPQueueTimeR.json";
const IMMD_V: &str = "https://secure1.info.gov.hk/immd/mobileapps/2bb9ae17/data/CPQueueTimeV.json";
const GOBYBUS_PAGE: &str =
    "https://www.gobybus.hk/route/en/hong-kong-and-shenzhen-bay-checkpoint-cross-border-bus/HKD";
const FERRY_PAGE: &str = "https://crossboundaryferryservices.mardep.gov.hk/en/";

/// One land boundary control point.
#[derive(Debug, Clone, Serialize)]
pub struct BorderPoint {
    pub code: String,
    pub name: String,
    pub name_zh: String,
    /// Resident arrival queue, as published.
    pub resident_arrival: String,
    pub resident_departure: String,
    /// Visitor arrival queue, as published.
    pub visitor_arrival: String,
    pub visitor_departure: String,
}

/// Queue snapshot for every land control point.
#[derive(Debug, Clone, Serialize)]
pub struct BorderReport {
    pub fetched_at: String,
    pub points: Vec<BorderPoint>,
}

/// One coach boarding or alighting point.
#[derive(Debug, Clone, Serialize)]
pub struct CoachStation {
    pub id: String,
    pub name: String,
    pub address: String,
}

/// A coach departure returned by GoByBus.
#[derive(Debug, Clone, Serialize)]
pub struct CoachTrip {
    pub route_number: String,
    pub from: String,
    pub to: String,
    pub depart: String,
    pub arrive: String,
    pub price: String,
    pub currency: String,
    pub seats_available: Option<u64>,
    pub coach_type: String,
    pub option_id: String,
}

/// Ferry berthing record.
#[derive(Debug, Clone, Serialize)]
pub struct FerryMovement {
    pub time: String,
    pub direction: String,
    pub port: String,
    pub operator: String,
    pub terminal: String,
    pub berth: String,
    pub status: String,
}

/// Live cross-boundary ferry board.
#[derive(Debug, Clone, Serialize)]
pub struct FerryReport {
    pub cft_updated: String,
    pub mft_updated: String,
    pub movements: Vec<FerryMovement>,
}

/// Cached cross-border client.
pub struct Client {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, String)>>,
}

impl Client {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "hongkong-chinabus-skill/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/hongkong-ai-labs/hongkong-chinabus-skill)"
            ))
            .timeout(Duration::from_secs(45))
            .build()?;
        Ok(Self {
            http,
            cache: Mutex::new(HashMap::new()),
        })
    }

    async fn get(&self, key: &str, url: &str, ttl: Duration) -> Result<String> {
        if let Some((at, body)) = self
            .cache
            .lock()
            .expect("cross-border cache poisoned")
            .get(key)
            .cloned()
            && at.elapsed() < ttl
        {
            return Ok(body);
        }
        let response = self
            .http
            .get(url)
            .header("Accept-Language", "en")
            .send()
            .await
            .with_context(|| format!("requesting {url}"))?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{url} returned HTTP {status}");
        }
        self.cache
            .lock()
            .expect("cross-border cache poisoned")
            .insert(key.to_string(), (Instant::now(), body.clone()));
        Ok(body)
    }

    /// Official Immigration Department land-boundary queue snapshot.
    pub async fn borders(&self) -> Result<BorderReport> {
        let (residents, visitors) = tokio::try_join!(
            self.get("immd-r", IMMD_R, Duration::from_secs(120)),
            self.get("immd-v", IMMD_V, Duration::from_secs(120))
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

    /// GoByBus station directory plus the origin -> destination adjacency.
    pub async fn coach_directory(
        &self,
    ) -> Result<(Vec<CoachStation>, HashMap<String, Vec<String>>)> {
        let html = self
            .get("gobybus-page", GOBYBUS_PAGE, Duration::from_secs(3600))
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

    /// Live GoByBus departures between two station ids on a date.
    pub async fn coach_trips(
        &self,
        departure: &str,
        arrival: &str,
        date: NaiveDate,
    ) -> Result<Vec<CoachTrip>> {
        let response = self
            .http
            .post("https://www.gobybus.hk/zh/getSchedule.php")
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
        parse_coach_trips(&body)
    }

    /// Marine Department cross-boundary ferry berthing board.
    pub async fn ferries(&self) -> Result<FerryReport> {
        let html = self
            .get("mardep-ferry", FERRY_PAGE, Duration::from_secs(90))
            .await?;
        let arrivals = extract_array(&html, "let arr_record = ")
            .context("Marine Department page no longer contains arr_record")?;
        let departures = extract_array(&html, "let dep_record = ")
            .context("Marine Department page no longer contains dep_record")?;
        let cft_updated = extract_js_string(&html, "let cft_update = ").unwrap_or_default();
        let mft_updated = extract_js_string(&html, "let mft_update = ").unwrap_or_default();
        let mut movements = Vec::new();
        for (direction, records) in [("arrival", arrivals), ("departure", departures)] {
            for record in records {
                let fields: Vec<String> = record
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| value.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                if fields.len() < 6 {
                    continue;
                }
                movements.push(FerryMovement {
                    time: fields[0].clone(),
                    direction: direction.to_string(),
                    port: fields[1].clone(),
                    operator: fields[2].clone(),
                    terminal: fields[3].clone(),
                    berth: fields[4].clone(),
                    status: fields[5].clone(),
                });
            }
        }
        movements.sort_by(|left, right| left.time.cmp(&right.time));
        Ok(FerryReport {
            cft_updated,
            mft_updated,
            movements,
        })
    }
}

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

/// Turn an ImmD queue integer into a readable band.
pub fn queue_label(value: Option<&Value>) -> String {
    let Some(value) = value.and_then(Value::as_i64) else {
        return "unknown".to_string();
    };
    match value {
        0 => "normal (<15 min)".to_string(),
        1 => "busy (15-30 min)".to_string(),
        2 => "very busy (30-60 min)".to_string(),
        3 => "very busy (>60 min)".to_string(),
        99 => "closed / not in service".to_string(),
        other => format!("status {other}"),
    }
}

fn parse_coach_trips(body: &str) -> Result<Vec<CoachTrip>> {
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

/// Find the JSON object that follows `marker` in an HTML page.
pub fn extract_object(html: &str, marker: &str) -> Option<Value> {
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

/// Find the JSON array that follows `marker` in an HTML page.
pub fn extract_array(html: &str, marker: &str) -> Option<Vec<Value>> {
    let start = html.find(marker)? + marker.len();
    let bytes = html.as_bytes();
    let open = start + html[start..].find('[')?;
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
            b'[' => depth += 1,
            b']' => {
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

fn extract_js_string(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let quote = rest.find(['\'', '"'])?;
    let quote_char = rest.as_bytes()[quote];
    let body = &rest[quote + 1..];
    let end = body.find(quote_char as char)?;
    Some(body[..end].to_string())
}

/// Current time in Hong Kong, RFC 3339.
pub fn now_hkt() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).expect("valid offset");
    Utc::now()
        .with_timezone(&offset)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Today in Hong Kong.
pub fn today_hkt() -> NaiveDate {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).expect("valid offset");
    Utc::now().with_timezone(&offset).date_naive()
}

/// East Rail / high-speed rail reference information.
pub mod rail {
    pub const EAST_RAIL: [(&str, &str, &str, &str, &str); 2] = [
        (
            "low",
            "Lo Wu",
            "羅湖",
            "first trains around 06:40, last around 00:00",
            "About every 3-8 minutes from Sheung Shui; cross the boundary on foot at Lo Wu",
        ),
        (
            "lmc",
            "Lok Ma Chau",
            "落馬洲",
            "first trains around 06:40, last around 22:45",
            "About every 10-12 minutes from Sheung Shui; connects to the Huanggang port shuttle",
        ),
    ];

    pub const HSR_DESTINATIONS: [(&str, &str, &str, &str, &str); 9] = [
        (
            "futian",
            "Futian",
            "福田",
            "14-18 min",
            "About every 10-20 minutes",
        ),
        (
            "shenzhen-north",
            "Shenzhen North",
            "深圳北",
            "18-25 min",
            "About every 10-20 minutes",
        ),
        (
            "guangmingcheng",
            "Guangmingcheng",
            "光明城",
            "25-35 min",
            "Regular short-haul service",
        ),
        (
            "dongguannan",
            "Dongguannan",
            "東莞南",
            "30-45 min",
            "Regular short-haul service",
        ),
        (
            "changping",
            "Changping",
            "常平",
            "40-55 min",
            "Regular short-haul service",
        ),
        (
            "dongguan",
            "Dongguan",
            "東莞",
            "40-60 min",
            "Regular short-haul service",
        ),
        (
            "guangzhou-xintang",
            "Guangzhou Xintang",
            "廣州新塘",
            "45-60 min",
            "Regular short-haul service",
        ),
        (
            "guangzhou-south",
            "Guangzhou South",
            "廣州南",
            "47-75 min",
            "Frequent short-haul service",
        ),
        (
            "guangzhou-east",
            "Guangzhou East",
            "廣州東",
            "70-90 min",
            "Regular short-haul service",
        ),
    ];

    pub const HSR_NOTE: &str = "West Kowloon departures run from about 07:00 to 23:00. MTR publishes the timetable as a PDF and 12306 is the booking authority; this skill does not invent live train times. Check the official timetable or 12306 before travelling.";
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

    #[test]
    fn extracts_js_objects() {
        let html = r#"<script>const stationMap = {"1":{"enName":"A","address":"x"},"2":{"enName":"B"}};
            depStnRouteMap = {"1":["2"]};</script>"#;
        let stations = extract_object(html, "const stationMap = ").unwrap();
        assert_eq!(stations["1"]["enName"], "A");
        let routes = extract_object(html, "depStnRouteMap = ").unwrap();
        assert_eq!(routes["1"][0], "2");
        let arr = extract_array(r#"let arr_record = [["a","b"]];"#, "let arr_record = ").unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn parses_coach_trips() {
        let body = r#"{"success":true,"data":[{"route_number":"SW+1+2+08:05",
            "dep_stn_name":"Hong Kong","arr_stn_name":"Macao","dep_time":"08:05:00",
            "arr_time":"10:05:00","adult_price":"330","currency":"HK$",
            "avail_ticket":"4","car_type":"C","opt_id":"95"}]}"#;
        let trips = parse_coach_trips(body).unwrap();
        assert_eq!(trips.len(), 1);
        assert_eq!(trips[0].price, "330");
        assert_eq!(trips[0].coach_type, "car");
    }
}
