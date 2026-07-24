//! Manual check against the real MusicBrainz API. No token needed.
//!
//! Usage:
//!   cargo run -p tagrex-providers-musicbrainz \
//!     --example musicbrainz_search -- "album or query"
//!
//! Prints the top search hits, then fetches and prints the first release's
//! tracklist, genres, and Cover Art Archive URL. Read-only; it only issues GET
//! requests. MusicBrainz asks clients to stay under ~1 req/s — this makes just
//! two requests, well within etiquette.

use tagrex_core::provider::{MetadataProvider, SearchQuery};
use tagrex_providers_musicbrainz::MusicBrainzProvider;

fn main() {
    let Some(query_text) = std::env::args().nth(1) else {
        eprintln!("usage: musicbrainz_search <album or query>");
        std::process::exit(1);
    };

    let provider = MusicBrainzProvider::new();
    let query = SearchQuery {
        album: Some(query_text),
        ..SearchQuery::default()
    };

    let candidates = match provider.search(&query) {
        Ok(candidates) => candidates,
        Err(err) => {
            eprintln!("search failed: {err}");
            std::process::exit(1);
        }
    };

    println!("{} candidates:", candidates.len());
    for candidate in candidates.iter().take(10) {
        println!(
            "  [{}] {} - {} ({})  score {:.2}",
            candidate.id.0,
            candidate.artist,
            candidate.title,
            candidate
                .year
                .map(|y| y.to_string())
                .unwrap_or_else(|| "?".to_string()),
            candidate.score,
        );
    }

    let Some(first) = candidates.first() else {
        return;
    };

    println!("\nfetching release {}...", first.id.0);
    match provider.fetch_release(&first.id) {
        Ok(release) => {
            println!(
                "{} - {} ({})",
                release.artist,
                release.title,
                release
                    .year
                    .map(|y| y.to_string())
                    .unwrap_or_else(|| "?".to_string())
            );
            println!("genres: {}", release.genres.join(", "));
            if let Some(cover) = &release.cover_image_url {
                println!("cover: {cover}");
            }
            for track in &release.tracks {
                let artist = track.artist.as_deref().unwrap_or(&release.artist);
                println!("  {:>4}  {} - {}", track.position, artist, track.title);
            }
        }
        Err(err) => {
            eprintln!("fetch failed: {err}");
            std::process::exit(1);
        }
    }
}
