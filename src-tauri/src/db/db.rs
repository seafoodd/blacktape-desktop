use crate::db::schema::{ALBUM_SOURCES_SCHEMA_SQL, TRASH_SCHEMA_SQL};
use crate::lyrics::LyricsSource;
use crate::types::{Album, ArtistSummary, Playlist, Song};
use sqlx::sqlite::{SqliteConnection, SqlitePool};
use sqlx::Error;

pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &str) -> Self {
        let pool = SqlitePool::connect(&format!("sqlite:{db_path}"))
            .await
            .expect("Failed to connect to database");
        let db = Self { pool };
        // The trash tables must exist before the first scan writes `library_dir`.
        db.ensure_trash_schema()
            .await
            .expect("Failed to create trash tables");
        db.ensure_album_sources_schema()
            .await
            .expect("Failed to create album sources table");
        db
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

    /// Updates user-editable metadata and recomputes the canonical slugs.
    pub async fn update_song_info(
        &self,
        id: i64,
        title: &str,
        artists: &[String],
        album: &str,
        album_artist: &str,
        track_number: Option<i32>,
        genre: Option<&str>,
        release_year: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE songs SET title = ?, artists = ?, album = ?, album_artist = ?,
                track_number = ?, genre = ?, release_year = ?,
                canonical_track_slug = ?, canonical_album_slug = ?
             WHERE id = ?",
        )
        .bind(title)
        .bind(sqlx::types::Json(artists))
        .bind(album)
        .bind(album_artist)
        .bind(track_number)
        .bind(genre)
        .bind(release_year)
        .bind(crate::utils::make_canonical_slug(album_artist, title))
        .bind(crate::utils::make_canonical_slug(album_artist, album))
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
                source_url = CASE WHEN COALESCE(excluded.source_url, '') = '' AND excluded.source = songs.source THEN songs.source_url ELSE excluded.source_url END,
                source_item_id = CASE WHEN COALESCE(excluded.source_item_id, '') = '' AND excluded.source = songs.source THEN songs.source_item_id ELSE excluded.source_item_id END,
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
                    source_url = CASE WHEN COALESCE(excluded.source_url, '') = '' AND excluded.source = songs.source THEN songs.source_url ELSE excluded.source_url END,
                    source_item_id = CASE WHEN COALESCE(excluded.source_item_id, '') = '' AND excluded.source = songs.source THEN songs.source_item_id ELSE excluded.source_item_id END,
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

        let mut next_position = current_max.unwrap_or(0) + 1;

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

        // If the caller passed 0-based indices (e.g. containing 0), convert to 1-based positions
        let is_zero_based = positions.iter().any(|&p| p <= 0);
        let mut sorted_positions: Vec<i64> = if is_zero_based {
            positions.iter().map(|&p| p + 1).collect()
        } else {
            positions.to_vec()
        };

        // Sort descending to delete from highest position to lowest.
        sorted_positions.sort_unstable_by(|a, b| b.cmp(a));
        sorted_positions.dedup();

        for pos in sorted_positions {
            sqlx::query("DELETE FROM playlist_songs WHERE playlist_id = ? AND position = ?")
                .bind(playlist_id)
                .bind(pos)
                .execute(&mut *tx)
                .await?;
        }

        // Re-index remaining items to keep positions continuous (1, 2, 3...)
        // Using rowid ensures we uniquely identify the row even if there are duplicate songs.
        let remaining_songs: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT rowid, position FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&mut *tx)
        .await?;

        for (idx, (row_id, old_pos)) in remaining_songs.iter().enumerate() {
            let new_pos = (idx + 1) as i64;
            if *old_pos != new_pos {
                sqlx::query("UPDATE playlist_songs SET position = ? WHERE rowid = ?")
                    .bind(new_pos)
                    .bind(row_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Moves several rows (1-based `positions`) as one block so that they end up
    /// before the row that was at `insert_before` (1-based, `len + 1` = the end).
    pub async fn move_playlist_songs(
        &self,
        playlist_id: i64,
        positions: &[i64],
        insert_before: i64,
    ) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;

        let ids: Vec<(i64,)> = sqlx::query_as(
            "SELECT id FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&mut *tx)
        .await?;
        let len = ids.len();

        if insert_before < 1
            || insert_before as usize > len + 1
            || positions.is_empty()
            || positions.iter().any(|&p| p < 1 || p as usize > len)
        {
            return Err(Error::RowNotFound);
        }

        let selected: Vec<usize> = positions.iter().map(|&p| p as usize - 1).collect();
        let order = crate::playlists::ordering::move_block_order(
            len,
            &selected,
            insert_before as usize - 1,
        );

        // Park every row above the final range first (positions are UNIQUE per playlist).
        sqlx::query("UPDATE playlist_songs SET position = position + ? WHERE playlist_id = ?")
            .bind(len as i64 + 1)
            .bind(playlist_id)
            .execute(&mut *tx)
            .await?;

        for (new_index, old_index) in order.iter().enumerate() {
            sqlx::query("UPDATE playlist_songs SET position = ? WHERE id = ?")
                .bind(new_index as i64 + 1)
                .bind(ids[*old_index].0)
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

/// A row of `trash_items`; see `trash.rs` for what each field means.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrashRow {
    pub id: i64,
    pub song_json: String,
    pub original_path: String,
    pub trash_path: String,
    pub size_bytes: i64,
    pub trashed_at: i64,
}

/// Inserts a full song row with an explicit id (`None` lets SQLite pick one).
async fn insert_song_tx(
    conn: &mut SqliteConnection,
    song: &Song,
    id: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO songs (
            id, path, title, artists, album_artist, album, track_number,
            duration_ms, cover_url, external_cover_url, genre, release_year,
            lyrics, lyrics_source, source, source_url, source_item_id,
            canonical_track_slug, canonical_album_slug, quality_tier
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            source_url = CASE WHEN COALESCE(excluded.source_url, '') = '' AND excluded.source = songs.source THEN songs.source_url ELSE excluded.source_url END,
            source_item_id = CASE WHEN COALESCE(excluded.source_item_id, '') = '' AND excluded.source = songs.source THEN songs.source_item_id ELSE excluded.source_item_id END,
            canonical_track_slug = excluded.canonical_track_slug,
            canonical_album_slug = excluded.canonical_album_slug,
            quality_tier = excluded.quality_tier",
    )
    .bind(id)
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
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Renumbers a playlist's positions to 1, 2, 3... (keeps the same order). Ascending
/// processing is safe because every new position is <= the old one.
async fn reindex_playlist_tx(
    conn: &mut SqliteConnection,
    playlist_id: i64,
) -> Result<(), sqlx::Error> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT rowid, position FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
    )
    .bind(playlist_id)
    .fetch_all(&mut *conn)
    .await?;

    for (idx, (row_id, old_pos)) in rows.iter().enumerate() {
        let new_pos = (idx + 1) as i64;
        if *old_pos != new_pos {
            sqlx::query("UPDATE playlist_songs SET position = ? WHERE rowid = ?")
                .bind(new_pos)
                .bind(row_id)
                .execute(&mut *conn)
                .await?;
        }
    }

    sqlx::query("UPDATE playlists SET date_modified = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(playlist_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Removes a song row and its playlist links, then closes the gaps in those playlists (D6).
async fn detach_song_tx(conn: &mut SqliteConnection, song_id: i64) -> Result<(), sqlx::Error> {
    let playlist_ids: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT playlist_id FROM playlist_songs WHERE song_id = ?")
            .bind(song_id)
            .fetch_all(&mut *conn)
            .await?;

    sqlx::query("DELETE FROM playlist_songs WHERE song_id = ?")
        .bind(song_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM songs WHERE id = ?")
        .bind(song_id)
        .execute(&mut *conn)
        .await?;

    for playlist_id in playlist_ids {
        reindex_playlist_tx(&mut *conn, playlist_id).await?;
    }
    Ok(())
}

impl Database {
    /// Creates the trash tables if they are missing (D10). Safe to run repeatedly.
    pub async fn ensure_trash_schema(&self) -> Result<(), sqlx::Error> {
        sqlx::query(TRASH_SCHEMA_SQL).execute(&self.pool).await?;
        Ok(())
    }

    /// Creates the `album_sources` table if it is missing (migration 7 does the same). Safe to run repeatedly.
    pub async fn ensure_album_sources_schema(&self) -> Result<(), sqlx::Error> {
        sqlx::query(ALBUM_SOURCES_SCHEMA_SQL).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_app_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn set_app_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Oldest first (trashed_at ASC, id ASC).
    pub async fn list_trash_rows(&self) -> Result<Vec<TrashRow>, sqlx::Error> {
        sqlx::query_as::<_, TrashRow>(
            "SELECT id, song_json, original_path, trash_path, size_bytes, trashed_at
             FROM trash_items
             ORDER BY trashed_at ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_trash_row(&self, id: i64) -> Result<Option<TrashRow>, sqlx::Error> {
        sqlx::query_as::<_, TrashRow>(
            "SELECT id, song_json, original_path, trash_path, size_bytes, trashed_at
             FROM trash_items
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_trash_item(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM trash_items WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Removes the song's row and playlist links and records its trash item, in one
    /// transaction. Returns the new trash item id. The file itself is moved by the caller.
    pub async fn move_song_to_trash(
        &self,
        song_id: i64,
        song_json: &str,
        original_path: &str,
        trash_path: &str,
        size_bytes: i64,
        trashed_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        detach_song_tx(&mut *tx, song_id).await?;
        let result = sqlx::query(
            "INSERT INTO trash_items (song_json, original_path, trash_path, size_bytes, trashed_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(song_json)
        .bind(original_path)
        .bind(trash_path)
        .bind(size_bytes)
        .bind(trashed_at)
        .execute(&mut *tx)
        .await?;
        let trash_id = result.last_insert_rowid();
        tx.commit().await?;
        Ok(trash_id)
    }

    /// Removes a song whose file is already gone: its row and playlist links only.
    pub async fn remove_song_rows(&self, song_id: i64) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        detach_song_tx(&mut *tx, song_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Re-inserts a restored song and deletes its trash item in one transaction. The
    /// song keeps its old id when that id is free, otherwise SQLite assigns a new one
    /// (D16). Returns the id the song has in the library now.
    pub async fn restore_trashed_song(
        &self,
        song: &Song,
        trash_id: i64,
    ) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let explicit_id = match song.id {
            Some(id) => {
                let taken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM songs WHERE id = ?")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await?;
                (taken == 0).then_some(id)
            }
            None => None,
        };

        insert_song_tx(&mut *tx, song, explicit_id).await?;

        // An upsert on an existing path keeps that row's id, so look the id up by path.
        let id: i64 = sqlx::query_scalar("SELECT id FROM songs WHERE path = ?")
            .bind(&song.path)
            .fetch_one(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM trash_items WHERE id = ?")
            .bind(trash_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::{Platform, QualityTier};

    pub(crate) async fn setup_test_db() -> Database {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT,
                cover_url TEXT,
                date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
                date_modified DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS songs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                album_artist TEXT NOT NULL DEFAULT 'Unknown Artist',
                album TEXT,
                track_number INTEGER,
                duration_ms INTEGER NOT NULL,
                cover_url TEXT,
                external_cover_url TEXT,
                source_url TEXT,
                genre TEXT,
                release_year INTEGER,
                date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
                lyrics TEXT,
                lyrics_source TEXT,
                artists TEXT NOT NULL DEFAULT '[]',
                source TEXT NOT NULL DEFAULT 'local',
                source_item_id TEXT,
                canonical_track_slug TEXT NOT NULL DEFAULT '',
                canonical_album_slug TEXT NOT NULL DEFAULT '',
                quality_tier TEXT NOT NULL DEFAULT 'standard'
            );
            CREATE TABLE IF NOT EXISTS playlist_songs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                playlist_id INTEGER NOT NULL,
                song_id INTEGER NOT NULL,
                position INTEGER NOT NULL CHECK (position > 0),
                date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
                FOREIGN KEY (song_id) REFERENCES songs(id) ON DELETE CASCADE,
                UNIQUE(playlist_id, position)
            );",
        )
        .execute(&pool)
        .await
        .unwrap();

        Database { pool }
    }

    #[tokio::test]
    async fn test_add_and_remove_songs_from_playlist() {
        let db = setup_test_db().await;
        let playlist_id = db
            .create_playlist("Test Playlist", None, None)
            .await
            .unwrap();

        // Insert mock songs
        sqlx::query("INSERT INTO songs (path, title, duration_ms) VALUES ('/a.mp3', 'A', 1000), ('/b.mp3', 'B', 2000), ('/c.mp3', 'C', 3000)")
            .execute(&db.pool)
            .await
            .unwrap();

        // Add songs to empty playlist: should have positions 1, 2, 3
        db.add_songs_to_playlist(playlist_id, &[1, 2, 3])
            .await
            .unwrap();

        let songs = db.get_playlist_songs(playlist_id).await.unwrap();
        assert_eq!(songs.len(), 3);

        // Remove 1st song (passed as 0-based index [0])
        db.remove_songs_from_playlist(playlist_id, &[0])
            .await
            .unwrap();

        let remaining = db.get_playlist_songs(playlist_id).await.unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].title, "B");
        assert_eq!(remaining[1].title, "C");

        // Verify positions in DB are 1 and 2
        let positions: Vec<i64> = sqlx::query_scalar(
            "SELECT position FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&db.pool)
        .await
        .unwrap();
        assert_eq!(positions, vec![1, 2]);

        // Remove 2nd song (passed as 1-based position [2])
        db.remove_songs_from_playlist(playlist_id, &[2])
            .await
            .unwrap();
        let remaining_after = db.get_playlist_songs(playlist_id).await.unwrap();
        assert_eq!(remaining_after.len(), 1);
        assert_eq!(remaining_after[0].title, "B");
    }

    #[tokio::test]
    async fn test_delete_songs_removes_rows_and_playlist_links() {
        let db = setup_test_db().await;
        let playlist_id = db
            .create_playlist("Test Playlist", None, None)
            .await
            .unwrap();

        sqlx::query("INSERT INTO songs (path, title, duration_ms) VALUES ('/a.mp3', 'A', 1000), ('/b.mp3', 'B', 2000), ('/c.mp3', 'C', 3000)")
            .execute(&db.pool)
            .await
            .unwrap();

        db.add_songs_to_playlist(playlist_id, &[1, 2, 3])
            .await
            .unwrap();

        // Delete the middle song
        db.delete_songs(vec![2]).await.unwrap();

        let song_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM songs ORDER BY id ASC")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        assert_eq!(song_ids, vec![1, 3]);

        let songs = db.get_playlist_songs(playlist_id).await.unwrap();
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].title, "A");
        assert_eq!(songs[1].title, "C");

        // Empty input is a no-op
        db.delete_songs(vec![]).await.unwrap();

        let song_ids_after: Vec<i64> = sqlx::query_scalar("SELECT id FROM songs ORDER BY id ASC")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        assert_eq!(song_ids_after, vec![1, 3]);
        let songs_after = db.get_playlist_songs(playlist_id).await.unwrap();
        assert_eq!(songs_after.len(), 2);
    }

    #[tokio::test]
    async fn test_update_song_info_updates_fields_and_slugs() {
        let db = setup_test_db().await;

        sqlx::query(
            "INSERT INTO songs (path, title, duration_ms, album_artist, album, track_number) VALUES ('/a.mp3', 'Old Title', 1000, 'Old Artist', 'Old Album', 1)",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        db.update_song_info(
            1,
            "New Title",
            &["A".to_string(), "B".to_string()],
            "New Album",
            "A",
            Some(3),
            Some("Rock"),
            Some(1999),
        )
        .await
        .unwrap();

        let songs = db.get_songs_by_ids(&[1]).await.unwrap();
        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(song.title, "New Title");
        assert_eq!(song.artists, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(song.album, "New Album");
        assert_eq!(song.album_artist, "A");
        assert_eq!(song.track_number, Some(3));
        assert_eq!(song.genre.as_deref(), Some("Rock"));
        assert_eq!(song.release_year, Some(1999));
        assert_eq!(song.canonical_track_slug, "a-new-title");
        assert_eq!(song.canonical_album_slug, "a-new-album");

        // Clearing the track number stores NULL
        db.update_song_info(1, "New Title", &["A".to_string()], "New Album", "A", None, None, None)
            .await
            .unwrap();

        let songs = db.get_songs_by_ids(&[1]).await.unwrap();
        assert_eq!(songs[0].track_number, None);
    }

    /// Creates a playlist whose rows are `song_for_row` in order (position i+1 gets
    /// `song_for_row[i]`). Songs 1..=max are inserted first so the ids exist.
    async fn setup_move_playlist(db: &Database, song_for_row: &[i64]) -> i64 {
        let playlist_id = db.create_playlist("Moves", None, None).await.unwrap();
        let max_song = *song_for_row.iter().max().unwrap();
        for i in 1..=max_song {
            sqlx::query("INSERT INTO songs (path, title, duration_ms) VALUES (?, ?, 1000)")
                .bind(format!("/s{i}.mp3"))
                .bind(format!("S{i}"))
                .execute(&db.pool)
                .await
                .unwrap();
        }
        for (i, song_id) in song_for_row.iter().enumerate() {
            sqlx::query(
                "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?, ?, ?)",
            )
            .bind(playlist_id)
            .bind(*song_id)
            .bind(i as i64 + 1)
            .execute(&db.pool)
            .await
            .unwrap();
        }
        playlist_id
    }

    /// (playlist_songs.id, song_id, position) for every row, ordered by position.
    async fn playlist_rows(db: &Database, playlist_id: i64) -> Vec<(i64, i64, i64)> {
        sqlx::query_as(
            "SELECT id, song_id, position FROM playlist_songs WHERE playlist_id = ? ORDER BY position ASC",
        )
        .bind(playlist_id)
        .fetch_all(&db.pool)
        .await
        .unwrap()
    }

    fn row_ids(rows: &[(i64, i64, i64)]) -> Vec<i64> {
        rows.iter().map(|r| r.0).collect()
    }

    fn song_ids(rows: &[(i64, i64, i64)]) -> Vec<i64> {
        rows.iter().map(|r| r.1).collect()
    }

    fn positions(rows: &[(i64, i64, i64)]) -> Vec<i64> {
        rows.iter().map(|r| r.2).collect()
    }

    #[tokio::test]
    async fn test_move_playlist_songs_moves_block_and_renumbers() {
        let db = setup_test_db().await;
        // Row ids and song ids both 1..=5 in order.
        let playlist_id = setup_move_playlist(&db, &[1, 2, 3, 4, 5]).await;

        // Move positions 2 and 4 (S2, S4) before position 1.
        db.move_playlist_songs(playlist_id, &[2, 4], 1).await.unwrap();

        let rows = playlist_rows(&db, playlist_id).await;
        assert_eq!(song_ids(&rows), vec![2, 4, 1, 3, 5]);
        assert_eq!(row_ids(&rows), vec![2, 4, 1, 3, 5]);
        // No gaps or duplicates: positions are exactly 1..=5.
        assert_eq!(positions(&rows), vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_move_playlist_songs_moves_block_to_end() {
        let db = setup_test_db().await;
        let playlist_id = setup_move_playlist(&db, &[1, 2, 3, 4, 5]).await;

        // Move positions 1 and 2 before position 6 (len + 1 = end).
        db.move_playlist_songs(playlist_id, &[1, 2], 6).await.unwrap();

        let rows = playlist_rows(&db, playlist_id).await;
        assert_eq!(song_ids(&rows), vec![3, 4, 5, 1, 2]);
        assert_eq!(positions(&rows), vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_move_playlist_songs_moves_by_position_not_song_id() {
        let db = setup_move_playlist_with_duplicates().await;
        // Playlist: S1, S2, S1, S3, S1 (song 1 appears at positions 1, 3, 5).
        let playlist_id = 1;

        // Move only the row at position 3 (a duplicate of S1) before position 1.
        db.move_playlist_songs(playlist_id, &[3], 1).await.unwrap();

        let rows = playlist_rows(&db, playlist_id).await;
        assert_eq!(row_ids(&rows), vec![3, 1, 2, 4, 5]);
        assert_eq!(song_ids(&rows), vec![1, 1, 2, 3, 1]);
        assert_eq!(positions(&rows), vec![1, 2, 3, 4, 5]);

        // Current order: ids [3,1,2,4,5]. Move positions 1 and 3 (ids 3 and 2) to the end.
        db.move_playlist_songs(playlist_id, &[1, 3], 6).await.unwrap();

        let rows = playlist_rows(&db, playlist_id).await;
        assert_eq!(row_ids(&rows), vec![1, 4, 5, 3, 2]);
        assert_eq!(song_ids(&rows), vec![1, 3, 1, 1, 2]);
        assert_eq!(positions(&rows), vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_move_playlist_songs_noop_drops_keep_order() {
        let db = setup_test_db().await;
        let playlist_id = setup_move_playlist(&db, &[1, 2, 3, 4, 5]).await;
        let before = playlist_rows(&db, playlist_id).await;

        // Dropping a row right before itself or right after itself changes nothing.
        db.move_playlist_songs(playlist_id, &[2], 2).await.unwrap();
        db.move_playlist_songs(playlist_id, &[2], 3).await.unwrap();

        assert_eq!(playlist_rows(&db, playlist_id).await, before);
    }

    #[tokio::test]
    async fn test_move_playlist_songs_rejects_invalid_input_without_changes() {
        let db = setup_test_db().await;
        let playlist_id = setup_move_playlist(&db, &[1, 2, 3, 4, 5]).await;
        let before = playlist_rows(&db, playlist_id).await;

        let empty: &[i64] = &[];
        let cases: Vec<(&[i64], i64)> = vec![
            (&[0], 1),          // position 0
            (&[6], 1),          // position > len
            (&[-1], 1),         // negative position
            (&[1, 6], 2),       // one bad position among good ones
            (&[1], 0),          // insert_before 0
            (&[1], 7),          // insert_before len + 2
            (empty, 1),         // empty positions
        ];

        for (positions, insert_before) in cases {
            let result = db
                .move_playlist_songs(playlist_id, positions, insert_before)
                .await;
            assert!(
                result.is_err(),
                "expected error for positions={positions:?} insert_before={insert_before}"
            );
            assert_eq!(
                playlist_rows(&db, playlist_id).await,
                before,
                "order changed for positions={positions:?} insert_before={insert_before}"
            );
        }
    }

    /// Playlist (playlist id 1) with rows S1, S2, S1, S3, S1.
    async fn setup_move_playlist_with_duplicates() -> Database {
        let db = setup_test_db().await;
        setup_move_playlist(&db, &[1, 2, 1, 3, 1]).await;
        db
    }

    fn make_source_song(
        path: &str,
        source: Platform,
        source_url: Option<&str>,
        source_item_id: Option<&str>,
    ) -> Song {
        Song {
            id: None,
            path: path.to_string(),
            title: "Track".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_ms: 1000,
            track_number: Some(1),
            genre: None,
            release_year: None,
            cover_url: None,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source,
            source_url: source_url.map(str::to_string),
            source_item_id: source_item_id.map(str::to_string),
            canonical_track_slug: "artist-track".to_string(),
            canonical_album_slug: "artist-album".to_string(),
            quality_tier: QualityTier::Standard,
        }
    }

    async fn only_song(db: &Database) -> Song {
        let mut songs = db.get_all_songs().await.unwrap();
        assert_eq!(songs.len(), 1, "expected exactly one stored song");
        songs.remove(0)
    }

    #[tokio::test]
    async fn test_insert_songs_keeps_source_when_reinserted_with_nulls() {
        let db = setup_test_db().await;
        let first = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/a"),
            Some("a"),
        );
        db.insert_songs(vec![first]).await.unwrap();

        let second = make_source_song("/m/a.mp3", Platform::Bandcamp, None, None);
        db.insert_songs(vec![second]).await.unwrap();

        let song = only_song(&db).await;
        assert_eq!(song.source, Platform::Bandcamp);
        assert_eq!(song.source_url.as_deref(), Some("https://x.bandcamp.com/track/a"));
        assert_eq!(song.source_item_id.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn test_insert_songs_keeps_source_when_reinserted_with_empty_url() {
        let db = setup_test_db().await;
        let first = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/a"),
            Some("a"),
        );
        db.insert_songs(vec![first]).await.unwrap();

        let second = make_source_song("/m/a.mp3", Platform::Bandcamp, Some(""), Some(""));
        db.insert_songs(vec![second]).await.unwrap();

        let song = only_song(&db).await;
        assert_eq!(song.source_url.as_deref(), Some("https://x.bandcamp.com/track/a"));
        assert_eq!(song.source_item_id.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn test_insert_songs_overwrites_source_when_new_values_given() {
        let db = setup_test_db().await;
        let first = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/a"),
            Some("a"),
        );
        db.insert_songs(vec![first]).await.unwrap();

        let second = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/b"),
            Some("b"),
        );
        db.insert_songs(vec![second]).await.unwrap();

        let song = only_song(&db).await;
        assert_eq!(song.source_url.as_deref(), Some("https://x.bandcamp.com/track/b"));
        assert_eq!(song.source_item_id.as_deref(), Some("b"));
    }

    #[tokio::test]
    async fn test_insert_songs_clears_source_when_source_platform_changes() {
        let db = setup_test_db().await;
        let first = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/a"),
            Some("a"),
        );
        db.insert_songs(vec![first]).await.unwrap();

        let second = make_source_song("/m/a.mp3", Platform::Local, None, None);
        db.insert_songs(vec![second]).await.unwrap();

        let song = only_song(&db).await;
        assert_eq!(song.source, Platform::Local);
        assert_eq!(song.source_url, None);
        assert_eq!(song.source_item_id, None);
    }

    #[tokio::test]
    async fn test_insert_song_keeps_source_when_reinserted_with_nulls() {
        let db = setup_test_db().await;
        let first = make_source_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/a"),
            Some("a"),
        );
        db.insert_song(first).await.unwrap();

        let second = make_source_song("/m/a.mp3", Platform::Bandcamp, None, None);
        db.insert_song(second).await.unwrap();

        let song = only_song(&db).await;
        assert_eq!(song.source_url.as_deref(), Some("https://x.bandcamp.com/track/a"));
        assert_eq!(song.source_item_id.as_deref(), Some("a"));
    }
}
