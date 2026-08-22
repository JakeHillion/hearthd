//! Map between Snapcast JSON-RPC state and hearthd's Matter data model.

use std::collections::HashMap;

use crate::engine::NodeId;
use crate::integrations::snapcast::models::Client;
use crate::integrations::snapcast::models::Group;
use crate::integrations::snapcast::models::Stream;
use crate::integrations::snapcast::models::Volume;
use crate::matter::BooleanStateCluster;
use crate::matter::Cluster;
use crate::matter::ClusterCommand;
use crate::matter::Endpoint;
use crate::matter::EndpointId;
use crate::matter::InputInfo;
use crate::matter::InputType;
use crate::matter::LevelControlCluster;
use crate::matter::MediaInputCluster;
use crate::matter::MediaPlaybackCluster;
use crate::matter::MediaPlaybackCommand;
use crate::matter::Node;
use crate::matter::OnOffCluster;
use crate::matter::OnOffCommand;
use crate::matter::PlaybackState;

/// Endpoint used for all Snapcast-derived nodes.
pub const SNAPCAST_ENDPOINT: EndpointId = 1;

/// Integration name reported to the engine.
const INTEGRATION_NAME: &str = "snapcast";

/// Matter's `CurrentLevel` maximum, as used by the other integrations.
const MATTER_LEVEL_MAX: u32 = 254;

/// Snapcast volume percent to Matter's 0-254 `CurrentLevel`.
pub fn percent_to_level(percent: u8) -> u8 {
    let percent = u32::from(percent).min(100);
    ((percent * MATTER_LEVEL_MAX + 50) / 100) as u8
}

/// Matter's 0-254 `CurrentLevel` back to Snapcast volume percent.
pub fn level_to_percent(level: u8) -> u8 {
    ((u32::from(level) * 100 + MATTER_LEVEL_MAX / 2) / MATTER_LEVEL_MAX).min(100) as u8
}

/// State the command translation needs to read before it can build a call.
///
/// Snapcast has no partial updates for volume: `Client.SetVolume` carries both
/// the mute flag and the percent, so muting without knowing the current
/// percent would silently reset it.
pub struct CommandContext<'a> {
    pub node_to_group: &'a HashMap<NodeId, String>,
    pub node_to_client: &'a HashMap<NodeId, String>,
    pub groups: &'a HashMap<String, Group>,
    pub clients: &'a HashMap<String, Client>,
    pub stream_by_index: &'a HashMap<u8, String>,
}

/// Build a Matter node for a Snapcast group.
pub fn group_node(
    group: &Group,
    streams: &HashMap<String, Stream>,
    stream_indices: &HashMap<String, u8>,
    entity_id: &str,
) -> Node {
    let mut endpoint = Endpoint::default();
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
        Cluster::OnOff(OnOffCluster {
            on_off: !group.muted,
        }),
    );

    // Sorted by index so the exposed list has a stable order rather than the
    // hash map's.
    let mut input_list = streams
        .values()
        .map(|s| InputInfo {
            index: stream_indices.get(&s.id).copied().unwrap_or(0),
            input_type: input_type_from_scheme(&s.uri.scheme),
            name: s.id.clone(),
            description: format!("{} stream", s.uri.scheme),
        })
        .collect::<Vec<_>>();
    input_list.sort_by_key(|i| i.index);

    let current_input = stream_indices.get(&group.stream_id).copied().unwrap_or(0);

    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_MEDIA_INPUT.to_string(),
        Cluster::MediaInput(MediaInputCluster {
            input_list,
            current_input,
        }),
    );

    // Always present, so a group whose stream is unknown still exposes
    // playback rather than dropping the cluster and changing its shape.
    let playback = streams
        .get(&group.stream_id)
        .map(media_playback_from_stream)
        .unwrap_or_default();
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_MEDIA_PLAYBACK.to_string(),
        Cluster::MediaPlayback(playback),
    );

    let mut endpoints = HashMap::new();
    endpoints.insert(SNAPCAST_ENDPOINT, endpoint);

    Node {
        entity_id: entity_id.to_string(),
        integration: INTEGRATION_NAME.to_string(),
        name: Some(group_display_name(group)),
        endpoints,
    }
}

