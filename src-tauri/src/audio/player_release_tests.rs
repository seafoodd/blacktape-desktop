use super::*;

struct FailingSeekSource {
    samples: Vec<f32>,
    pos: usize,
}

impl Iterator for FailingSeekSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.samples.get(self.pos).copied()?;
        self.pos += 1;
        Some(sample)
    }
}

impl Source for FailingSeekSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.pos))
    }

    fn channels(&self) -> std::num::NonZero<u16> {
        std::num::NonZero::new(1).unwrap()
    }

    fn sample_rate(&self) -> std::num::NonZero<u32> {
        std::num::NonZero::new(8000).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn try_seek(&mut self, _pos: Duration) -> Result<(), rodio::source::SeekError> {
        Err(rodio::source::SeekError::Unsupported {
            underlying_source: "test source rejects seek".into(),
        })
    }
}

#[test]
fn failed_seek_keeps_primed_head_available() {
    let expected: Vec<f32> = (0..120).map(|sample| sample as f32).collect();
    let mut primed = super::PrimedSource::prime(
        Box::new(FailingSeekSource { samples: expected.clone(), pos: 0 }),
        80,
        0,
    );

    assert!(primed.try_seek(Duration::from_millis(500)).is_err());
    assert_eq!(primed.collect::<Vec<_>>(), expected);
}

#[test]
fn failed_seek_keeps_tail_trim_lookahead_available() {
    let expected: Vec<f32> = (1..=16).map(|sample| sample as f32).collect();
    let mut trimmed = super::TailTrimSource {
        inner: Box::new(FailingSeekSource { samples: expected.clone(), pos: 0 }),
        buf: std::collections::VecDeque::new(),
        max_samples: 4,
        ended: false,
        channels: std::num::NonZero::new(1).unwrap(),
        sample_rate: std::num::NonZero::new(8000).unwrap(),
        total_duration: Some(Duration::from_secs(1)),
    };

    assert_eq!(trimmed.next(), Some(1.0));
    let remaining: Vec<f32> = (2..=16).map(|sample| sample as f32).collect();
    assert!(trimmed.try_seek(Duration::from_millis(500)).is_err());
    assert_eq!(trimmed.collect::<Vec<_>>(), remaining);
}
