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
const HSR_TIMETABLE: &str =
    "https://www.highspeed.mtr.com.hk/res/content/app/XRL_content_Timetable.json";
const HSR_STATIONS: &str =
    "https://www.highspeed.mtr.com.hk/res/content/XRL_content_TripPlanning.json";

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

/// One stop on an HSR service.
#[derive(Debug, Clone, Serialize)]
pub struct HsrStop {
    pub code: String,
    pub name: String,
    pub name_zh: String,
    pub time: String,
}

/// One scheduled HSR departure from West Kowloon.
#[derive(Debug, Clone, Serialize)]
pub struct HsrDeparture {
    pub train: String,
    pub destination: String,
    pub destination_zh: String,
    pub departs: String,
    pub arrives: String,
    pub duration_minutes: i64,
    pub via: Vec<HsrStop>,
    pub remarks: String,
}

/// Official MTR scheduled timetable, not a live feed.
#[derive(Debug, Clone, Serialize)]
pub struct HsrTimetable {
    pub effective_from: String,
    pub effective_to: String,
    pub destination: String,
    pub destination_zh: String,
    pub source: String,
    pub departures: Vec<HsrDeparture>,
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

    /// MTR's official HSR timetable, filtered to trains from West Kowloon
    /// that call at the requested destination.
    pub async fn hsr_timetable(&self, destination: &str) -> Result<HsrTimetable> {
        let (timetable, stations) = tokio::try_join!(
            self.get(
                "hsr-timetable",
                HSR_TIMETABLE,
                Duration::from_secs(6 * 3600)
            ),
            self.get("hsr-stations", HSR_STATIONS, Duration::from_secs(6 * 3600))
        )?;
        let timetable: Value = serde_json::from_str(timetable.trim_start_matches('\u{feff}'))
            .context("decoding MTR HSR timetable")?;
        let stations: Value = serde_json::from_str(stations.trim_start_matches('\u{feff}'))
            .context("decoding MTR HSR stations")?;
        parse_hsr_timetable(&timetable, &stations, destination)
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

fn clean_station_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hsr_friendly_name(code: &str) -> Option<&'static str> {
    match code {
        "WEK" => Some("Hong Kong West Kowloon"),
        "SZB" => Some("Shenzhen North"),
        "QIS" => Some("Nansha North"),
        "XWQ" => Some("Guangzhou Xintang"),
        "GZN" => Some("Guangzhou South"),
        "GZD" => Some("Guangzhou East"),
        _ => None,
    }
}

fn normalized_name(raw: &str) -> String {
    raw.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn hsr_station_map(stations: &Value) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    for station in stations["station"].as_array().into_iter().flatten() {
        let Some(code) = station["station_id"].as_str() else {
            continue;
        };
        let name = clean_station_name(station["station_title_en"].as_str().unwrap_or_default());
        let name_zh = clean_station_name(
            station["station_title_tc"]
                .as_str()
                .or_else(|| station["station_title_sc"].as_str())
                .unwrap_or_default(),
        );
        map.insert(code.to_string(), (name, name_zh));
    }
    map
}

fn parse_hsr_timetable(
    timetable: &Value,
    stations: &Value,
    destination: &str,
) -> Result<HsrTimetable> {
    let names = hsr_station_map(stations);
    let filter = if destination.eq_ignore_ascii_case("all") {
        None
    } else {
        let wanted = normalized_name(destination);
        let exact: Vec<String> = names
            .iter()
            .filter(|(code, (name, _))| {
                code.eq_ignore_ascii_case(destination)
                    || normalized_name(name) == wanted
                    || hsr_friendly_name(code)
                        .is_some_and(|friendly| normalized_name(friendly) == wanted)
            })
            .map(|(code, _)| code.clone())
            .collect::<Vec<_>>();
        let matches = if exact.is_empty() {
            names
                .iter()
                .filter(|(code, (name, _))| {
                    normalized_name(name).contains(&wanted)
                        || hsr_friendly_name(code)
                            .is_some_and(|friendly| normalized_name(friendly).contains(&wanted))
                })
                .map(|(code, _)| code.clone())
                .collect::<Vec<_>>()
        } else {
            exact
        };
        match matches.as_slice() {
            [code] => Some(code.clone()),
            [] => bail!(
                "no HSR station matches {destination:?}; try Futian, Shenzhen North, Guangzhou South or Guangzhou East"
            ),
            many => {
                let mut options = many
                    .iter()
                    .filter_map(|code| names.get(code).map(|(name, _)| name.as_str()))
                    .collect::<Vec<_>>();
                options.sort_unstable();
                bail!(
                    "{destination:?} matches several HSR stations: {}",
                    options.join(", ")
                );
            }
        }
    };

    let (destination_name, destination_zh) = match &filter {
        Some(code) => (
            hsr_friendly_name(code)
                .map(str::to_string)
                .or_else(|| names.get(code).map(|(name, _)| name.clone()))
                .unwrap_or_else(|| code.to_string()),
            names
                .get(code)
                .map(|(_, name_zh)| name_zh.clone())
                .unwrap_or_default(),
        ),
        None => ("All destinations".to_string(), "所有目的地".to_string()),
    };

    let mut departures = Vec::new();
    for entry in timetable["routes"].as_array().into_iter().flatten() {
        let Some(train) = entry.as_object().and_then(|object| object.values().next()) else {
            continue;
        };
        if train["start_station_code"].as_str() != Some("WEK") {
            continue;
        }
        let mut stops: Vec<(String, String)> = train["routestation"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(code, stop)| Some((code.clone(), stop["time"].as_str()?.to_string())))
            .collect();
        stops.sort_by(|left, right| left.1.cmp(&right.1));
        let Some((_, departs)) = stops.first() else {
            continue;
        };
        let destination_index = match &filter {
            Some(code) => stops.iter().position(|(stop, _)| stop == code),
            None => stops.len().checked_sub(1),
        };
        let Some(destination_index) = destination_index.filter(|index| *index > 0) else {
            continue;
        };
        let (code, arrives) = &stops[destination_index];
        let via = stops[1..destination_index]
            .iter()
            .map(|(stop, time)| {
                let (name, name_zh) = names
                    .get(stop)
                    .cloned()
                    .unwrap_or_else(|| (stop.clone(), String::new()));
                HsrStop {
                    code: stop.clone(),
                    name,
                    name_zh,
                    time: time.clone(),
                }
            })
            .collect();
        let duration = hsr_minutes(departs, arrives)
            .with_context(|| format!("invalid HSR times {departs}-{arrives} for {train:?}"))?;
        departures.push(HsrDeparture {
            train: train["id"].as_str().unwrap_or_default().to_string(),
            destination: hsr_friendly_name(code)
                .map(str::to_string)
                .or_else(|| names.get(code).map(|(name, _)| name.clone()))
                .unwrap_or_else(|| code.clone()),
            destination_zh: names
                .get(code)
                .map(|(_, name_zh)| name_zh.clone())
                .unwrap_or_default(),
            departs: departs.clone(),
            arrives: arrives.clone(),
            duration_minutes: duration,
            via,
            remarks: train["remarks_EN"].as_str().unwrap_or_default().to_string(),
        });
    }
    departures.sort_by(|left, right| left.departs.cmp(&right.departs));
    Ok(HsrTimetable {
        effective_from: timetable["effective_from"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        effective_to: timetable["effective_to"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        destination: destination_name,
        destination_zh,
        source: "https://www.highspeed.mtr.com.hk/en/ticket/timetable.html".to_string(),
        departures,
    })
}

fn hsr_minutes(departs: &str, arrives: &str) -> Option<i64> {
    let parse = |value: &str| {
        let (hour, minute) = value.split_once(':')?;
        Some(hour.parse::<i64>().ok()? * 60 + minute.parse::<i64>().ok()?)
    };
    let mut duration = parse(arrives)? - parse(departs)?;
    if duration < 0 {
        duration += 24 * 60;
    }
    Some(duration)
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
    fn parses_hsr_timetable_and_resolves_names() {
        let stations = serde_json::json!({
            "station": [
                {"station_id": "WEK", "station_title_en": "Hong Kong\n West Kowloon", "station_title_tc": "香港西九龍"},
                {"station_id": "FUT", "station_title_en": "Futian", "station_title_tc": "福田"},
                {"station_id": "SZB", "station_title_en": "Shenzhenbei", "station_title_tc": "深圳北"}
            ]
        });
        let timetable = serde_json::json!({
            "effective_from": "11/10/2026",
            "effective_to": "11/01/2027",
            "routes": [
                {"G1": {"id": "G1", "start_station_code": "WEK", "end_station_code": "SZB", "start_time": "07:01", "end_time": "07:19", "remarks_EN": "", "routestation": {"WEK": {"time": "07:01"}, "FUT": {"time": "07:11"}, "SZB": {"time": "07:19"}}}},
                {"G2": {"id": "G2", "start_station_code": "SZB", "end_station_code": "WEK", "start_time": "08:00", "end_time": "08:18", "remarks_EN": "", "routestation": {"SZB": {"time": "08:00"}, "WEK": {"time": "08:18"}}}}
            ]
        });
        let report = parse_hsr_timetable(&timetable, &stations, "Shenzhen North").unwrap();
        assert_eq!(report.destination, "Shenzhen North");
        assert_eq!(report.departures.len(), 1);
        let departure = &report.departures[0];
        assert_eq!(departure.train, "G1");
        assert_eq!(departure.departs, "07:01");
        assert_eq!(departure.arrives, "07:19");
        assert_eq!(departure.duration_minutes, 18);
        assert_eq!(departure.via[0].name, "Futian");
        assert_eq!(
            parse_hsr_timetable(&timetable, &stations, "all")
                .unwrap()
                .departures
                .len(),
            1
        );
        assert!(parse_hsr_timetable(&timetable, &stations, "nowhere").is_err());
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
