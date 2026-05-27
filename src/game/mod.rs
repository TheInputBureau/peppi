//! A single game of Super Smash Brothers Melee.
//!
//! The mutable/immutable distinction is essentially an artifact of the underlying Arrow library.
//! You'll only encounter mutable data if you're parsing live games.

use std::{
	collections::BTreeMap,
	fmt::{self, Debug, Display, Formatter},
};

use base64::prelude::{BASE64_STANDARD, Engine};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::{Map, Value};

use crate::{
	frame::{PortOccupancy, transpose},
	game::shift_jis::MeleeString,
	io::slippi::{self, Version},
};

pub mod immutable;
pub mod mutable;
pub mod shift_jis;

/// How many ports the game supports.
pub const NUM_PORTS: usize = 4;

/// Some modes allow more characters than ports, e.g. Cruel Melee.
pub const MAX_PLAYERS: usize = 6;

/// Since ICs are unique mechanically, sometimes we need to treat them specially.
pub const ICE_CLIMBERS: u8 = 14;

/// A slot that can be occupied by a player.
#[repr(u8)]
#[derive(
	Clone,
	Copy,
	Debug,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
	Serialize,
	Deserialize,
	IntoPrimitive,
	TryFromPrimitive,
)]
pub enum Port {
	P1 = 0,
	P2 = 1,
	P3 = 2,
	P4 = 3,
}

impl Port {
	pub fn parse(s: &str) -> Result<Self, String> {
		match s {
			"P1" => Ok(Port::P1),
			"P2" => Ok(Port::P2),
			"P3" => Ok(Port::P3),
			"P4" => Ok(Port::P4),
			_ => Err(format!("invalid port: {}", s)),
		}
	}
}

impl Display for Port {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		use Port::*;
		match *self {
			P1 => write!(f, "P1"),
			P2 => write!(f, "P2"),
			P3 => write!(f, "P3"),
			P4 => write!(f, "P4"),
		}
	}
}

impl Default for Port {
	fn default() -> Self {
		Self::P1
	}
}

/// How a player is controlled.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TryFromPrimitive)]
pub enum PlayerType {
	Human = 0,
	Cpu = 1,
	Demo = 2,
}

/// Information about the team a player belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
	pub color: u8,
	pub shade: u8,
}

/// Dashback fix type.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TryFromPrimitive)]
pub enum DashBack {
	Ucf = 1,
	Arduino = 2,
}

/// Shield drop fix type.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TryFromPrimitive)]
pub enum ShieldDrop {
	Ucf = 1,
	Arduino = 2,
}

/// The language the game is set to.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TryFromPrimitive)]
pub enum Language {
	Japanese = 0,
	English = 1,
}

/// Information about the "Universal Controller Fix" mod.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ucf {
	pub dash_back: Option<DashBack>,
	pub shield_drop: Option<ShieldDrop>,
}

/// Netplay name, connect code, and Slippi UID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Netplay {
	pub name: MeleeString,

	pub code: MeleeString,

	/// Slippi UID (added: v3.11)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub suid: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataPlatform {
	Dolphin,
	Network,
	Nintendont,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataNames {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub netplay: Option<String>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub code: Option<String>,

	#[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
	pub extra: Map<String, Value>,
}

