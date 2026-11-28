pub mod commands;
pub mod fallback;
pub mod types;

pub use fallback::synthesize_canonical_from_preview;
pub use crate::platforms::musicbrainz::MusicBrainzClient;
pub use types::{
    AudioCandidate, AudioQualityTier, AudioSource, CanonicalDisc, CanonicalRelease,
    CanonicalReleaseSummary, CanonicalTrack, ReleaseEditionOption,
};
