//! Minimal MCP server over stdio (JSON-RPC 2.0, one message per line).

use anyhow::Result;
use chrono::NaiveDate;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::{Client, today_hkt};
use crate::render;

pub async fn serve(client: Client) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if request.get("id").is_none() {
            continue;
        }
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let response = match method {
            "initialize" => ok(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {
                        "name": "hongkong-chinabus",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            ),
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, json!({"tools": tools()})),
            "tools/call" => match call_tool(&client, &request).await {
                Ok(text) => ok(
                    id,
                    json!({"content": [{"type": "text", "text": text}], "isError": false}),
                ),
                Err(error) => ok(
                    id,
                    json!({
                        "content": [{"type": "text", "text": format!("{error:#}")}],
                        "isError": true
                    }),
                ),
            },
            other => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("method not found: {other}")}
            }),
        };
        stdout
            .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
            .await?;
        stdout.flush().await?;
    }
    Ok(())
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn tools() -> Value {
    json!([
        {
            "name": "border_wait_times",
            "description": "Live Immigration Department queue times at Hong Kong land boundary control points (Lo Wu, Lok Ma Chau, Shenzhen Bay, HZMB, Heung Yuen Wai, Huanggang, Man Kam To, Sha Tau Kok).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "point": {"type": "string", "description": "Optional control point name or code, e.g. SBC or Shenzhen Bay."}
                }
            }
        },
        {
            "name": "search_cross_border_coaches",
            "description": "Live GoByBus cross-border coach departures between Hong Kong and mainland China or Macao, including prices and seats. Resolves station names such as 'China HK City' or 'MGM Cotai'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {"type": "string", "description": "Boarding point name or GoByBus station id."},
                    "to": {"type": "string", "description": "Alighting point name or GoByBus station id."},
                    "date": {"type": "string", "description": "Travel date YYYY-MM-DD (Hong Kong time). Defaults to today."},
                    "limit": {"type": "number", "description": "Maximum departures to return (default 20)."}
                },
                "required": ["from", "to"]
            }
        },
        {
            "name": "cross_border_ferry_board",
            "description": "Live Marine Department berthing board for ferries between Hong Kong (China Ferry Terminal, Macau Ferry Terminal) and mainland China / Macao ports.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "port": {"type": "string", "description": "Filter by port or operator, e.g. Macao, Shekou, Zhuhai."},
                    "terminal": {"type": "string", "description": "cft (China Ferry Terminal) or mft (Macau Ferry Terminal)."},
                    "direction": {"type": "string", "description": "arrival or departure."}
                }
            }
        },
        {
            "name": "cross_border_rail_info",
            "description": "East Rail service to Lo Wu and Lok Ma Chau, and high-speed rail destinations from West Kowloon to Shenzhen and Guangzhou. Returns published service patterns, not live train times.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["east-rail", "hsr"], "description": "Rail mode."},
                    "destination": {"type": "string", "description": "Station or destination slug/name; defaults to all."}
                },
                "required": ["mode"]
            }
        }
    ])
}

