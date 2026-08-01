use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ogg::PacketReader;
use opus::{Channels, Decoder as OpusDecoder};
use rodio::Source;

pub struct OpusSource<R: Read + Seek> {
    packet_reader: PacketReader<R>,
    decoder: OpusDecoder,
    audio_stream_serial: u32,
    channels: NonZeroU16,
    sample_rate: NonZeroU32,
    buffer: Vec<f32>,
    buffer_idx: usize,
    current_sample: u64,
    path: PathBuf,
}

impl OpusSource<BufReader<File>> {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path_buf = path.as_ref().to_path_buf();
        let file = File::open(&path_buf)?;
        let reader = BufReader::new(file);
        let mut packet_reader = PacketReader::new(reader);

        let id_packet = packet_reader.read_packet_expected()?;
        if id_packet.data.len() < 19 || &id_packet.data[0..8] != b"OpusHead" {
            return Err("Invalid Opus identification header".into());
        }

        let audio_stream_serial = id_packet.stream_serial();
        let raw_channels = id_packet.data[9] as u16;
        let channels = NonZeroU16::new(raw_channels).ok_or("Invalid channel count: 0")?;
        let sample_rate = NonZeroU32::new(48000).unwrap();

        let _comment_packet = packet_reader.read_packet_expected()?;

        let opus_channels = match channels.get() {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => return Err(format!("Unsupported Opus channel count: {}", channels.get()).into()),
        };

        let decoder = OpusDecoder::new(sample_rate.get(), opus_channels)?;

        Ok(Self {
            packet_reader,
            decoder,
            audio_stream_serial,
            channels,
            sample_rate,
            buffer: Vec::new(),
            buffer_idx: 0,
            current_sample: 0,
            path: path_buf,
        })
    }
}

impl<R: Read + Seek> Iterator for OpusSource<R> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer_idx >= self.buffer.len() {
            loop {
                match self.packet_reader.read_packet() {
                    Ok(Some(packet)) => {
                        if packet.stream_serial() != self.audio_stream_serial {
                            continue;
                        }

                        let mut pcm = vec![0.0f32; 5760 * self.channels.get() as usize];
                        match self.decoder.decode_float(&packet.data, &mut pcm, false) {
                            Ok(samples_per_channel) => {
                                let total_samples =
                                    samples_per_channel * self.channels.get() as usize;
                                pcm.truncate(total_samples);
                                self.buffer = pcm;
                                self.buffer_idx = 0;
                                break;
                            }
                            Err(_) => continue,
                        }
                    }
                    Ok(None) | Err(_) => return None,
                }
            }
        }

        let sample = self.buffer[self.buffer_idx];
        self.buffer_idx += 1;
        self.current_sample += 1;
        Some(sample)
    }
}

impl<R: Read + Seek> Source for OpusSource<R> {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len() - self.buffer_idx)
    }

    fn channels(&self) -> NonZeroU16 {
        self.channels
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        // Opus in Ogg always operates on a 48,000 Hz granule rate,
        // regardless of channel count or actual output sample rate.
        let target_granule = (pos.as_secs_f64() * 48_000.0) as u64;

        // Perform a fast binary search on Ogg pages to find the target time
        let seek_success = self
            .packet_reader
            .seek_absgp(Some(self.audio_stream_serial), target_granule)
            .map_err(|_| rodio::source::SeekError::NotSupported {
                underlying_source: std::any::type_name::<Self>(),
            })?;

        if !seek_success {
            // Seek failed (e.g., pos is past the end of the file)
            return Err(rodio::source::SeekError::NotSupported {
                underlying_source: std::any::type_name::<Self>(),
            });
        }

        // Reset decoder state so old predictive data doesn't glitch the newly seeked audio
        self.decoder
            .reset_state()
            .map_err(|_| rodio::source::SeekError::NotSupported {
                underlying_source: std::any::type_name::<Self>(),
            })?;

        // Clear old samples from our buffer
        self.buffer.clear();
        self.buffer_idx = 0;

        // Sync the internal sample tracker for Iterator to our new position
        self.current_sample =
            (pos.as_secs_f64() * self.sample_rate.get() as f64 * self.channels.get() as f64) as u64;

        Ok(())
    }
}
