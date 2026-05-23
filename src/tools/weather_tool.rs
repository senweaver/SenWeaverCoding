// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

const WTTR_BASE_URL: &str = "https://wttr.in";
const WTTR_TIMEOUT_SECS: u64 = 15;
const WTTR_CONNECT_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Deserialize)]
struct WttrResponse {
    current_condition: Vec<CurrentCondition>,
    nearest_area: Vec<NearestArea>,
    weather: Vec<WeatherDay>,
}

#[derive(Debug, Deserialize)]
struct CurrentCondition {
    #[serde(rename = "temp_C")]
    temp_c: String,
    #[serde(rename = "temp_F")]
    temp_f: String,
    #[serde(rename = "FeelsLikeC")]
    feels_like_c: String,
    #[serde(rename = "FeelsLikeF")]
    feels_like_f: String,
    humidity: String,
    #[serde(rename = "weatherDesc")]
    weather_desc: Vec<StringValue>,
    #[serde(rename = "windspeedKmph")]
    windspeed_kmph: String,
    #[serde(rename = "windspeedMiles")]
    windspeed_miles: String,
    #[serde(rename = "winddir16Point")]
    winddir_16point: String,
    #[serde(rename = "precipMM")]
    precip_mm: String,
    #[serde(rename = "precipInches")]
    precip_inches: String,
    visibility: String,
    #[serde(rename = "visibilityMiles")]
    visibility_miles: String,
    #[serde(rename = "uvIndex")]
    uv_index: String,
    #[serde(rename = "cloudcover")]
    cloud_cover: String,
    #[serde(rename = "pressure")]
    pressure_mb: String,
    #[serde(rename = "pressureInches")]
    pressure_inches: String,
    #[serde(rename = "observation_time")]
    observation_time: String,
}

#[derive(Debug, Deserialize)]
struct NearestArea {
    #[serde(rename = "areaName")]
    area_name: Vec<StringValue>,
    country: Vec<StringValue>,
    region: Vec<StringValue>,
}

#[derive(Debug, Deserialize)]
struct WeatherDay {
    date: String,
    #[serde(rename = "maxtempC")]
    max_temp_c: String,
    #[serde(rename = "maxtempF")]
    max_temp_f: String,
    #[serde(rename = "mintempC")]
    min_temp_c: String,
    #[serde(rename = "mintempF")]
    min_temp_f: String,
    #[serde(rename = "avgtempC")]
    avg_temp_c: String,
    #[serde(rename = "avgtempF")]
    avg_temp_f: String,
    #[serde(rename = "sunHour")]
    sun_hours: String,
    #[serde(rename = "uvIndex")]
    uv_index: String,
    #[serde(rename = "totalSnow_cm")]
    total_snow_cm: String,
    astronomy: Vec<Astronomy>,
    hourly: Vec<HourlyCondition>,
}

#[derive(Debug, Deserialize)]
struct Astronomy {
    sunrise: String,
    sunset: String,
    moon_phase: String,
}

#[derive(Debug, Deserialize)]
struct HourlyCondition {
    time: String,
    #[serde(rename = "tempC")]
    temp_c: String,
    #[serde(rename = "tempF")]
    temp_f: String,
    #[serde(rename = "weatherDesc")]
    weather_desc: Vec<StringValue>,
    #[serde(rename = "chanceofrain")]
    chance_of_rain: String,
    #[serde(rename = "chanceofsnow")]
    chance_of_snow: String,
    #[serde(rename = "windspeedKmph")]
    windspeed_kmph: String,
    #[serde(rename = "windspeedMiles")]
    windspeed_miles: String,
    #[serde(rename = "winddir16Point")]
    winddir_16point: String,
}

#[derive(Debug, Deserialize)]
struct StringValue {
    value: String,
}

pub struct WeatherTool;

impl WeatherTool {
    pub fn new() -> Self {
        Self
    }

    fn build_url(location: &str) -> String {

        let encoded = location.trim().replace(' ', "+");
        format!("{WTTR_BASE_URL}/{encoded}?format=j1")
    }

