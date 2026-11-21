//! Release lookups and settings an assistant can read (#833, #834).
//!
//! The lookups go through the same provider layer the ONLINE panel uses, so
//! they honour the proxy, the rate limit and the retries, and the Discogs token
//! or the Beatport sign-in is used here and never handed to the assistant.

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use tagrex_commands::{ErrorDto, SearchQueryDto};

use crate::ProviderState;

/// The sources an assistant may search, as the providers name them.
pub const SOURCES: [&str; 5] = ["discogs", "musicbrainz", "beatport", "bandcamp", "soundeo"];
/// The media filters a search takes.
pub const FORMATS: [&str; 4] = ["CD", "Vinyl", "LP", "File"];
/// The most results one search page asks for.
pub const MAX_PER_PAGE: u32 = 50;

fn parse<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, ErrorDto> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments)
        .map_err(|error| ErrorDto::plain(format!("invalid arguments: {error}")))
}

fn known_source(source: &str) -> Result<(), ErrorDto> {
    if SOURCES.contains(&source) {
        Ok(())
    } else {
        Err(ErrorDto::plain(format!(
            "unknown source {source:?}; use one of {}",
            SOURCES.join(", ")
        )))
    }
}

/// The credential a source needs, read inside TagRex. Never returned.
async fn token(app: &tauri::AppHandle, source: &str) -> Result<String, ErrorDto> {
    match source {
        "discogs" => {
            let token = crate::saved_discogs_token(app.clone()).await?;
            if token.is_empty() {
                return Err(ErrorDto::plain(
                    "No Discogs token is saved in TagRex. Ask the user to add one in TagRex's Settings, or search musicbrainz instead.",
                ));
            }
            Ok(token)
        }
        "beatport" => crate::beatport_token(app.clone()).await.map_err(|_| {
            ErrorDto::plain(
                "TagRex is not signed in to Beatport. Ask the user to sign in in TagRex's Settings, or use another source.",
            )
        }),
        _ => Ok(String::new()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    source: String,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    catalog_number: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    per_page: Option<u32>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The provider query for `args`, or why there is none.
fn search_query(args: SearchArgs) -> Result<(String, SearchQueryDto), ErrorDto> {
    known_source(&args.source)?;
    let format = non_empty(args.format);
    if let Some(format) = &format {
        if !FORMATS.contains(&format.as_str()) {
            return Err(ErrorDto::plain(format!(
                "unknown format {format:?}; use one of {}",
                FORMATS.join(", ")
            )));
        }
    }
    let query = SearchQueryDto {
        artist: non_empty(args.artist),
        title: non_empty(args.title),
        album: non_empty(args.album),
        catalog_number: non_empty(args.catalog_number),
        format,
        page: args.page.unwrap_or(1).max(1),
        per_page: args.per_page.unwrap_or(0).min(MAX_PER_PAGE),
    };
    if query.artist.is_none()
        && query.title.is_none()
        && query.album.is_none()
        && query.catalog_number.is_none()
    {
        return Err(ErrorDto::plain(
            "give at least one of artist, title, album or catalog_number",
        ));
    }
    Ok((args.source, query))
}

/// `search_releases`.
pub async fn search(app: &tauri::AppHandle, arguments: Value) -> Result<Value, ErrorDto> {
    let (source, query) = search_query(parse(arguments)?)?;
    let token = token(app, &source).await?;
    let page = query.page;
    let candidates =
        crate::provider_search(app.state::<ProviderState>(), source.clone(), token, query).await?;
    let results: Vec<Value> = candidates
        .iter()
        .map(|candidate| {
            json!({
                "id": candidate.id,
                "artist": candidate.artist,
                "title": candidate.title,
                "year": candidate.year,
                "label": candidate.label,
                "catalog_number": candidate.catalog_number,
                "country": candidate.country,
                "format": candidate.format,
                "score": (candidate.score * 100.0).round() / 100.0,
                "url": crate::release_url(&source, &candidate.id).ok(),
                "cover_url": candidate.cover_url,
            })
        })
        .collect();
    Ok(json!({ "source": source, "page": page, "results": results }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseArgs {
    source: String,
    release_id: String,
}

/// `read_release`.
pub async fn release(app: &tauri::AppHandle, arguments: Value) -> Result<Value, ErrorDto> {
    let args: ReleaseArgs = parse(arguments)?;
    known_source(&args.source)?;
    let id = args.release_id.trim().to_string();
    if id.is_empty() {
        return Err(ErrorDto::plain("a release_id is required"));
    }
    let token = token(app, &args.source).await?;
    let release = crate::provider_fetch_release(
        app.state::<ProviderState>(),
        args.source.clone(),
        token,
        id.clone(),
    )
    .await?;
    let mut answer = serde_json::to_value(&release).map_err(crate::shell_error)?;
    if answer["url"].is_null() {
        answer["url"] = json!(crate::release_url(&args.source, &id).ok());
    }
    answer["source"] = json!(args.source);
    Ok(answer)
}

/// `read_settings` (#834): what decides the tags and names TagRex writes.
/// Built field by field, so a setting added later, or one that can hold a
/// credential such as the proxy, is not handed out by accident.
pub async fn settings(app: &tauri::AppHandle) -> Result<Value, ErrorDto> {
    let settings = crate::read_settings(app);
    let discogs_token = !crate::saved_discogs_token(app.clone()).await?.is_empty();
    let beatport = crate::read_beatport_session(app).is_some();
    Ok(settings_view(&settings, discogs_token, beatport))
}

fn settings_view(
    settings: &tagrex_commands::SettingsDto,
    discogs_token: bool,
    beatport: bool,
) -> Value {
    let vinyl = if settings.import_vinyl_numbering.is_empty() {
        if settings.import_vinyl_position {
            "position"
        } else {
            "records"
        }
    } else {
        tagrex_commands::VinylNumbering::from_storage_key(&settings.import_vinyl_numbering)
            .to_storage_key()
    };
    let separator = if settings.multi_value_separator.is_empty() {
        "; "
    } else {
        settings.multi_value_separator.as_str()
    };
    let presets: Vec<Value> = settings
        .mask_presets
        .iter()
        .map(|preset| json!({ "name": preset.name, "mask": preset.mask }))
        .collect();
    let ready: Vec<&str> = SOURCES
        .iter()
        .copied()
        .filter(|source| match *source {
            "discogs" => discogs_token,
            "beatport" => beatport,
            _ => true,
        })
        .collect();
    json!({
        "id3v2_version": if settings.id3_v23 { "2.3" } else { "2.4" },
        "import": {
            "cover": settings.import_cover,
            "vinyl_numbering": vinyl,
            "vinyl_numbering_means": match vinyl {
                "position" => "the vinyl position (A1) is written as the track number",
                "sides" => "each side is a disc, numbered from 1 on each side",
                _ => "each record is a disc and the track number runs on across its two sides",
            },
            "skip_fields": settings.import_skip_fields,
            "genre_from": "the release styles joined with \"/\", or its genres when it has no styles",
        },
        "multi_value_separator": separator,
        "portable_names": settings.portable_names,
        "rename_carries": {
            "sidecars": settings.carry_sidecars,
            "sidecar_extensions": settings.sidecar_extensions,
            "folder_extras": settings.carry_folder_extras,
        },
        "read_priority": settings.read_priority,
        "legacy_code_page": if settings.legacy_code_page.is_empty() { "Latin-1" } else { settings.legacy_code_page.as_str() },
        "cover": {
            "max_px": settings.cover_max_px,
            "jpeg_quality": if settings.cover_quality == 0 { 85 } else { settings.cover_quality },
        },
        "bpm": {
            "detection": settings.bpm_detection,
            "range_floor": settings.bpm_floor,
            "max_minutes": settings.bpm_max_minutes,
        },
        "mask_presets": presets,
        "online": {
            "discogs_token_saved": discogs_token,
            "beatport_signed_in": beatport,
            "ready_sources": ready,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tagrex_commands::{MaskPresetDto, SettingsDto};

    #[test]
    fn a_search_needs_a_known_source_and_something_to_look_for() {
        let args = |value: Value| search_query(serde_json::from_value(value).unwrap());
        assert!(args(json!({ "source": "nowhere", "artist": "x" })).is_err());
        assert!(args(json!({ "source": "discogs", "artist": "  " })).is_err());
        assert!(args(json!({ "source": "discogs", "artist": "x", "format": "8-track" })).is_err());
        let (source, query) = args(
            json!({ "source": "musicbrainz", "catalog_number": " CD NTH 8 ", "per_page": 500 }),
        )
        .unwrap();
        assert_eq!(source, "musicbrainz");
        assert_eq!(query.catalog_number.as_deref(), Some("CD NTH 8"));
        assert_eq!((query.page, query.per_page), (1, MAX_PER_PAGE));
    }

    #[test]
    fn settings_leave_the_connection_out_and_resolve_defaults() {
        let settings = SettingsDto {
            proxy: "http://user:secret@proxy.example:8080".into(),
            import_vinyl_position: true,
            mask_presets: vec![MaskPresetDto {
                name: "By catalogue".into(),
                mask: "[%catalognumber%] %album%".into(),
            }],
            ..SettingsDto::default()
        };
        let view = settings_view(&settings, false, true);
        let text = view.to_string();
        assert!(!text.contains("secret") && !text.contains("proxy.example"));
        assert_eq!(view["import"]["vinyl_numbering"], "position");
        assert_eq!(view["multi_value_separator"], "; ");
        assert_eq!(view["id3v2_version"], "2.4");
        assert_eq!(view["mask_presets"][0]["mask"], "[%catalognumber%] %album%");
        assert_eq!(
            view["online"]["ready_sources"],
            json!(["musicbrainz", "beatport", "bandcamp", "soundeo"])
        );
    }
}
