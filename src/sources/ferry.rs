//! Marine Department cross-boundary ferry board.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::api::{Client, FerryMovement, FerryReport};

const PAGE: &str = "https://crossboundaryferryservices.mardep.gov.hk/en/";

pub async fn ferries(client: &Client) -> Result<FerryReport> {
    let html = client
        .get("mardep-ferry", PAGE, Duration::from_secs(90))
        .await?;
    let arrivals = extract_array(&html, "let arr_record = ")
        .context("no arr_record on Marine Department page")?;
    let departures = extract_array(&html, "let dep_record = ")
        .context("no dep_record on Marine Department page")?;
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

fn extract_array(html: &str, marker: &str) -> Option<Vec<Value>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_js_arrays() {
        let arr = extract_array(r#"let arr_record = [["a","b"]];"#, "let arr_record = ").unwrap();
        assert_eq!(arr.len(), 1);
    }
}
