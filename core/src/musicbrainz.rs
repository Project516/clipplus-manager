//! Recording search using the MusicBrainz web service.

use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://musicbrainz.org/ws/2/recording/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub release: Option<String>,
    pub length_ms: Option<u64>,
}

#[derive(Deserialize)]
struct Results {
    recordings: Option<Vec<MusicBrainzRecording>>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct MusicBrainzRecording {
    id: String,
    title: String,
    length: Option<u64>,
    #[serde(default)]
    score: u32,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(default)]
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct ArtistCredit {
    name: String,
    #[serde(default)]
    joinphrase: String,
}

#[derive(Deserialize)]
struct Release {
    title: String,
    status: Option<String>,
    date: Option<String>,
    #[serde(rename = "release-group")]
    group: Option<ReleaseGroup>,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    #[serde(rename = "primary-type")]
    primary: Option<String>,
    #[serde(rename = "secondary-types", default)]
    secondary: Vec<String>,
}

impl Release {
    /// Lower is better: official studio albums first, then the earliest.
    fn rank(&self) -> (u8, &str) {
        let official = self.status.as_deref() == Some("Official");
        let group = self.group.as_ref();
        let album = group.is_some_and(|g| g.primary.as_deref() == Some("Album"));
        // Film albums like "Help!" are tagged Soundtrack but hold the original songs.
        let studio = album && group.is_some_and(|g| g.secondary.iter().all(|t| t == "Soundtrack"));
        let kind = match (official, studio, album) {
            (true, true, _) => 0,
            (true, false, true) => 1,
            (true, false, false) => 2,
            _ => 3,
        };
        (
            kind,
            self.date
                .as_deref()
                .filter(|d| !d.is_empty())
                .unwrap_or("9999"),
        )
    }
}

/// Lowercase words made of letters, digits and apostrophes.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| word.chars().any(char::is_alphanumeric))
        .map(str::to_string)
        .collect()
}

/// Search recordings. Call from a worker thread: MusicBrainz limits requests to one per second.
pub fn search(query: &str) -> Result<Vec<Recording>, String> {
    if words(query).is_empty() {
        return Ok(Vec::new());
    }
    search_with_retry(
        &crate::net::agent(),
        ENDPOINT,
        query,
        Duration::from_secs(2),
        pace,
    )
    .map_err(|e| e.to_string())
}

/// Waits until a second has passed since the last request, then records this one.
fn pace() {
    static LAST_REQUEST: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let mut last = LAST_REQUEST
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(wait) = last.and_then(|time| Duration::from_secs(1).checked_sub(time.elapsed())) {
        std::thread::sleep(wait);
    }
    *last = Some(Instant::now());
}

// `pace` runs before every attempt, so a retry also counts against the limit.
fn search_with_retry(
    agent: &ureq::Agent,
    endpoint: &str,
    query: &str,
    delay: Duration,
    pace: fn(),
) -> Result<Vec<Recording>, SearchError> {
    pace();
    match search_at(agent, endpoint, query) {
        Err(SearchError::Busy) => {
            std::thread::sleep(delay);
            pace();
            search_at(agent, endpoint, query)
        }
        other => other,
    }
}

#[derive(Debug)]
enum SearchError {
    Busy,
    Other(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SearchError::Busy => f.write_str("MusicBrainz is busy, try again in a moment"),
            SearchError::Other(e) => f.write_str(e),
        }
    }
}