async fn call_tool(client: &Client, request: &Value) -> Result<String> {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arg = |key: &str| args.get(key).and_then(Value::as_str).map(ToOwned::to_owned);

    match name {
        "border_wait_times" => {
            let mut report = client.borders().await?;
            if let Some(filter) = arg("point") {
                let needle = filter.to_lowercase();
                report.points.retain(|item| {
                    item.code.to_lowercase() == needle
                        || item.name.to_lowercase().contains(&needle)
                        || item.name_zh.contains(&filter)
                });
            }
            Ok(render::borders(&report))
        }
        "search_cross_border_coaches" => {
            let from = arg("from").ok_or_else(|| anyhow::anyhow!("from is required"))?;
            let to = arg("to").ok_or_else(|| anyhow::anyhow!("to is required"))?;
            let (stations, adjacency) = client.coach_directory().await?;
            let origin = resolve(&stations, &adjacency, &from)?;
            let ids = adjacency
                .get(&origin.id)
                .ok_or_else(|| anyhow::anyhow!("{} has no cross-border departures", origin.name))?;
            let destinations: Vec<_> = ids
                .iter()
                .filter_map(|id| stations.iter().find(|station| &station.id == id).cloned())
                .collect();
            let arrival = resolve(&destinations, &adjacency, &to)?;
            let day = match arg("date") {
                Some(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")?,
                None => today_hkt(),
            };
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let mut trips = client.coach_trips(&origin.id, &arrival.id, day).await?;
            trips.sort_by(|left, right| left.depart.cmp(&right.depart));
            trips.truncate(limit);
            Ok(render::coach_trips(
                &origin,
                &arrival,
                &day.to_string(),
                &trips,
            ))
        }
        "cross_border_ferry_board" => {
            let mut report = client.ferries().await?;
            if let Some(filter) = arg("port") {
                let needle = filter.to_lowercase();
                report.movements.retain(|item| {
                    item.port.to_lowercase().contains(&needle)
                        || item.operator.to_lowercase().contains(&needle)
                });
            }
            if let Some(filter) = arg("terminal") {
                let needle = filter.to_lowercase();
                let code = match needle.as_str() {
                    "cft" | "china" => "CFT".to_string(),
                    "mft" | "macau" | "macao" => "MFT".to_string(),
                    other => other.to_uppercase(),
                };
                report
                    .movements
                    .retain(|item| item.terminal.eq_ignore_ascii_case(&code));
            }
            if let Some(filter) = arg("direction") {
                report.movements.retain(|item| {
                    item.direction
                        .to_lowercase()
                        .starts_with(&filter.to_lowercase())
                });
            }
            Ok(render::ferries(&report))
        }
        "cross_border_rail_info" => match arg("mode").as_deref() {
            Some("east-rail") => Ok(render::east_rail(
                &arg("destination").unwrap_or_else(|| "all".to_string()),
            )),
            Some("hsr") => Ok(render::hsr(
                &arg("destination").unwrap_or_else(|| "all".to_string()),
            )),
            other => anyhow::bail!("mode must be east-rail or hsr, got {other:?}"),
        },
        other => anyhow::bail!("unknown tool: {other}"),
    }
}

fn resolve(
    stations: &[crate::api::CoachStation],
    adjacency: &std::collections::HashMap<String, Vec<String>>,
    query: &str,
) -> Result<crate::api::CoachStation> {
    if let Some(station) = stations.iter().find(|station| station.id == query) {
        return Ok(station.clone());
    }
    let needle = query.to_lowercase();
    let exact: Vec<&crate::api::CoachStation> = stations
        .iter()
        .filter(|station| station.name.to_lowercase() == needle)
        .collect();
    if !exact.is_empty() {
        return Ok(exact
            .into_iter()
            .max_by_key(|station| adjacency.get(&station.id).map(Vec::len).unwrap_or(0))
            .cloned()
            .expect("exact match is not empty"));
    }
    let mut matches: Vec<&crate::api::CoachStation> = stations
        .iter()
        .filter(|station| {
            station.name.to_lowercase().contains(&needle)
                || station.address.to_lowercase().contains(&needle)
        })
        .collect();
    matches.sort_by_key(|station| {
        (
            !station.name.to_lowercase().starts_with(&needle),
            station.name.len(),
        )
    });
    match matches.len() {
        0 => anyhow::bail!("no GoByBus station matches {query:?}"),
        1 => Ok(matches[0].clone()),
        _ => {
            let options: Vec<String> = matches
                .iter()
                .take(8)
                .map(|station| format!("{} ({})", station.name, station.id))
                .collect();
            anyhow::bail!(
                "{query:?} matches several stations: {}. Use a station id.",
                options.join(", ")
            )
        }
    }
}
