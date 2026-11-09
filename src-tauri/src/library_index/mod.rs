//! Library index (library-sync): which remote tracks/albums are already in the library, and which tracks of a
//! library album are missing. Owner: part A. See PLAN.md (v3) and DECISIONS.md D3/D4/D5/D20.
pub mod candidates;
pub mod commands;
pub mod normalize;
pub mod sources;
pub mod status;