fn search_at(
    agent: &ureq::Agent,
    endpoint: &str,
    query: &str,
) -> Result<Vec<Recording>, SearchError> {
    let text = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = agent
        .get(endpoint)
        .header(
            "User-Agent",
            concat!(
                "clipplus-manager/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/Project516/clipplus-manager)"
            ),
        )
        // dismax takes the words as plain text and tolerates extra or missing ones.
        .query("query", text)
        .query("dismax", "true")
        .query("fmt", "json")
        // Each song repeats once per release, so ask for more than we show.
        .query("limit", "100")
        .call()
        .map_err(|e| match e {
            // MusicBrainz answers 503 when it is rate limiting.
            ureq::Error::StatusCode(503) => SearchError::Busy,
            e => SearchError::Other(format!("MusicBrainz search: {e}")),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| SearchError::Other(format!("MusicBrainz response: {e}")))?;
    let results: Results = serde_json::from_str(&body)
        .map_err(|e| SearchError::Other(format!("MusicBrainz response: {e}")))?;
    // Overload can also come as a 200 with an error body.
    if results.error.is_some() {
        return Err(SearchError::Busy);
    }
    let recordings = results
        .recordings
        .ok_or_else(|| SearchError::Other("MusicBrainz response: no recordings".into()))?;
    Ok(rank(recordings, query))
}

/// Closest match to the query first, one row per song.
fn rank(recordings: Vec<MusicBrainzRecording>, query: &str) -> Vec<Recording> {
    let wanted: HashSet<String> = words(query).into_iter().collect();
    let mut ranked: Vec<_> = recordings
        .into_iter()
        .enumerate()
        .map(|(index, recording)| {
            let artist: String = recording
                .artist_credit
                .iter()
                .map(|credit| format!("{}{}", credit.name, credit.joinphrase))
                .collect();
            let extra = words(&format!("{artist} {}", recording.title))
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .filter(|word| !wanted.contains(word))
                .count();
            let key = (
                extra,
                Reverse(recording.score),
                Reverse(recording.releases.len()),
                index,
            );
            (key, artist, recording)
        })
        .collect();
    ranked.sort_by_key(|(key, ..)| *key);
    let mut seen = HashSet::new();
    ranked
        .into_iter()
        .filter(|(_, artist, recording)| {
            seen.insert((recording.title.to_lowercase(), artist.to_lowercase()))
        })
        .map(|(_, artist, recording)| {
            let best = recording
                .releases
                .iter()
                .min_by(|a, b| a.rank().cmp(&b.rank()));
            Recording {
                release: best.map(|release| release.title.clone()),
                id: recording.id,
                title: recording.title,
                artist,
                length_ms: recording.length,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread::JoinHandle;

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const BUSY: &str =
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// Answers one connection per response, in order, and returns the raw requests.
    fn serve(responses: Vec<String>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/ws/2/recording/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0; 4096];
                    let size = stream.read(&mut request).unwrap();
                    stream.write_all(response.as_bytes()).unwrap();
                    String::from_utf8_lossy(&request[..size]).into_owned()
                })
                .collect()
        });
        (endpoint, server)
    }

    fn release(title: &str, status: &str, date: &str, secondary: &[&str]) -> Value {
        json!({"title": title, "status": status, "date": date,
            "release-group": {"primary-type": "Album", "secondary-types": secondary}})
    }

    fn albums(count: usize) -> Vec<Value> {
        (0..count)
            .map(|n| release(&format!("Album {n}"), "Official", "2000", &[]))
            .collect()
    }

    fn recording(id: &str, title: &str, artist: &str, score: u32, releases: Vec<Value>) -> Value {
        json!({"id": id, "title": title, "score": score,
            "artist-credit": [{"name": artist}], "releases": releases})
    }

    #[test]
    fn searches_and_reads_recording_metadata() {
        let body = r#"{"recordings":[{"id":"other-id","title":"Other"},{"id":"recording-id","title":"Song","length":123000,"artist-credit":[{"name":"First","joinphrase":" & "},{"name":"Second"}],"releases":[{"title":"Album"}]}]}"#;
        let (endpoint, server) = serve(vec![ok(body)]);
        let results = search_at(&crate::net::agent(), &endpoint, " first & second  song ").unwrap();
        let request = &server.join().unwrap()[0];
        assert!(
            request.contains("query=first%20%26%20second%20song"),
            "{request}"
        );
        assert!(request.contains("dismax=true"));
        assert!(!request.contains("artist%3A") && !request.contains("recording%3A"));
        assert!(request.contains("fmt=json"));
        assert!(request.contains("limit=100"));
        assert!(
            request
                .to_lowercase()
                .contains("user-agent: clipplus-manager/")
        );
        assert_eq!(
            results[0],
            Recording {
                id: "recording-id".into(),
                title: "Song".into(),
                artist: "First & Second".into(),
                release: Some("Album".into()),
                length_ms: Some(123000),
            }
        );
        assert_eq!(results[1].release, None);
        assert_eq!(results[1].length_ms, None);
    }

    #[test]
    fn empty_query_does_not_search() {
        assert!(search("  ").unwrap().is_empty());
        assert!(search(" & - ").unwrap().is_empty());
    }

    #[test]
    fn splits_words_on_anything_but_letters_digits_and_apostrophes() {
        assert_eq!(words(" Toto  AFRICA "), ["toto", "africa"]);
        assert_eq!(words("AC/DC & t.n.t"), ["ac", "dc", "t", "n", "t"]);
        assert_eq!(words("Don\u{2019}t Stop 99 - '"), ["don't", "stop", "99"]);
        // A combining accent splits the word, the same as in web/src/musicbrainz.js.
        assert_eq!(words("Cafe\u{301} Band"), ["cafe", "band"]);
    }

    #[test]
    fn ranks_by_fewest_extra_words_then_score_then_releases() {
        let body = json!({"recordings": [
            recording("cover", "Northern Lights (Glass Harbor Cover)", "Pier Kids", 100, albums(1)),
            recording("quartet", "Northern Lights", "Tidal Quartet", 100, albums(3)),
            recording("demo", "northern lights", "the glass harbor", 80, albums(1)),
            recording("choir", "Northern Lights", "Salt Choir", 100, albums(5)),
            recording("trio", "Northern Lights", "Tidal Trio", 100, albums(3)),
            recording("original", "Northern Lights", "The Glass Harbor", 90, vec![
                release("Greatest Tides", "Official", "2022", &["Compilation"]),
                release("Tidewater", "Official", "2019", &[]),
                release("Promo", "Promotion", "2018", &[]),
                release("Reissue", "Official", "2020", &[]),
            ]),
            recording("faint", "Northern Lights", "Mira Salt", 70, albums(9)),
        ]})
        .to_string();
        let (endpoint, server) = serve(vec![ok(&body)]);
        let results = search_at(
            &crate::net::agent(),
            &endpoint,
            "the glass harbor northern lights",
        )
        .unwrap();
        server.join().unwrap();
        let ids: Vec<_> = results.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            ["original", "choir", "quartet", "trio", "faint", "cover"]
        );
        assert_eq!(results[0].release.as_deref(), Some("Tidewater"));
    }

    #[test]
    fn keeps_one_row_per_song_from_the_studio_album() {
        let body = json!({"recordings": [
            recording("d", "Song (live)", "Band", 0,
                vec![release("Live", "Bootleg", "1990", &["Live"])]),
            recording("a", "Song", "Band", 0,
                vec![release("Hits", "Official", "1999", &["Compilation"])]),
            recording("b", "Other", "Band", 0, vec![]),
            recording("c", "song", "Band", 0, vec![
                release("Bootleg", "Bootleg", "1980", &[]),
                release("Reissue", "Official", "2003", &[]),
                release("Debut", "Official", "1982-04", &["Soundtrack"]),
            ]),
        ]})
        .to_string();
        let (endpoint, server) = serve(vec![ok(&body)]);
        let results = search_at(&crate::net::agent(), &endpoint, "band song").unwrap();
        server.join().unwrap();
        let rows: Vec<_> = results
            .iter()
            .map(|r| (r.id.as_str(), r.release.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [("c", Some("Debut")), ("d", Some("Live")), ("b", None)]
        );
    }

    #[test]
    fn retries_once_when_busy() {
        let body = json!({"recordings": [recording("a", "Song", "Band", 1, vec![])]}).to_string();
        let busy_body = ok(r#"{"error":"Your requests are exceeding the allowable rate limit."}"#);
        static PACED: AtomicUsize = AtomicUsize::new(0);
        fn count() {
            PACED.fetch_add(1, Ordering::SeqCst);
        }
        for busy in [BUSY.to_string(), busy_body] {
            let (endpoint, server) = serve(vec![busy, ok(&body)]);
            let before = PACED.load(Ordering::SeqCst);
            let results = search_with_retry(
                &crate::net::agent(),
                &endpoint,
                "band song",
                Duration::ZERO,
                count,
            )
            .unwrap();
            assert_eq!(server.join().unwrap().len(), 2);
            assert_eq!(results.len(), 1);
            // The retry is paced too.
            assert_eq!(PACED.load(Ordering::SeqCst) - before, 2);
        }
    }

    #[test]
    fn gives_up_after_one_retry() {
        let (endpoint, server) = serve(vec![BUSY.to_string(), BUSY.to_string()]);
        let error = search_with_retry(
            &crate::net::agent(),
            &endpoint,
            "band song",
            Duration::ZERO,
            || {},
        )
        .unwrap_err();
        assert_eq!(server.join().unwrap().len(), 2);
        assert!(error.to_string().contains("busy"));
    }
}