/// Build a Matter node for a Snapcast client.
pub fn client_node(client: &Client, entity_id: &str) -> Node {
    let mut endpoint = Endpoint::default();
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_ON_OFF.to_string(),
        Cluster::OnOff(OnOffCluster {
            on_off: !client.config.volume.muted,
        }),
    );
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_LEVEL_CONTROL.to_string(),
        Cluster::LevelControl(LevelControlCluster {
            current_level: Some(percent_to_level(client.config.volume.percent)),
        }),
    );
    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_BOOLEAN_STATE.to_string(),
        Cluster::BooleanState(BooleanStateCluster {
            state_value: client.connected,
        }),
    );

    let mut endpoints = HashMap::new();
    endpoints.insert(SNAPCAST_ENDPOINT, endpoint);

    Node {
        entity_id: entity_id.to_string(),
        integration: INTEGRATION_NAME.to_string(),
        name: Some(client_display_name(client)),
        endpoints,
    }
}

/// Translate a Matter cluster command into a Snapcast JSON-RPC call.
pub fn command_to_rpc(
    node_id: NodeId,
    command: &ClusterCommand,
    ctx: &CommandContext<'_>,
) -> Option<(&'static str, serde_json::Value)> {
    match command {
        ClusterCommand::OnOff(OnOffCommand::On) => set_muted(node_id, ctx, false),
        ClusterCommand::OnOff(OnOffCommand::Off) => set_muted(node_id, ctx, true),

        ClusterCommand::LevelControl(crate::matter::LevelControlCommand::MoveToLevel {
            level,
            ..
        }) => {
            let id = ctx.node_to_client.get(&node_id)?;
            // Volume and mute are separate axes in Matter, so a level change
            // leaves the mute flag as it found it.
            let muted = ctx
                .clients
                .get(id)
                .map(|c| c.config.volume.muted)
                .unwrap_or(false);
            Some((
                "Client.SetVolume",
                serde_json::to_value(super::models::ClientSetVolumeParams {
                    id: id.clone(),
                    volume: Volume {
                        muted,
                        percent: level_to_percent(*level),
                    },
                })
                .ok()?,
            ))
        }

        ClusterCommand::MediaInput(crate::matter::MediaInputCommand::SelectInput { index }) => {
            let group_id = ctx.node_to_group.get(&node_id)?;
            let stream_id = ctx.stream_by_index.get(index)?;
            Some((
                "Group.SetStream",
                serde_json::to_value(super::models::GroupSetStreamParams {
                    id: group_id.clone(),
                    stream_id: stream_id.clone(),
                })
                .ok()?,
            ))
        }

        ClusterCommand::MediaPlayback(cmd) => {
            let group_id = ctx.node_to_group.get(&node_id)?;
            // Control the stream this group is actually playing. Addressing a
            // fixed index would drive whichever stream happened to be first.
            let stream_id = ctx.groups.get(group_id).map(|g| g.stream_id.clone())?;
            let command_name = match cmd {
                MediaPlaybackCommand::Play => "play",
                MediaPlaybackCommand::Pause => "pause",
                MediaPlaybackCommand::Stop => "stop",
                MediaPlaybackCommand::Next => "next",
                MediaPlaybackCommand::Previous => "previous",
                // Snapserver's Stream.Control vocabulary has no scanning
                // commands and answers these with "Command not supported", so
                // report no mapping instead of a call that always fails.
                MediaPlaybackCommand::FastForward | MediaPlaybackCommand::Rewind => return None,
            };
            Some((
                "Stream.Control",
                serde_json::to_value(super::models::StreamControlParams {
                    id: stream_id,
                    command: command_name.to_string(),
                    params: None,
                })
                .ok()?,
            ))
        }

        _ => None,
    }
}

/// Mute or unmute whichever of a group or client the node denotes.
fn set_muted(
    node_id: NodeId,
    ctx: &CommandContext<'_>,
    muted: bool,
) -> Option<(&'static str, serde_json::Value)> {
    if let Some(id) = ctx.node_to_group.get(&node_id) {
        return Some((
            "Group.SetMute",
            serde_json::to_value(super::models::GroupSetMuteParams {
                id: id.clone(),
                mute: muted,
            })
            .ok()?,
        ));
    }

    let id = ctx.node_to_client.get(&node_id)?;
    // Carry the current percent through: Snapcast would otherwise take the
    // volume in this call literally and unmuting would come back at whatever
    // level we invented.
    let percent = ctx
        .clients
        .get(id)
        .map(|c| c.config.volume.percent)
        .unwrap_or(100);
    Some((
        "Client.SetVolume",
        serde_json::to_value(super::models::ClientSetVolumeParams {
            id: id.clone(),
            volume: Volume { muted, percent },
        })
        .ok()?,
    ))
}