    async fn fetch(location: &str) -> anyhow::Result<WttrResponse> {
        let url = Self::build_url(location);

        let builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(WTTR_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(WTTR_CONNECT_TIMEOUT_SECS))
            .user_agent("sen-weather/1.0");

        let builder = crate::services::get_services()
            .proxy_runtime()
            .apply_to_builder(builder, "tool.weather");
        let client = builder.build()?;

        let response = client.get(&url).send().await?;
        let status = response.status();

        if !status.is_success() {
            anyhow::bail!(
                "wttr.in returned HTTP {status} for location '{location}'. \
                 Check that the location is valid."
            );
        }

        let body = response.text().await?;

        if !body.trim_start().starts_with('{') {
            anyhow::bail!(
                "wttr.in could not resolve location '{location}'. \
                 Try a city name, airport code, GPS coordinates (lat,lon), or zip code."
            );
        }

        let parsed: WttrResponse = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse wttr.in response: {e}"))?;

        Ok(parsed)
    }

    fn format_hourly(h: &HourlyCondition, metric: bool) -> String {

        let hour_num: u32 = h.time.parse().unwrap_or(0);
        let hour_display = format!("{:02}:00", hour_num / 100);
        let temp = if metric {
            format!("{}°C", h.temp_c)
        } else {
            format!("{}°F", h.temp_f)
        };
        let wind_speed = if metric {
            format!("{} km/h", h.windspeed_kmph)
        } else {
            format!("{} mph", h.windspeed_miles)
        };
        let desc = h
            .weather_desc
            .first()
            .map(|v| v.value.trim().to_string())
            .unwrap_or_default();
        format!(
            "    {hour_display}: {temp} — {desc} | Wind: {wind_speed} {} | Rain: {}% | Snow: {}%",
            h.winddir_16point, h.chance_of_rain, h.chance_of_snow,
        )
    }

    fn format_day(day: &WeatherDay, metric: bool, include_hourly: bool) -> String {
        let (max, min, avg) = if metric {
            (
                format!("{}°C", day.max_temp_c),
                format!("{}°C", day.min_temp_c),
                format!("{}°C", day.avg_temp_c),
            )
        } else {
            (
                format!("{}°F", day.max_temp_f),
                format!("{}°F", day.min_temp_f),
                format!("{}°F", day.avg_temp_f),
            )
        };

        let astronomy = day.astronomy.first();
        let sunrise = astronomy.map(|a| a.sunrise.as_str()).unwrap_or("N/A");
        let sunset = astronomy.map(|a| a.sunset.as_str()).unwrap_or("N/A");
        let moon = astronomy.map(|a| a.moon_phase.as_str()).unwrap_or("N/A");

        let snow_note = if day.total_snow_cm != "0.0" && day.total_snow_cm != "0" {
            let snow_str = if metric {
                format!(" | Snow: {} cm", day.total_snow_cm)
            } else {

                let cm: f64 = day.total_snow_cm.parse().unwrap_or(0.0);
                format!(" | Snow: {:.1} in", cm / 2.54)
            };
            snow_str
        } else {
            String::new()
        };

        let mut out = format!(
            "  {date}: High {max} / Low {min} / Avg {avg} | UV: {uv} | Sun: {sun_hours}h | {snow}\
             Sunrise: {sunrise} | Sunset: {sunset} | Moon: {moon}",
            date = day.date,
            uv = day.uv_index,
            sun_hours = day.sun_hours,
            snow = snow_note,
        );

        if include_hourly && !day.hourly.is_empty() {
            out.push('\n');

            for h in day.hourly.iter().step_by(2) {
                out.push('\n');
                out.push_str(&Self::format_hourly(h, metric));
            }
        }

        out
    }

