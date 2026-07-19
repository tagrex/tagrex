//! Manual check against the real Discogs API. Requires a personal token.
//!
//! Usage:
//!   DISCOGS_TOKEN=xxxxx cargo run -p tagrex-providers-discogs \
//!     --example discogs_search -- "album or query"
//!
//! Prints the top search hits, then fetches and prints the first release's
//! tracklist. Read-only; it only issues GET requests to Discogs.

use tagrex_core::provider::{MetadataProvider, SearchQuery};
use tagrex_providers_discogs::DiscogsProvider;

fn main() {
    let Ok(token) = std::env::var("DISCOGS_TOKEN") else {
        eprintln!("set DISCOGS_TOKEN to your personal Discogs token");
        std::process::exit(1);
    };
    let Some(query_text) = std::env::args().nth(1) else {
        eprintln!("usage: discogs_search <album or query>");
        std::process::exit(1);
    };

    let provider = DiscogsProvider::new(token);
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
            println!("styles: {}", release.styles.join(", "));
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
