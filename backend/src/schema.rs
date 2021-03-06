table! {
    artists_genres (id) {
        id -> Integer,
        artist_id -> Integer,
        genre -> Varchar,
    }
}

table! {
    artist_rank_snapshots (id) {
        id -> Bigint,
        user_id -> Bigint,
        update_time -> Datetime,
        mapped_spotify_id -> Integer,
        timeframe -> Unsigned<Tinyint>,
        ranking -> Unsigned<Smallint>,
    }
}

table! {
    artist_stats_history (id) {
        id -> Bigint,
        spotify_id -> Varchar,
        followers -> Unsigned<Bigint>,
        popularity -> Unsigned<Bigint>,
        uri -> Text,
    }
}

table! {
    spotify_items (id) {
        id -> Integer,
        spotify_id -> Varchar,
    }
}

table! {
    tracks_artists (id) {
        id -> Integer,
        track_id -> Integer,
        artist_id -> Integer,
    }
}

table! {
    track_rank_snapshots (id) {
        id -> Bigint,
        user_id -> Bigint,
        update_time -> Datetime,
        mapped_spotify_id -> Integer,
        timeframe -> Unsigned<Tinyint>,
        ranking -> Unsigned<Smallint>,
    }
}

table! {
    track_stats_history (id) {
        id -> Bigint,
        followers -> Unsigned<Bigint>,
        popularity -> Unsigned<Bigint>,
        playcount -> Nullable<Unsigned<Bigint>>,
    }
}

table! {
    users (id) {
        id -> Bigint,
        creation_time -> Datetime,
        last_update_time -> Datetime,
        spotify_id -> Varchar,
        username -> Text,
        token -> Text,
        refresh_token -> Text,
    }
}

joinable!(artist_rank_snapshots -> spotify_items (mapped_spotify_id));
joinable!(artist_rank_snapshots -> users (user_id));
joinable!(artists_genres -> spotify_items (artist_id));
joinable!(track_rank_snapshots -> spotify_items (mapped_spotify_id));
joinable!(track_rank_snapshots -> users (user_id));

allow_tables_to_appear_in_same_query!(
    artists_genres,
    artist_rank_snapshots,
    artist_stats_history,
    spotify_items,
    tracks_artists,
    track_rank_snapshots,
    track_stats_history,
    users,
);
