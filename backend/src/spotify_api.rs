use std::ops::Try;
use std::thread;

use chrono::Utc;
use crossbeam::channel;
use diesel::prelude::*;
use hashbrown::HashMap;
use reqwest;
use serde::{Deserialize, Serialize};

use crate::conf::CONF;
use crate::models::{
    AccessTokenResponse, Artist, ArtistGenrePair, NewArtistHistoryEntry, NewTrackHistoryEntry,
    SpotifyBatchArtistsResponse, SpotifyBatchTracksResponse, SpotifyResponse, StatsSnapshot,
    TopArtistsResponse, TopTracksResponse, Track, TrackArtistPair, User, UserProfile,
};
use crate::DbConn;

const SPOTIFY_USER_RECENTLY_PLAYED_URL: &str =
    "https://api.spotify.com/v1/me/player/recently-played";
const SPOTIFY_USER_PROFILE_INFO_URL: &str = "https://api.spotify.com/v1/me";
const SPOTIFY_BATCH_TRACKS_URL: &str = "https://api.spotify.com/v1/tracks";
const SPOTIFY_BATCH_ARTISTS_URL: &str = "https://api.spotify.com/v1/artists";
const SPOTIFY_APP_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const ENTITY_FETCH_COUNT: usize = 50;

fn get_top_entities_url(entity_type: &str, timeframe: &str) -> String {
    format!(
        "https://api.spotify.com/v1/me/top/{}?limit={}&time_range={}_term",
        entity_type, ENTITY_FETCH_COUNT, timeframe
    )
}

pub fn spotify_user_api_request<T: for<'de> Deserialize<'de> + std::fmt::Debug + Clone>(
    url: &str,
    token: &str,
) -> Result<T, String> {
    let client = reqwest::blocking::Client::new();
    let res = client
        .get(url)
        .bearer_auth(token)
        .send()
        .map_err(|err| -> String {
            error!("Error fetching user data from Spotify API: {:?}", err);
            "Error requesting latest user data from the Spotify API".into()
        })?;

    res.json::<SpotifyResponse<T>>()
        .map_err(|err| -> String {
            error!(
                "Error parsing user data response from Spotify API: {:?}",
                err
            );
            "Error parsing user data response from Spotify API".into()
        })?
        .into_result()
}

pub fn get_user_profile_info(token: &str) -> Result<UserProfile, String> {
    spotify_user_api_request(SPOTIFY_USER_PROFILE_INFO_URL, token)
}

pub fn spotify_server_api_request<T: for<'de> Deserialize<'de> + std::fmt::Debug + Clone>(
    url: &str,
    params: HashMap<&str, &str>,
) -> Result<T, String> {
    let client = reqwest::blocking::Client::new();

    info!("Hitting Spotify API at URL {}, params: {:?}", url, params);
    let res = client
        .post(url)
        .header("Authorization", CONF.get_authorization_header_content())
        .form(&params)
        .send()
        .map_err(|err| -> String {
            error!("Error communicating with Spotify API: {:?}", err);
            "Error communicating with from the Spotify API".into()
        })?;

    if res.status() != reqwest::StatusCode::OK {
        error!("Got bad status code of {} from Spotify API", res.status());
        return Err("Got bad response from Spotify API".into());
    }

    res.json::<SpotifyResponse<T>>()
        .map_err(|err| -> String {
            error!("Error decoding response from Spotify API: {:?}.", err,);
            "Error decoding response from Spotify API".into()
        })?
        .into_result()
}

pub fn fetch_auth_token() -> Result<AccessTokenResponse, String> {
    let mut params = HashMap::new();
    params.insert("grant_type", "client_credentials");

    spotify_server_api_request(SPOTIFY_APP_TOKEN_URL, params)
}

pub fn refresh_user_token(refresh_token: &str) -> Result<String, String> {
    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh_token);

    let res: AccessTokenResponse = spotify_server_api_request(SPOTIFY_APP_TOKEN_URL, params)?;
    Ok(res.access_token)
}

