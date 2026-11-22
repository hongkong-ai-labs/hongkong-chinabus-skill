//! MTR high-speed rail timetable.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::api::{Client, HsrDeparture, HsrStop, HsrTimetable};

const TIMETABLE: &str =
    "https://www.highspeed.mtr.com.hk/res/content/app/XRL_content_Timetable.json";
const STATIONS: &str = "https://www.highspeed.mtr.com.hk/res/content/XRL_content_TripPlanning.json";

pub async fn timetable(client: &Client, destination: &str) -> Result<HsrTimetable> {
    let (timetable, stations) = tokio::try_join!(
        client.get("hsr-timetable", TIMETABLE, Duration::from_secs(6 * 3600)),
        client.get("hsr-stations", STATIONS, Duration::from_secs(6 * 3600))
    )?;
    let timetable: Value = serde_json::from_str(timetable.trim_start_matches('\u{feff}'))
        .context("decoding MTR HSR timetable")?;
    let stations: Value = serde_json::from_str(stations.trim_start_matches('\u{feff}'))
        .context("decoding MTR HSR stations")?;
    parse_timetable(&timetable, &stations, destination)
}

fn clean_station_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn friendly_name(code: &str) -> Option<&'static str> {
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

fn station_map(stations: &Value) -> HashMap<String, (String, String)> {
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

fn parse_timetable(timetable: &Value, stations: &Value, destination: &str) -> Result<HsrTimetable> {
    let names = station_map(stations);
    let filter = if destination.eq_ignore_ascii_case("all") {
        None
    } else {
        let wanted = normalized_name(destination);
        let exact: Vec<String> = names
            .iter()
            .filter(|(code, (name, _))| {
                code.eq_ignore_ascii_case(destination)
                    || normalized_name(name) == wanted
                    || friendly_name(code)
                        .is_some_and(|friendly| normalized_name(friendly) == wanted)
            })
            .map(|(code, _)| code.clone())
            .collect::<Vec<_>>();
        let matches = if exact.is_empty() {
            names
                .iter()
                .filter(|(code, (name, _))| {
                    normalized_name(name).contains(&wanted)
                        || friendly_name(code)
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
            friendly_name(code)
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
        let duration = minutes(departs, arrives)
            .with_context(|| format!("invalid HSR times {departs}-{arrives} for {train:?}"))?;
        departures.push(HsrDeparture {
            train: train["id"].as_str().unwrap_or_default().to_string(),
            destination: friendly_name(code)
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

fn minutes(departs: &str, arrives: &str) -> Option<i64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timetable_and_resolves_names() {
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
        let report = parse_timetable(&timetable, &stations, "Shenzhen North").unwrap();
        assert_eq!(report.destination, "Shenzhen North");
        assert_eq!(report.departures.len(), 1);
        let departure = &report.departures[0];
        assert_eq!(departure.train, "G1");
        assert_eq!(departure.departs, "07:01");
        assert_eq!(departure.arrives, "07:19");
        assert_eq!(departure.duration_minutes, 18);
        assert_eq!(departure.via[0].name, "Futian");
        assert_eq!(
            parse_timetable(&timetable, &stations, "all")
                .unwrap()
                .departures
                .len(),
            1
        );
        assert!(parse_timetable(&timetable, &stations, "nowhere").is_err());
    }
}