/// Build a `MediaPlaybackCluster` from a Snapcast stream.
fn media_playback_from_stream(stream: &Stream) -> MediaPlaybackCluster {
    // `playbackStatus` is the MPRIS-style property a controllable stream
    // publishes; `status` is the server's own idle/playing view, which is all
    // a plain pipe source has.
    let status = stream
        .properties
        .playback_status
        .as_deref()
        .unwrap_or(&stream.status)
        .to_lowercase();

    let current_state = match status.as_str() {
        "playing" => PlaybackState::Playing,
        "paused" => PlaybackState::Paused,
        "buffering" => PlaybackState::Buffering,
        _ => PlaybackState::NotPlaying,
    };

    let metadata = stream.properties.metadata.as_ref();
    // Snapcast reports seconds and Matter's Duration is milliseconds. The
    // filter drops NaN and negative values, which would otherwise saturate to
    // a nonsense duration rather than to "unknown".
    let duration = metadata
        .and_then(|m| m.duration)
        .filter(|d| d.is_finite() && *d >= 0.0)
        .map(|d| (d * 1000.0) as u64);
    let track_title = metadata.and_then(|m| m.title.clone());
    let artist = metadata.and_then(|m| m.artist.as_ref()?.first().cloned());

    MediaPlaybackCluster {
        current_state,
        start_time: None,
        duration,
        playback_speed: None,
        track_title,
        artist,
    }
}

/// Map a Snapcast URI scheme to a Matter `InputType`.
///
/// `InputTypeEnum` has no network value, so the streaming sources map to
/// `Other` rather than to an identifier outside the range a Matter controller
/// will accept.
fn input_type_from_scheme(scheme: &str) -> InputType {
    match scheme.to_lowercase().as_str() {
        "pipe" | "file" | "meta" => InputType::Internal,
        "alsa" | "jack" => InputType::Line,
        "pipewire" => InputType::Usb,
        _ => InputType::Other,
    }
}

/// Human-readable name for a group, falling back to its id.
fn group_display_name(group: &Group) -> String {
    if group.name.is_empty() {
        format!("Snapcast group {}", short_id(&group.id))
    } else {
        group.name.clone()
    }
}

/// Human-readable name for a client, falling back through host name to id.
fn client_display_name(client: &Client) -> String {
    if !client.config.name.is_empty() {
        client.config.name.clone()
    } else if !client.host.name.is_empty() {
        client.host.name.clone()
    } else {
        format!("Snapcast client {}", short_id(&client.id))
    }
}

/// Entity id for a group.
///
/// Derived from the name configured in Snapcast where there is one and from
/// the group id otherwise — never from the stream, which changes as the group
/// is retargeted and would take the entity id with it.
pub fn group_entity_id(group: &Group) -> String {
    format!("media_player.{}", slug_or_id(&group.name, &group.id))
}

/// Entity id for a client.
///
/// Only the name set in Snapcast is used, never the host name: entity ids have
/// to be unique, and a host name is neither chosen for that purpose nor
/// distinct — two machines both reporting `localhost` is ordinary, and the
/// engine resolves one name to one node, so the second would take the first's
/// place and removing either would strand the survivor.
pub fn client_entity_id(client: &Client) -> String {
    format!("speaker.{}", slug_or_id(&client.config.name, &client.id))
}

/// Slug of `name`, falling back to the full id.
///
/// The whole id is used rather than the short form the display names take,
/// because Snapcast only guarantees the whole of it to be distinct: one device
/// running two instances yields ids that differ solely in a trailing `#1`.
///
/// The fallback also catches a name that slugs to nothing, such as one made
/// entirely of punctuation, which would otherwise leave a bare domain prefix
/// that every such device would share.
fn slug_or_id(name: &str, id: &str) -> String {
    let slugged = slug(name);
    if slugged.is_empty() {
        slug(&format!("snapcast {id}"))
    } else {
        slugged
    }
}

/// First id segment, enough to tell devices apart without the full UUID.
fn short_id(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

/// Lowercase, underscore-separated form suitable for an entity id.
fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}