pub fn fetch_cur_stats(user: &User) -> Result<Option<StatsSnapshot>, String> {
    // Use the user's token to fetch their current stats
    let (tx, rx) = channel::unbounded::<(
        &'static str,
        &'static str,
        Result<reqwest::blocking::Response, String>,
    )>();

    // Create threads for each of the inner requests (we have to make 6; one for each of the three
    // timeframes, and then that multiplied by each of the two entities (tracks and artists)).
    info!("Kicking off 6 API requests on separate threads...");
    for entity_type in &["tracks", "artists"] {
        for timeframe in &["short", "medium", "long"] {
            let token = user.token.clone();
            let tx = tx.clone();

            thread::spawn(move || {
                let client = reqwest::blocking::Client::new();
                let res: Result<reqwest::blocking::Response, String> = client
                    .get(&get_top_entities_url(entity_type, timeframe))
                    .bearer_auth(token)
                    .send()
                    .map_err(|_err| -> String {
                        "Error requesting latest user stats from the Spotify API".into()
                    });

                tx.send((entity_type, timeframe, res))
            });
        }
    }

    let mut stats_snapshot = StatsSnapshot::new(Utc::now().naive_utc());

    // Wait for all 6 requests to return back and then
    info!("Waiting for all 6 inner stats requests to return...");
    for _ in 0..6 {
        match rx.recv().unwrap() {
            ("tracks", timeframe, res) => {
                let parsed_res: TopTracksResponse = res?.json().map_err(|err| -> String {
                    error!("Error parsing top tracks response: {:?}", err);
                    "Error parsing response from Spotify".into()
                })?;

                for top_track in parsed_res.items.into_iter() {
                    stats_snapshot.tracks.add_item(timeframe, top_track);
                }
            }
            ("artists", timeframe, res) => {
                let parsed_res: TopArtistsResponse = res?.json().map_err(|err| -> String {
                    error!("Error parsing top artists response: {:?}", err);
                    "Error parsing response from Spotify".into()
                })?;

                for top_artist in parsed_res.items.into_iter() {
                    stats_snapshot.artists.add_item(timeframe, top_artist);
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(Some(stats_snapshot))
}

fn map_timeframe_to_timeframe_id(timeframe: &str) -> u8 {
    match timeframe {
        "short" => 0,
        "medium" => 1,
        "long" => 2,
        _ => panic!(format!(
            "Tried to convert invalid timeframe to id: \"{}\"",
            timeframe
        )),
    }
}

/// For each track and artist timeframe, store a row in the `track_rank_snapshots` and `artist_rank_snapshots`
/// tables respectively
pub fn store_stats_snapshot(
    conn: &DbConn,
    user: &User,
    stats: StatsSnapshot,
) -> Result<(), String> {
    let update_time = stats.last_update_time;

    let genres_by_artist_id: HashMap<String, Vec<String>> = stats
        .artists
        .iter()
        .flat_map(|(_artist_timeframe, artists)| artists.iter())
        // Also include all other artists included in track metadata
        .chain(stats.tracks.iter().flat_map(|(_track_timeframe, tracks)| {
            tracks.iter().flat_map(|track| track.artists.iter())
        }))
        .fold(HashMap::new(), |mut acc, artist| {
            acc.insert(
                artist.id.clone(),
                artist.genres.clone().unwrap_or_else(Vec::new),
            );
            acc
        });
    let mapped_artist_spotify_ids =
        crate::db_util::retrieve_mapped_spotify_ids(conn, genres_by_artist_id.keys())?;

    let artist_entries: Vec<NewArtistHistoryEntry> = stats
        .artists
        .into_iter()
        .flat_map(|(artist_timeframe, artists)| {
            artists
                .into_iter()
                .enumerate()
                .map(move |(artist_ranking, artist)| (artist_timeframe, artist_ranking, artist.id))
                .map(|(artist_timeframe, artist_ranking, artist_spotify_id)| {
                    NewArtistHistoryEntry {
                        user_id: user.id,
                        mapped_spotify_id: mapped_artist_spotify_ids[&artist_spotify_id],
                        update_time,
                        timeframe: map_timeframe_to_timeframe_id(&artist_timeframe),
                        ranking: artist_ranking as u16,
                    }
                })
        })
        .collect();

    diesel::insert_into(crate::schema::artist_rank_snapshots::table)
        .values(&artist_entries)
        .execute(&conn.0)
        .map_err(|err| -> String {
            println!("Error inserting row: {:?}", err);
            "Error inserting user into database".into()
        })?;

    let track_spotify_ids: Vec<String> = stats
        .tracks
        .iter()
        .flat_map(|(_artist_timeframe, tracks)| tracks.iter().map(|track| track.id.clone()))
        .collect::<Vec<_>>();
    let mapped_track_spotify_ids =
        crate::db_util::retrieve_mapped_spotify_ids(conn, track_spotify_ids.iter())?;

    // Create track/artist mapping entries for each (track, artist) pair
    let track_artist_pairs: Vec<TrackArtistPair> = stats
        .tracks
        .iter()
        .flat_map(|(_artist_timeframe, tracks)| {
            tracks.iter().flat_map(|track| {
                let track_internal_id = mapped_track_spotify_ids[&track.id];

                track
                    .artists
                    .iter()
                    .map(|artist| mapped_artist_spotify_ids[&artist.id])
                    .map(move |artist_internal_id| TrackArtistPair {
                        track_id: track_internal_id,
                        artist_id: artist_internal_id,
                    })
            })
        })
        .collect();
    diesel::insert_or_ignore_into(crate::schema::tracks_artists::table)
        .values(&track_artist_pairs)
        .execute(&conn.0)
        .map_err(|err| -> String {
            error!("Error inserting track/artist mappings: {:?}", err);
            "Error inserting track/artist metadata into database".into()
        })?;

    // Create artist/genre mapping entries for each (artist, genre) pair
    let artist_genre_pairs: Vec<ArtistGenrePair> = genres_by_artist_id
        .into_iter()
        .flat_map(|(artist_id, genres)| {
            let artist_id: i32 = *mapped_artist_spotify_ids
                .get(&artist_id)
                .expect("No entry in artist id mapping");

            genres
                .into_iter()
                .map(move |genre| ArtistGenrePair { artist_id, genre })
        })
        .collect();

    // Delete all old artist/genre entries for the artists we have here and insert the new ones, making sure that the entries we have are all valid and up-to-date
    conn.0
        .transaction::<_, diesel::result::Error, _>(|| {
            use crate::schema::artists_genres::dsl::*;

            diesel::delete(
                artists_genres.filter(artist_id.eq_any(mapped_artist_spotify_ids.values())),
            )
            .execute(&conn.0)?;

            diesel::insert_into(artists_genres)
                .values(&artist_genre_pairs)
                .execute(&conn.0)
        })
        .map_err(|err| -> String {
            error!("Error inserting artist/genre mappings: {:?}", err);
            "Error inserting artist/genre mappings into database".into()
        })?;

    let track_entries: Vec<NewTrackHistoryEntry> = stats
        .tracks
        .into_iter()
        .flat_map(|(track_timeframe, tracks)| {
            tracks
                .into_iter()
                .enumerate()
                .map(move |(track_ranking, track)| (track_timeframe, track_ranking, track.id))
                .map(
                    |(track_timeframe, track_ranking, track_spotify_id)| NewTrackHistoryEntry {
                        user_id: user.id,
                        mapped_spotify_id: mapped_track_spotify_ids[&track_spotify_id],
                        update_time,
                        timeframe: map_timeframe_to_timeframe_id(&track_timeframe),
                        ranking: track_ranking as u16,
                    },
                )
        })
        .collect();

    diesel::insert_into(crate::schema::track_rank_snapshots::table)
        .values(&track_entries)
        .execute(&conn.0)
        .map_err(|err| -> String {
            error!("Error inserting row: {:?}", err);
            "Error inserting user into database".into()
        })?;

    // Update the user to have a last update time that matches all of the new updates
    let updated_row_count = crate::db_util::update_user_last_updated(&user, &conn, update_time)?;

    if updated_row_count != 1 {
        error!(
            "Updated {} rows when setting last update time, but should have updated 1.",
            updated_row_count
        );
    }

    Ok(())
}

const MAX_BATCH_ENTITY_COUNT: usize = 50;

fn fetch_batch_entities<T: for<'de> Deserialize<'de>>(
    base_url: &str,
    token: &str,
    spotify_entity_ids: &[&str],
) -> Result<T, String> {
    let url = format!("{}?ids={}", base_url, spotify_entity_ids.join(","));
    let client = reqwest::blocking::Client::new();
    client
        .get(&url)
        .bearer_auth(token)
        .send()
        .map_err(|_err| -> String { "Error requesting batch data from the Spotify API".into() })?
        .json()
        .map_err(|err| -> String {
            error!("Error decoding JSON from Spotify API: {:?}", err);
            "Error reading data from the Spotify API".into()
        })
}

fn fetch_with_cache<
    ResponseType: for<'de> Deserialize<'de>,
    T: Clone + Serialize + for<'de> Deserialize<'de>,
>(
    cache_key: &str,
    api_url: &str,
    spotify_access_token: &str,
    spotify_ids: &[&str],
    map_response_to_items: fn(ResponseType) -> Result<Vec<T>, String>,
) -> Result<Vec<T>, String> {
    // First, try to get as many items as we can from the cache
    info!("Checking cache for {} spotify ids...", spotify_ids.len());
    let cache_res = crate::cache::get_hash_items::<T>(cache_key, spotify_ids)?;

    // Fire off a request to Spotify to fill in the missing items
    let mut missing_indices = Vec::new();
    let mut missing_ids = Vec::new();
    for (i, datum) in cache_res.iter().enumerate() {
        if datum.is_none() {
            missing_indices.push(i);
            missing_ids.push(spotify_ids[i]);
        }
    }
    info!(
        "{}/{} items found in the cache.",
        cache_res.len() - missing_indices.len(),
        spotify_ids.len()
    );

    let mut fetched_entities = Vec::with_capacity(missing_indices.len());
    for (chunk_ix, chunk) in missing_ids.chunks(MAX_BATCH_ENTITY_COUNT).enumerate() {
        info!("Fetching chunk {}...", chunk_ix);
        let res: ResponseType = fetch_batch_entities(api_url, spotify_access_token, chunk)?;
        let fetched_artist_data = map_response_to_items(res)?;

        for i in 0..chunk.len() {
            debug_assert_eq!(
                chunk[i],
                missing_ids[(chunk_ix * MAX_BATCH_ENTITY_COUNT) + i]
            );
        }

        // Update the cache with the missing items
        crate::cache::set_hash_items(
            cache_key,
            &fetched_artist_data
                .iter()
                .enumerate()
                .map(|(i, datum)| (chunk[i], datum))
                .collect::<Vec<_>>(),
        )?;

        fetched_entities.extend(fetched_artist_data)
    }
    info!("Fetched all chunks.");

    let mut i = 0;
    let combined_results = cache_res
        .into_iter()
        .map(|opt| {
            opt.unwrap_or_else(|| {
                // We could avoid this clone by reversing the direction in which we fetch the items
                // but that's 100% premature and likely useless optimization
                let val = fetched_entities[i].clone();
                i += 1;
                val
            })
        })
        .collect::<Vec<_>>();
    Ok(combined_results)
}

pub fn fetch_artists(
    spotify_access_token: &str,
    spotify_ids: &[&str],
) -> Result<Vec<Artist>, String> {
    fetch_with_cache::<SpotifyBatchArtistsResponse, _>(
        &CONF.artists_cache_hash_name,
        SPOTIFY_BATCH_ARTISTS_URL,
        spotify_access_token,
        spotify_ids,
        |res: SpotifyBatchArtistsResponse| Ok(res.artists),
    )
}

pub fn fetch_tracks(
    spotify_access_token: &str,
    spotify_ids: &[&str],
) -> Result<Vec<Track>, String> {
    fetch_with_cache::<SpotifyBatchTracksResponse, _>(
        &CONF.tracks_cache_hash_name,
        SPOTIFY_BATCH_TRACKS_URL,
        spotify_access_token,
        spotify_ids,
        |res: SpotifyBatchTracksResponse| Ok(res.tracks),
    )
}
