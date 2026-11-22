//! Cross-border models and source orchestration.
//!
//! Border queues, coaches, ferries and HSR each live in `src/sources/` behind a
//! Cargo feature. [`rail`] holds East Rail reference data.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, Utc};
use serde::Serialize;

/// One land boundary control point.
#[derive(Debug, Clone, Serialize)]
pub struct BorderPoint {
    pub code: String,
    pub name: String,
    pub name_zh: String,
    pub resident_arrival: String,
    pub resident_departure: String,
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
    // Read by the compiled-in source modules.
    pub(crate) http: reqwest::Client,
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

    #[allow(dead_code)]
    pub(crate) async fn get(&self, key: &str, url: &str, ttl: Duration) -> Result<String> {
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
        #[cfg(feature = "source-border")]
        {
            return crate::sources::border::borders(self).await;
        }
        #[cfg(not(feature = "source-border"))]
        bail!("Immigration Department source is not compiled into this binary")
    }

    /// MTR's official HSR timetable from West Kowloon.
    pub async fn hsr_timetable(&self, destination: &str) -> Result<HsrTimetable> {
        #[cfg(feature = "source-hsr")]
        {
            return crate::sources::hsr::timetable(self, destination).await;
        }
        #[cfg(not(feature = "source-hsr"))]
        {
            let _ = destination;
            bail!("HSR source is not compiled into this binary")
        }
    }

    /// GoByBus station directory plus the origin -> destination adjacency.
    pub async fn coach_directory(
        &self,
    ) -> Result<(Vec<CoachStation>, HashMap<String, Vec<String>>)> {
        #[cfg(feature = "source-coach")]
        {
            return crate::sources::coach::directory(self).await;
        }
        #[cfg(not(feature = "source-coach"))]
        bail!("GoByBus source is not compiled into this binary")
    }

    /// Live GoByBus departures between two station ids on a date.
    pub async fn coach_trips(
        &self,
        departure: &str,
        arrival: &str,
        date: NaiveDate,
    ) -> Result<Vec<CoachTrip>> {
        #[cfg(feature = "source-coach")]
        {
            return crate::sources::coach::trips(self, departure, arrival, date).await;
        }
        #[cfg(not(feature = "source-coach"))]
        {
            let _ = (departure, arrival, date);
            bail!("GoByBus source is not compiled into this binary")
        }
    }

    /// Marine Department cross-boundary ferry berthing board.
    pub async fn ferries(&self) -> Result<FerryReport> {
        #[cfg(feature = "source-ferry")]
        {
            return crate::sources::ferry::ferries(self).await;
        }
        #[cfg(not(feature = "source-ferry"))]
        bail!("Marine Department source is not compiled into this binary")
    }
}

/// Current time in Hong Kong, RFC 3339.
#[allow(dead_code)]
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

/// East Rail reference information.
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
