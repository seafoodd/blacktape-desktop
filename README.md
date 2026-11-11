# Blacktape

A desktop music player and library manager. You can play your own files, search YouTube, Bandcamp and SoundCloud from the same search bar, and download what you find straight into your library.

I made it because I wanted one place for the music I own and the music I'm still looking for, without a browser tab for every site.

## What it does

**Player**
- Plays a folder of music files, with queue, shuffle, repeat and gapless playback.
- Streams a track from YouTube, Bandcamp or SoundCloud before you download it, so you can listen first.
- Lyrics: synced when it can find them (Musixmatch, LRCLIB), plain text otherwise (lyrics.ovh, Genius). You can edit them yourself.
- Media keys and the system media controls work, and it shows what you're playing in Discord.

**Library**
- Scans your music folder and keeps the library in a local database.
- Playlists with their own covers, and a warning when you add a song that is already there.
- Edit tags of a song or a whole album, saved to the file itself.
- Deleted songs go to an in-app trash first, so you can bring them back.
- Shows which tracks of an album are missing and helps you find them or look for a better version.

**Search and download**
- One search over YouTube, Bandcamp and SoundCloud, with a filter for each.
- Download a track or a whole album. You see the available formats and quality first (FLAC, MP3, Opus) before you pick one.
- Songs remember where they came from, so you can download them again in better quality later.
- You can sign in to your YouTube account in Settings.

**Other**
- Checks for updates and installs them from inside the app.

## Download

Windows builds are on the [Releases page](https://github.com/seafoodd/blacktape-desktop/releases). Other systems are not built yet, so for now you'd need to run it from source.

The app downloads [yt-dlp](https://github.com/yt-dlp/yt-dlp) by itself the first time it needs it, so the first download needs an internet connection.

## Run from source

You need [Node.js](https://nodejs.org) and [Rust](https://rustup.rs), plus whatever Tauri needs on your system ([prerequisites](https://tauri.app/start/prerequisites/)).

```
git clone https://github.com/seafoodd/blacktape-desktop
cd blacktape-desktop
npm install
npm run tauri dev
```

Tests:

```
npm test
cd src-tauri && cargo test
```

## Built with

- [Tauri 2](https://tauri.app) with a Rust backend: audio with [rodio](https://github.com/RustAudio/rodio) and Symphonia, tags with [lofty](https://github.com/Serial-ATA/lofty-rs), SQLite through sqlx, [souvlaki](https://github.com/Sinono3/souvlaki) for media controls
- React 19, TypeScript, Zustand and Vite on the frontend
- [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [rustypipe](https://codeberg.org/ThetaDev/rustypipe) for getting tracks from the sites

## Status

This is a personal project, still in development. Things will change and break, so back up your library before you try it on one you care about.

## Contributing

Contributions are welcome, but I may not review or merge everything. By sending a contribution you agree that it is licensed under GPL-3.0-only.

## License

Blacktape is licensed under the [GNU General Public License v3.0 only](./LICENSE) (`GPL-3.0-only`). Versions published before this change were released under the MIT License.

## Disclaimer

Blacktape is meant for personal use with content you have the right to access. You are responsible for following the terms of the platforms you use and the copyright laws of your country. The author does not host or distribute any music.
