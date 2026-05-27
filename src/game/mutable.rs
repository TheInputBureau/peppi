//! Mutable (in-progress) game data.
//!
//! You’ll only encounter mutable frame data if you’re parsing live games.

use crate::{frame::mutable::Frame, game};

pub struct Game {
	pub start: game::Start,
	pub end: Option<game::End>,
	pub frames: Frame,
	pub metadata: Option<game::Metadata>,
	pub gecko_codes: Option<game::GeckoCodes>,
	pub hash: Option<String>,
	pub quirks: Option<game::Quirks>,
}