    fn format_output(data: &WttrResponse, metric: bool, days: u8) -> String {
        let current = match data.current_condition.first() {
            Some(c) => c,
            None => return "No current conditions available.".to_string(),
        };

        let area = data.nearest_area.first();
        let location_str = area
            .map(|a| {
                let city = a.area_name.first().map(|v| v.value.as_str()).unwrap_or("");
                let region = a.region.first().map(|v| v.value.as_str()).unwrap_or("");
                let country = a.country.first().map(|v| v.value.as_str()).unwrap_or("");
                match (city.is_empty(), region.is_empty()) {
                    (false, false) => format!("{city}, {region}, {country}"),
                    (false, true) => format!("{city}, {country}"),
                    _ => country.to_string(),
                }
            })
            .unwrap_or_else(|| "Unknown location".to_string());

        let desc = current
            .weather_desc
            .first()
            .map(|v| v.value.trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let (temp, feels_like, wind_speed, precip, visibility, pressure) = if metric {
            (
                format!("{}°C", current.temp_c),
                format!("{}°C", current.feels_like_c),
                format!("{} km/h", current.windspeed_kmph),
                format!("{} mm", current.precip_mm),
                format!("{} km", current.visibility),
                format!("{} hPa", current.pressure_mb),
            )
        } else {
            (
                format!("{}°F", current.temp_f),
                format!("{}°F", current.feels_like_f),
                format!("{} mph", current.windspeed_miles),
                format!("{} in", current.precip_inches),
                format!("{} mi", current.visibility_miles),
                format!("{} inHg", current.pressure_inches),
            )
        };

        let mut out = format!(
            "Weather for {location_str} (as of {obs_time})\n\
             ─────────────────────────────────────────\n\
             Conditions : {desc}\n\
             Temperature: {temp} (feels like {feels_like})\n\
             Humidity   : {humidity}%\n\
             Wind       : {wind_speed} {winddir}\n\
             Precipitation: {precip}\n\
             Visibility : {visibility}\n\
             Pressure   : {pressure}\n\
             Cloud Cover: {cloud}%\n\
             UV Index   : {uv}",
            obs_time = current.observation_time,
            humidity = current.humidity,
            winddir = current.winddir_16point,
            cloud = current.cloud_cover,
            uv = current.uv_index,
        );

        let forecast_days: Vec<&WeatherDay> = data.weather.iter().take(days as usize).collect();
        if !forecast_days.is_empty() {
            out.push_str("\n\nForecast\n────────");
            let include_hourly = days <= 2;
            for day in &forecast_days {
                out.push('\n');
                out.push_str(&Self::format_day(day, metric, include_hourly));
            }
        }

        out
    }
}

impl Default for WeatherTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WeatherTool {
    fn name(&self) -> &str {
        "weather"
    }

    fn description(&self) -> &str {
        "Get current weather conditions and up to 3-day forecast for any location worldwide. \
         Supports city names (in any language or script), airport IATA codes (e.g. 'LAX'), \
         GPS coordinates (e.g. '51.5,-0.1'), postal/zip codes, and domain-based geolocation. \
         No API key required. Units default to metric (°C, km/h, mm) but can be switched to \
         imperial (°F, mph, inches) per request."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "Location to get weather for. Accepts city names in any \
                                    language/script, IATA airport codes, GPS coordinates \
                                    (e.g. '35.6762,139.6503'), postal/zip codes, or a \
                                    domain name for geolocation (e.g. 'stackoverflow.com')."
                },
                "units": {
                    "type": "string",
                    "enum": ["metric", "imperial"],
                    "description": "Unit system. 'metric' = °C, km/h, mm (default). \
                                    'imperial' = °F, mph, inches."
                },
                "days": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Number of forecast days to include (0–3). \
                                    0 returns current conditions only. Default: 1."
                }
            },
            "required": ["location"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let location = match args.get("location").and_then(|v| v.as_str()) {
            Some(loc) if !loc.trim().is_empty() => loc.trim().to_string(),
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing required parameter 'location'".into()),
                });
            }
        };

        let metric = args
            .get("units")
            .and_then(|v| v.as_str())
            .map(|u| u.to_lowercase() != "imperial")
            .unwrap_or(true);

        let days: u8 = args
            .get("days")
            .and_then(|v| v.as_u64())
            .map(|d| d.min(3) as u8)
            .unwrap_or(1);

        match Self::fetch(&location).await {
            Ok(data) => {
                let output = Self::format_output(&data, metric, days);
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}