impl MetadataNames {
	pub fn from_raw(raw: &Map<String, Value>) -> Self {
		let mut extra = raw.clone();
		let netplay = take_string(&mut extra, "netplay");
		let code = take_string(&mut extra, "code");
		MetadataNames {
			netplay,
			code,
			extra,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataPlayer {
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub characters: BTreeMap<u8, u32>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub names: Option<MetadataNames>,

	#[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
	pub extra: Map<String, Value>,
}

impl MetadataPlayer {
	pub fn from_raw(raw: &Map<String, Value>) -> Self {
		let mut extra = raw.clone();
		let mut characters = BTreeMap::new();
		if let Some(raw_characters) = raw.get("characters").and_then(Value::as_object) {
			let mut characters_extra = raw_characters.clone();
			for (character, frame_count) in raw_characters {
				let Ok(character) = character.parse::<u8>() else {
					continue;
				};
				let Some(frame_count) = frame_count.as_u64().and_then(|value| u32::try_from(value).ok()) else {
					continue;
				};
				characters.insert(character, frame_count);
				characters_extra.remove(character.to_string().as_str());
			}

			if characters_extra.is_empty() {
				extra.remove("characters");
			} else {
				extra.insert("characters".to_string(), Value::Object(characters_extra));
			}
		}

		let names = raw
			.get("names")
			.and_then(Value::as_object)
			.map(MetadataNames::from_raw);
		if names.is_some() {
			extra.remove("names");
		}

		MetadataPlayer {
			characters,
			names,
			extra,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub start_at: Option<String>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub played_on: Option<MetadataPlatform>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub last_frame: Option<i32>,

	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub players: BTreeMap<u8, MetadataPlayer>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub console_nick: Option<String>,

	#[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
	pub extra: Map<String, Value>,
}

impl Metadata {
	pub fn from_raw(raw: &Map<String, Value>) -> Self {
		let mut extra = raw.clone();
		let start_at = take_string(&mut extra, "startAt");
		let played_on = take_platform(&mut extra, "playedOn");
		let last_frame = take_i32(&mut extra, "lastFrame");
		let console_nick = take_string(&mut extra, "consoleNick");

		let mut players = BTreeMap::new();
		if let Some(raw_players) = raw.get("players").and_then(Value::as_object) {
			let mut players_extra = raw_players.clone();
			for (player_index, player_value) in raw_players {
				let Ok(player_index) = player_index.parse::<u8>() else {
					continue;
				};
				let Some(player_obj) = player_value.as_object() else {
					continue;
				};
				players.insert(player_index, MetadataPlayer::from_raw(player_obj));
				players_extra.remove(player_index.to_string().as_str());
			}

			if players_extra.is_empty() {
				extra.remove("players");
			} else {
				extra.insert("players".to_string(), Value::Object(players_extra));
			}
		}

		Metadata {
			start_at,
			played_on,
			last_frame,
			players,
			console_nick,
			extra,
		}
	}

	pub fn to_raw(&self) -> Map<String, Value> {
		serde_json::to_value(self)
			.ok()
			.and_then(|value| value.as_object().cloned())
			.unwrap_or_default()
	}

	pub fn is_empty(&self) -> bool {
		self.start_at.is_none()
			&& self.played_on.is_none()
			&& self.last_frame.is_none()
			&& self.players.is_empty()
			&& self.console_nick.is_none()
			&& self.extra.is_empty()
	}
}

fn take_string(extra: &mut Map<String, Value>, key: &str) -> Option<String> {
	extra
		.get(key)
		.and_then(Value::as_str)
		.map(String::from)
		.inspect(|_| {
			extra.remove(key);
		})
}

fn take_i32(extra: &mut Map<String, Value>, key: &str) -> Option<i32> {
	extra
		.get(key)
		.and_then(Value::as_i64)
		.and_then(|value| i32::try_from(value).ok())
		.inspect(|_| {
			extra.remove(key);
		})
}

fn take_platform(extra: &mut Map<String, Value>, key: &str) -> Option<MetadataPlatform> {
	extra
		.get(key)
		.and_then(Value::as_str)
		.and_then(|value| match value {
			"dolphin" => Some(MetadataPlatform::Dolphin),
			"network" => Some(MetadataPlatform::Network),
			"nintendont" => Some(MetadataPlatform::Nintendont),
			_ => None,
		})
		.inspect(|_| {
			extra.remove(key);
		})
}

/// Information about each player such as character, team, stock count, etc.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
	pub port: Port,

	pub character: u8,

	pub r#type: PlayerType,

	/// starting stock count
	pub stocks: u8,

	pub costume: u8,

	pub team: Option<Team>,

	/// handicap level; affects `offense_ratio` & `defense_ratio`
	pub handicap: u8,

	/// miscellaneous flags (metal, stamina mode, etc)
	pub bitfield: u8,

	/// CPU level
	pub cpu_level: Option<u8>,

	/// percent the player's first stock will start at
	pub damage_start: u16,

	/// percent the player's stocks will start at (including the first, if `damage_start` is zero)
	pub damage_spawn: u16,

	/// knockback multiplier when this player hits another
	pub offense_ratio: f32,

	/// knockback multiplier when this player is hit
	pub defense_ratio: f32,

	pub model_scale: f32,

	/// UCF info (added: v1.0)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub ucf: Option<Ucf>,

	/// in-game name-tag (added: v1.3)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name_tag: Option<MeleeString>,

	/// netplay info (added: v3.9)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub netplay: Option<Netplay>,

	/// Slippi user id (same source bytes as SUID when present)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub user_id: Option<String>,
}

/// Major & minor scene numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
	pub minor: u8,
	pub major: u8,
}

/// Container for raw bytes of `Start` & `End` events.
#[derive(PartialEq, Eq, Clone, Default)]
pub struct Bytes(pub Vec<u8>);

impl Serialize for Bytes {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&BASE64_STANDARD.encode(&self.0))
	}
}

struct BytesVisitor;

impl<'de> de::Visitor<'de> for BytesVisitor {
	type Value = Bytes;

	fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str("a base64-encoded string")
	}

	fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
		Ok(Bytes(
			BASE64_STANDARD
				.decode(value)
				.map_err(|_| E::custom("invalid base64"))?,
		))
	}
}

impl<'de> Deserialize<'de> for Bytes {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_string(BytesVisitor)
	}
}

impl Debug for Bytes {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
		write!(f, "Bytes {{ len: {} }}", self.0.len())
	}
}

/// Information about the match a game belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
	pub id: String,
	pub game: u32,
	pub tiebreaker: u32,
}

/// Information used to initialize the game such as the game mode, settings, characters & stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Start {
	pub slippi: slippi::Slippi,

	pub bitfield: [u8; 4],

	pub is_raining_bombs: bool,

	pub is_teams: bool,

	pub item_spawn_frequency: i8,

	pub self_destruct_score: i8,

	pub stage: u16,

	pub timer: u32,

	pub item_spawn_bitfield: [u8; 5],

	pub damage_ratio: f32,

	pub players: Vec<Player>,

	pub random_seed: u32,

	/// Partly-redundant copy of the raw start block, for round-tripping
	pub bytes: Bytes,

	/// (added: v1.5)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub is_pal: Option<bool>,

	/// (added: v2.0)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub is_frozen_ps: Option<bool>,

	/// (added: v3.7)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub scene: Option<Scene>,

	/// (added: v3.12)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub language: Option<Language>,

	/// (added: v3.14)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub r#match: Option<Match>,

	/// Convenience alias for the modern Slippi session / match id.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub session_id: Option<String>,
}

/// How the game ended.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TryFromPrimitive)]
pub enum EndMethod {
	Unresolved = 0,
	Time = 1,
	Game = 2,
	Resolved = 3,
	NoContest = 7,
}

/// Player placements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerEnd {
	pub port: Port,
	pub placement: u8,
}

/// Information about the end of the game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
	/// how the game ended
	pub method: EndMethod,

	/// Partly-redundant copy of the raw end block, for round-tripping
	pub bytes: Bytes,

	/// player who LRAS'd, if any (added: v2.0)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub lras_initiator: Option<Option<Port>>,

	/// player-specific data (added: v3.13)
	#[serde(skip_serializing_if = "Option::is_none")]
	pub players: Option<Vec<PlayerEnd>>,
}

impl End {
	pub(crate) fn size(version: Version) -> usize {
		if version.gte(3, 13) {
			6
		} else if version.gte(2, 0) {
			2
		} else {
			1
		}
	}
}

/// Binary blob of Gecko codes in use.
///
/// Currently unparsed, but still needed for round-tripping.
#[derive(Debug, PartialEq, Eq)]
pub struct GeckoCodes {
	pub bytes: Vec<u8>,
	pub actual_size: u32,
}

pub trait Game {
	fn start(&self) -> &Start;
	fn end(&self) -> &Option<End>;
	fn metadata(&self) -> &Option<Metadata>;
	fn gecko_codes(&self) -> &Option<GeckoCodes>;

	/// Duration of the game in frames.
	fn len(&self) -> usize;

	/// Combines all data for a single frame into a struct.
	/// Avoid calling this if you need maximum performance.
	fn frame(&self, idx: usize) -> transpose::Frame;
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
/// Slippi quirks that we need to track for round-trip integrity.
pub struct Quirks {
	pub double_game_end: bool,
}

pub fn port_occupancy(start: &Start) -> Vec<PortOccupancy> {
	start
		.players
		.iter()
		.map(|p| PortOccupancy {
			port: p.port,
			follower: p.character == ICE_CLIMBERS,
		})
		.collect()
}
