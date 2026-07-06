use blacktape_desktop_lib::scan::{old_scan_music_dir, scan_music_dir};
use criterion::{criterion_group, criterion_main, Criterion};
use std::path::PathBuf;

fn bench_music_scan(c: &mut Criterion) {
    let music_dir = String::from("Z:\\music");
    let covers_dir =
        PathBuf::from("C:\\Users\\seafood\\AppData\\Roaming\\dev.seafood.blacktape\\covers");

    let mut group = c.benchmark_group("Music Scanner Comparison");

    group.bench_function("Sequential (Old)", |b| {
        b.iter(|| {
            let _songs = old_scan_music_dir(music_dir.clone(), &covers_dir);
        })
    });
    group.bench_function("Parallel (New)", |b| {
        b.iter(|| {
            let _songs = scan_music_dir(music_dir.clone(), &covers_dir);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_music_scan);
criterion_main!(benches);
