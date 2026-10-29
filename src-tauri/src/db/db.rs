use crate::lyrics::LyricsSource;
use crate::types::{Album, ArtistSummary, Playlist, Song};
use sqlx::sqlite::SqlitePool;
use sqlx::Error;

pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &str) -> Self {
        let pool = SqlitePool::connect(&format!("sqlite:{db_path}"))
            .await
            .expect("Failed to connect to database");
        Self { pool }
    }

    pub async fn get_all_songs(&self) -> Result<Vec<Song>, sqlx::Error> {
        sqlx::query_as::<_, Song>(
            "SELECT *
             FROM songs
             ORDER BY album_artist ASC, album ASC, track_number ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_artists_summary(&self) -> Result<Vec<ArtistSummary>, sqlx::Error> {
        // MAX(cover_url) just to grab one valid album cover
        let artists = sqlx::query_as::<_, ArtistSummary>(
            "SELECT
                album_artist AS name,
                COUNT(DISTINCT album) AS album_count,
                MAX(cover_url) AS cover_url
             FROM songs
             GROUP BY album_artist
             ORDER BY album_artist ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(artists)
    }

    pub async fn get_artist_albums(&self, artist_name: &str) -> Result<Vec<Album>, sqlx::Error> {
        let songs = sqlx::query_as::<_, Song>(
            "SELECT * FROM songs WHERE album_artist = ? ORDER BY album ASC, track_number ASC",
        )
        .bind(artist_name)
        .fetch_all(&self.pool)
        .await?;

        let mut album_map: std::collections::BTreeMap<String, Album> =
            std::collections::BTreeMap::new();

        for song in songs {
            let entry = album_map.entry(song.album.clone()).or_insert(Album {
                title: song.album.clone(),
                album_artist: song.album_artist.clone(),
                cover_url: song.cover_url.clone(),
                songs: Vec::new(),
            });
            entry.songs.push(song);
        }

        let albums = album_map.into_values().collect();

        Ok(albums)
    }

    pub async fn update_external_cover(&self, id: i64, url: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE songs SET external_cover_url = ? WHERE id = ?")
            .bind(url)
            .bind(id)
            .execute(&self.pool)
            .await?;

        // println!("SAVED COVER URL: {url:?}");

        Ok(())
    }

    pub async fn update_song_lyrics(
        &self,
        id: i64,
        lyrics_source: LyricsSource,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE songs SET lyrics = ?, lyrics_source = ? WHERE id = ?")
            .bind(lyrics_source.lyrics)
            .bind(lyrics_source.source)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_song_lyrics(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE songs SET lyrics = NULL, lyrics_source = NULL WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_song(&self, song: Song) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO songs (
                path, title, artists, album_artist, album, track_number,
                duration_ms, cover_url, external_cover_url, genre, release_year,
                lyrics, lyrics_source, source, source_url, source_item_id,
                canonical_track_slug, canonical_album_slug, quality_tier
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET
                title = excluded.title,
                artists = excluded.artists,
                album_artist = excluded.album_artist,
                album = excluded.album,
                track_number = excluded.track_number,
                duration_ms = excluded.duration_ms,
                cover_url = excluded.cover_url,
                genre = excluded.genre,
                release_year = excluded.release_year,
                source = excluded.source,
                source_url = excluded.source_url,
                source_item_id = excluded.source_item_id,
                canonical_track_slug = excluded.canonical_track_slug,
                canonical_album_slug = excluded.canonical_album_slug,
                quality_tier = excluded.quality_tier",
        )
        .bind(&song.path)
        .bind(&song.title)
        .bind(sqlx::types::Json(&song.artists)) // Binds the Vec<String> as JSON String
        .bind(&song.album_artist)
        .bind(&song.album)
        .bind(song.track_number)
        .bind(song.duration_ms as i64)
        .bind(&song.cover_url)
        .bind(&song.external_cover_url)
        .bind(&song.genre)
        .bind(song.release_year)
        .bind(&song.lyrics)
        .bind(&song.lyrics_source)
        .bind(song.source)
        .bind(&song.source_url)
        .bind(&song.source_item_id)
        .bind(&song.canonical_track_slug)
        .bind(&song.canonical_album_slug)
        .bind(song.quality_tier)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_songs(&self, songs: Vec<Song>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for song in songs {
            sqlx::query(
                "INSERT INTO songs (
                    path, title, artists, album_artist, album, track_number,
                    duration_ms, cover_url, external_cover_url, genre, release_year,
                    lyrics, lyrics_source, source, source_url, source_item_id,
                    canonical_track_slug, canonical_album_slug, quality_tier
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(path) DO UPDATE SET
                    title = excluded.title,
                    artists = excluded.artists,
                    album_artist = excluded.album_artist,
                    album = excluded.album,
                    track_number = excluded.track_number,
                    duration_ms = excluded.duration_ms,
                    cover_url = excluded.cover_url,
                    genre = excluded.genre,
                    release_year = excluded.release_year,
                    source = excluded.source,
                    source_url = excluded.source_url,
                    source_item_id = excluded.source_item_id,
                    canonical_track_slug = excluded.canonical_track_slug,
                    canonical_album_slug = excluded.canonical_album_slug,
                    quality_tier = excluded.quality_tier
                ",
            )
            .bind(&song.path)
            .bind(&song.title)
            .bind(sqlx::types::Json(&song.artists))
            .bind(&song.album_artist)
            .bind(&song.album)
            .bind(song.track_number)
            .bind(song.duration_ms as i64)
            .bind(&song.cover_url)
            .bind(&song.external_cover_url)
            .bind(&song.genre)
            .bind(song.release_year)
            .bind(&song.lyrics)
            .bind(&song.lyrics_source)
            .bind(song.source)
            .bind(&song.source_url)
            .bind(&song.source_item_id)
            .bind(&song.canonical_track_slug)
            .bind(&song.canonical_album_slug)
            .bind(song.quality_tier)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_song_by_id(&self, id: i64) -> Result<Option<Song>, sqlx::Error> {
        let song = sqlx::query_as::<_, Song>(
            "SELECT
                id, path, title, artists, album_artist, album, track_number,
                duration_ms, cover_url, external_cover_url, genre, release_year,
                lyrics, lyrics_source, source, source_url, source_item_id,
                canonical_track_slug, canonical_album_slug, quality_tier
             FROM songs
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(song)
    }

    pub async fn get_songs_by_ids(&self, ids: &[i64]) -> Result<Vec<Song>, sqlx::Error> {
        sqlx::query_as::<_, Song>(
            "SELECT
                id, path, title, artists, album_artist, album, track_number,
                duration_ms, cover_url, external_cover_url, genre, release_year,
                lyrics, lyrics_source, source, source_url, source_item_id,
                canonical_track_slug, canonical_album_slug, quality_tier
             FROM songs
             WHERE id IN (SELECT value FROM json_each(?1))",
        )
        .bind(serde_json::to_string(ids).unwrap())
        .fetch_all(&self.pool)
        .await
    }

    pub async fn require_song_by_id(&self, song_id: i64) -> Result<Song, String> {
        match self.get_song_by_id(song_id).await {
            Ok(Some(song)) => Ok(song),
            Ok(None) => {
                let msg = format!("Song ID {} not found in database", song_id);
                eprintln!("[Playback Error] {}", msg);
                Err(msg)
            }
            Err(err) => {
                let msg = format!("SQLx failed to decode song ID {}: {:?}", song_id, err);
                eprintln!("[Playback Error] {}", msg);
                Err(msg)
            }
        }
    }

    pub async fn delete_songs(&self, ids: Vec<i64>) -> Result<(), sqlx::Error> {
        if ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for id in ids {
            sqlx::query("DELETE FROM songs WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn create_playlist(
        &self,
        name: &str,
        description: Option<&str>,
        cover_url: Option<&str>,
    ) -> Result<i64, sqlx::Error> {
        let result =
            sqlx::query("INSERT INTO playlists (name, description, cover_url) VALUES (?, ?, ?)")
                .bind(name)
                .bind(description)
                .bind(cover_url)
                .execute(&self.pool)
                .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_all_playlists(&self) -> Result<Vec<Playlist>, sqlx::Error> {
        sqlx::query_as::<_, Playlist>(
            "SELECT
                p.id,
                p.name,
                p.description,
                p.cover_url,
                p.date_added,
                p.date_modified,
                COUNT(ps.song_id) as track_count,
                COALESCE(SUM(s.duration_ms), 0) as total_duration_ms,
                (
                    SELECT GROUP_CONCAT(cover_url, ',')
                    FROM (
                        SELECT DISTINCT s_sub.cover_url
                        FROM songs s_sub
                        JOIN playlist_songs ps_sub ON s_sub.id = ps_sub.song_id
                        WHERE ps_sub.playlist_id = p.id AND s_sub.cover_url IS NOT NULL
                        ORDER BY ps_sub.position ASC
                        LIMIT 4
                    )
                ) as preview_covers
             FROM playlists p
             LEFT JOIN playlist_songs ps ON p.id = ps.playlist_id
             LEFT JOIN songs s ON ps.song_id = s.id
             GROUP BY p.id
             ORDER BY p.date_added DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_playlist_by_id(
        &self,
        playlist_id: i64,
    ) -> Result<Option<Playlist>, sqlx::Error> {
        sqlx::query_as::<_, Playlist>(
            "SELECT
                p.id,
                p.name,
                p.description,
                p.cover_url,
                p.date_added,
                p.date_modified,
                COUNT(ps.song_id) as track_count,
                COALESCE(SUM(s.duration_ms), 0) as total_duration_ms,
                (
                    SELECT GROUP_CONCAT(cover_url, ',')
                    FROM (
                        SELECT DISTINCT s_sub.cover_url
                        FROM songs s_sub
                        JOIN playlist_songs ps_sub ON s_sub.id = ps_sub.song_id
                        WHERE ps_sub.playlist_id = p.id AND s_sub.cover_url IS NOT NULL
                        ORDER BY ps_sub.position ASC
                        LIMIT 4
                    )
                ) as preview_covers
             FROM playlists p
             LEFT JOIN playlist_songs ps ON p.id = ps.playlist_id
             LEFT JOIN songs s ON ps.song_id = s.id
             WHERE p.id = ?
             GROUP BY p.id",
        )
        .bind(playlist_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn update_playlist_details(
        &self,
        playlist_id: i64,
        name: Option<&str>,
        description: Option<&str>,
        cover_url: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE playlists
             SET name = ?, description = ?, cover_url = ?, date_modified = CURRENT_TIMESTAMP
             WHERE id = ?",
        )
        .bind(name)
        .bind(description)
        .bind(cover_url)
        .bind(playlist_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_playlist(&self, playlist_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM playlists WHERE id = ?")
            .bind(playlist_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_playlist_songs(&self, playlist_id: i64) -> Result<Vec<Song>, sqlx::Error> {
        sqlx::query_as::<_, Song>(
            "SELECT s.id, s.path, s.title, s.artists, s.album_artist, s.album, s.track_number,
                    s.duration_ms, s.cover_url, s.external_cover_url, s.genre, s.release_year,
                    s.lyrics, s.lyrics_source, s.source, s.source_url, s.source_item_id,
                    s.canonical_track_slug, s.canonical_album_slug, s.quality_tier
             FROM songs s
             JOIN playlist_songs ps ON s.id = ps.song_id
             WHERE ps.playlist_id = ?
             ORDER BY ps.position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn add_songs_to_playlist(
        &self,
        playlist_id: i64,
        song_ids: &[i64],
    ) -> Result<(), sqlx::Error> {
        if song_ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        let current_max: Option<i64> =
            sqlx::query_scalar("SELECT MAX(position) FROM playlist_songs WHERE playlist_id = ?")
                .bind(playlist_id)
                .fetch_optional(&mut *tx)
                .await?;

        let mut next_position = current_max.unwrap_or(-1) + 1;

        for song_id in song_ids {
            sqlx::query(
                "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES (?, ?, ?)",
            )
            .bind(playlist_id)
            .bind(song_id)
            .bind(next_position)
            .execute(&mut *tx)
            .await?;

            next_position += 1;
        }

        sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn remove_songs_from_playlist(
        &self,
        playlist_id: i64,
        positions: &[i64],
    ) -> Result<(), sqlx::Error> {
        if positions.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        // Sort descending to delete from highest index to lowest.
        // This prevents the positions of remaining target items from shifting before they are deleted.
        let mut sorted_positions = positions.to_vec();
        sorted_positions.sort_unstable_by(|a, b| b.cmp(a));

        for pos in sorted_positions {
            sqlx::query("DELETE FROM playlist_songs WHERE playlist_id = ? AND position = ?")
                .bind(playlist_id)
                .bind(pos)
                .execute(&mut *tx)
                .await?;
        }

        // Re-index remaining items to keep positions continuous (0, 1, 2...)
        // Using rowid ensures we uniquely identify the row even if there are duplicate songs.
        let remaining_songs: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT rowid, song_id FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&mut *tx)
        .await?;

        for (new_pos, (row_id, _song_id)) in remaining_songs.iter().enumerate() {
            sqlx::query("UPDATE playlist_songs SET position = ? WHERE rowid = ?")
                .bind(new_pos as i64)
                .bind(row_id)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn reorder_playlist_song(
        &self,
        playlist_id: i64,
        from_position: i64,
        to_position: i64,
    ) -> Result<(), Error> {
        if from_position < 1 || to_position < 1 {
            return Err(Error::RowNotFound);
        }

        if from_position == to_position {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM playlist_songs
         WHERE playlist_id = ? AND position = ?",
        )
        .bind(playlist_id)
        .bind(from_position)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((song_id,)) = row else {
            return Err(Error::RowNotFound);
        };

        let max_position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), 0)
         FROM playlist_songs
         WHERE playlist_id = ?",
        )
        .bind(playlist_id)
        .fetch_one(&mut *tx)
        .await?;

        if to_position > max_position {
            return Err(Error::RowNotFound);
        }

        let offset = max_position.checked_add(2).ok_or(Error::RowNotFound)?;

        let parked_position = max_position.checked_add(1).ok_or(Error::RowNotFound)?;

        // Park the song being moved.
        sqlx::query("UPDATE playlist_songs SET position = ? WHERE id = ?")
            .bind(parked_position)
            .bind(song_id)
            .execute(&mut *tx)
            .await?;

        if from_position < to_position {
            // Move intervening songs into a temporary range.
            sqlx::query(
                "UPDATE playlist_songs
             SET position = position + ?
             WHERE playlist_id = ?
               AND position > ?
               AND position <= ?",
            )
            .bind(offset)
            .bind(playlist_id)
            .bind(from_position)
            .bind(to_position)
            .execute(&mut *tx)
            .await?;

            // Restore them at their final positions.
            sqlx::query(
                "UPDATE playlist_songs
             SET position = position - ? - 1
             WHERE playlist_id = ?
               AND position > ?
               AND position <= ?",
            )
            .bind(offset)
            .bind(playlist_id)
            .bind(offset)
            .bind(offset.checked_add(to_position).ok_or(Error::RowNotFound)?)
            .execute(&mut *tx)
            .await?;
        } else {
            // Move intervening songs into a temporary range.
            sqlx::query(
                "UPDATE playlist_songs
             SET position = position + ?
             WHERE playlist_id = ?
               AND position >= ?
               AND position < ?",
            )
            .bind(offset)
            .bind(playlist_id)
            .bind(to_position)
            .bind(from_position)
            .execute(&mut *tx)
            .await?;

            // Restore them at their final positions.
            sqlx::query(
                "UPDATE playlist_songs
             SET position = position - ? + 1
             WHERE playlist_id = ?
               AND position >= ?
               AND position < ?",
            )
            .bind(offset)
            .bind(playlist_id)
            .bind(offset.checked_add(to_position).ok_or(Error::RowNotFound)?)
            .bind(
                offset
                    .checked_add(from_position)
                    .ok_or(Error::RowNotFound)?,
            )
            .execute(&mut *tx)
            .await?;
        }

        // Place the moved song at its destination.
        sqlx::query("UPDATE playlist_songs SET position = ? WHERE id = ?")
            .bind(to_position)
            .bind(song_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "UPDATE playlists
         SET date_modified = CURRENT_TIMESTAMP
         WHERE id = ?",
        )
        .bind(playlist_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    // pub async fn remove_songs_from_playlist(
    //     &self,
    //     playlist_id: i64,
    //     song_ids: &[i64],
    // ) -> Result<(), sqlx::Error> {
    //     if song_ids.is_empty() {
    //         return Ok(());
    //     }
    //
    //     let mut tx = self.pool.begin().await?;
    //
    //     for song_id in song_ids {
    //         sqlx::query("DELETE FROM playlist_songs WHERE playlist_id = ? AND song_id = ?")
    //             .bind(playlist_id)
    //             .bind(song_id)
    //             .execute(&mut *tx)
    //             .await?;
    //     }
    //
    //     // Re-index remaining items to keep positions continuous
    //     let remaining_songs: Vec<i64> = sqlx::query_scalar(
    //         "SELECT song_id FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
    //     )
    //     .bind(playlist_id)
    //     .fetch_all(&mut *tx)
    //     .await?;
    //
    //     for (new_pos, song_id) in remaining_songs.iter().enumerate() {
    //         sqlx::query(
    //             "UPDATE playlist_songs SET position = ? WHERE playlist_id = ? AND song_id = ?",
    //         )
    //         .bind(new_pos as i64)
    //         .bind(playlist_id)
    //         .bind(song_id)
    //         .execute(&mut *tx)
    //         .await?;
    //     }
    //
    //     sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
    //         .bind(playlist_id)
    //         .execute(&mut *tx)
    //         .await?;
    //
    //     tx.commit().await?;
    //     Ok(())
    // }

    pub async fn reorder_playlist_songs(
        &self,
        playlist_id: i64,
        song_ids: &[i64],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for (position, song_id) in song_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE playlist_songs SET position = ? WHERE playlist_id = ? AND song_id = ?",
            )
            .bind(position as i64)
            .bind(playlist_id)
            .bind(song_id)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}
