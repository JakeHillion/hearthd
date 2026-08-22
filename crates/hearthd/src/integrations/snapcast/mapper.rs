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

    let input_list = streams
        .values()
        .map(|s| InputInfo {
            index: stream_indices.get(&s.id).copied().unwrap_or(0),
            input_type: input_type_from_scheme(&s.uri.scheme),
            name: s.id.clone(),
            description: format!("{} stream", s.uri.scheme),
        })
        .collect::<Vec<_>>();

    let current_input = stream_indices.get(&group.stream_id).copied().unwrap_or(0);

    endpoint.clusters.insert(
        crate::matter::CLUSTER_NAME_MEDIA_INPUT.to_string(),
        Cluster::MediaInput(MediaInputCluster {
            input_list,
            current_input,
        }),
    );

    if let Some(stream) = streams.get(&group.stream_id) {
        endpoint.clusters.insert(
            crate::matter::CLUSTER_NAME_MEDIA_PLAYBACK.to_string(),
            Cluster::MediaPlayback(media_playback_from_stream(stream)),
        );
    }

    let mut endpoints = HashMap::new();
    endpoints.insert(SNAPCAST_ENDPOINT, endpoint);

    let name = if group.name.is_empty() {
        None
    } else {
        Some(group.name.clone())
    };

    Node {
        entity_id: entity_id.to_string(),
        integration: INTEGRATION_NAME.to_string(),
        name,
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
            current_level: Some(client.config.volume.percent),
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

    let name = if client.config.name.is_empty() {
        Some(client.host.name.clone())
    } else {
        Some(client.config.name.clone())
    };

    Node {
        entity_id: entity_id.to_string(),
        integration: INTEGRATION_NAME.to_string(),
        name,
        endpoints,
    }
}

/// Translate a Matter cluster command into a Snapcast JSON-RPC call.
pub fn command_to_rpc(
    node_id: NodeId,
    command: &ClusterCommand,
    group_lookup: &HashMap<NodeId, String>,
    client_lookup: &HashMap<NodeId, String>,
    stream_lookup: &HashMap<(u8, String), String>,
) -> Option<(&'static str, serde_json::Value)> {
    match command {
        ClusterCommand::OnOff(OnOffCommand::On) => {
            if let Some(id) = group_lookup.get(&node_id) {
                Some((
                    "Group.SetMute",
                    serde_json::to_value(super::models::GroupSetMuteParams {
                        id: id.clone(),
                        mute: false,
                    })
                    .ok()?,
                ))
            } else if let Some(id) = client_lookup.get(&node_id) {
                Some((
                    "Client.SetVolume",
                    serde_json::to_value(super::models::ClientSetVolumeParams {
                        id: id.clone(),
                        volume: Volume {
                            muted: false,
                            percent: 100,
                        },
                    })
                    .ok()?,
                ))
            } else {
                None
            }
        }
        ClusterCommand::OnOff(OnOffCommand::Off) => {
            if let Some(id) = group_lookup.get(&node_id) {
                Some((
                    "Group.SetMute",
                    serde_json::to_value(super::models::GroupSetMuteParams {
                        id: id.clone(),
                        mute: true,
                    })
                    .ok()?,
                ))
            } else if let Some(id) = client_lookup.get(&node_id) {
                Some((
                    "Client.SetVolume",
                    serde_json::to_value(super::models::ClientSetVolumeParams {
                        id: id.clone(),
                        volume: Volume {
                            muted: true,
                            percent: 0,
                        },
                    })
                    .ok()?,
                ))
            } else {
                None
            }
        }
        ClusterCommand::LevelControl(crate::matter::LevelControlCommand::MoveToLevel {
            level,
            ..
        }) => {
            let id = client_lookup.get(&node_id)?;
            Some((
                "Client.SetVolume",
                serde_json::to_value(super::models::ClientSetVolumeParams {
                    id: id.clone(),
                    volume: Volume {
                        muted: false,
                        percent: *level,
                    },
                })
                .ok()?,
            ))
        }
        ClusterCommand::MediaInput(crate::matter::MediaInputCommand::SelectInput { index }) => {
            let group_id = group_lookup.get(&node_id)?;
            let stream_id = stream_lookup.get(&(*index, group_id.clone()))?;
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
            let group_id = group_lookup.get(&node_id)?;
            let stream_id = stream_lookup.get(&(0, group_id.clone()))?;
            let command_name = match cmd {
                MediaPlaybackCommand::Play => "play",
                MediaPlaybackCommand::Pause => "pause",
                MediaPlaybackCommand::Stop => "stop",
                MediaPlaybackCommand::FastForward => "fastForward",
                MediaPlaybackCommand::Rewind => "rewind",
                MediaPlaybackCommand::Next => "next",
                MediaPlaybackCommand::Previous => "previous",
            };
            Some((
                "Stream.Control",
                serde_json::to_value(super::models::StreamControlParams {
                    id: stream_id.clone(),
                    command: command_name.to_string(),
                    params: None,
                })
                .ok()?,
            ))
        }
        _ => None,
    }
}

/// Build a `MediaPlaybackCluster` from a Snapcast stream.
fn media_playback_from_stream(stream: &Stream) -> MediaPlaybackCluster {
    let status = stream.properties.playback_status.as_deref();
    let status_lower = status.unwrap_or(&stream.status).to_lowercase();

    let current_state = match status_lower.as_str() {
        "playing" => PlaybackState::Playing,
        "paused" => PlaybackState::Paused,
        "buffering" => PlaybackState::Buffering,
        _ => PlaybackState::NotPlaying,
    };

    let duration = stream
        .properties
        .metadata
        .as_ref()
        .and_then(|m| m.duration)
        .map(|d| d as u64);

    let (track_title, artist) = if let Some(meta) = &stream.properties.metadata {
        let artist = meta.artist.as_ref().and_then(|a| a.first()).cloned();
        (meta.title.clone(), artist)
    } else {
        (None, None)
    };

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
fn input_type_from_scheme(scheme: &str) -> InputType {
    match scheme.to_lowercase().as_str() {
        "pipe" | "file" => InputType::Internal,
        "librespot" | "airplay" | "process" => InputType::Network,
        "alsa" | "jack" => InputType::LineIn,
        "tcp" => InputType::Network,
        "pipewire" => InputType::Usb,
        _ => InputType::Internal,
    }
}

/// Entity id helpers.
pub fn group_entity_id(group_id: &str) -> String {
    format!("snapcast.group.{group_id}")
}

pub fn client_entity_id(client_id: &str) -> String {
    format!("snapcast.client.{client_id}")
}
