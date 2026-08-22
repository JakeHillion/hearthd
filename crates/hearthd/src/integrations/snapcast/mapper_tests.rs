//! Tests for the Snapcast state-to-Matter mapper.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::engine::NodeId;
    use crate::integrations::snapcast::mapper;
    use crate::integrations::snapcast::models::Client;
    use crate::integrations::snapcast::models::ClientConfig;
    use crate::integrations::snapcast::models::Group;
    use crate::integrations::snapcast::models::HostInfo;
    use crate::integrations::snapcast::models::Stream;
    use crate::integrations::snapcast::models::StreamProperties;
    use crate::integrations::snapcast::models::StreamUri;
    use crate::integrations::snapcast::models::Volume;
    use crate::matter::Cluster;
    use crate::matter::ClusterCommand;
    use crate::matter::InputType;
    use crate::matter::LevelControlCommand;
    use crate::matter::MediaInputCommand;
    use crate::matter::MediaPlaybackCommand;
    use crate::matter::OnOffCommand;
    use crate::matter::PlaybackState;

    fn stream(id: &str, scheme: &str) -> Stream {
        Stream {
            id: id.to_string(),
            status: "idle".to_string(),
            uri: StreamUri {
                scheme: scheme.to_string(),
                raw: format!("{scheme}:///dev/null?name={id}"),
            },
            properties: StreamProperties::default(),
        }
    }

    fn group(stream_id: &str) -> Group {
        Group {
            id: "g1".to_string(),
            muted: false,
            name: "Kitchen".to_string(),
            stream_id: stream_id.to_string(),
            clients: vec![],
        }
    }

    fn client(muted: bool, percent: u8, connected: bool) -> Client {
        Client {
            id: "00:11:22:33:44:55".to_string(),
            connected,
            host: HostInfo {
                arch: "x86_64".to_string(),
                ip: "127.0.0.1".to_string(),
                mac: "00:11:22:33:44:55".to_string(),
                name: "kitchen-speaker".to_string(),
                os: "Linux".to_string(),
            },
            config: ClientConfig {
                instance: 1,
                latency: 0,
                name: "Kitchen Speaker".to_string(),
                volume: Volume { muted, percent },
            },
            last_seen: Default::default(),
        }
    }

    #[test]
    fn group_node_exposes_on_off_muted_inverse() {
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let g = group("spotify");
        let node = mapper::group_node(&g, &streams, &indices, "snapcast.group.g1");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let on_off = match endpoint.clusters.get("OnOff") {
            Some(Cluster::OnOff(c)) => c,
            _ => panic!("missing OnOff cluster"),
        };
        assert!(on_off.on_off);
    }

    #[test]
    fn group_node_muted_is_off() {
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let mut g = group("spotify");
        g.muted = true;
        let node = mapper::group_node(&g, &streams, &indices, "snapcast.group.g1");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let on_off = match endpoint.clusters.get("OnOff") {
            Some(Cluster::OnOff(c)) => c,
            _ => panic!("missing OnOff cluster"),
        };
        assert!(!on_off.on_off);
    }

    #[test]
    fn media_input_lists_streams() {
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        streams.insert("airplay".to_string(), stream("airplay", "airplay"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);
        indices.insert("airplay".to_string(), 1);

        let g = group("airplay");
        let node = mapper::group_node(&g, &streams, &indices, "snapcast.group.g1");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_input = match endpoint.clusters.get("MediaInput") {
            Some(Cluster::MediaInput(c)) => c,
            _ => panic!("missing MediaInput cluster"),
        };
        assert_eq!(media_input.input_list.len(), 2);
        assert_eq!(media_input.current_input, 1);
        assert_eq!(media_input.input_list[0].input_type, InputType::Network);
        assert_eq!(media_input.input_list[1].input_type, InputType::Network);
    }

    #[test]
    fn media_playback_reflects_stream_status() {
        let mut streams = HashMap::new();
        let mut s = stream("spotify", "librespot");
        s.status = "playing".to_string();
        streams.insert("spotify".to_string(), s);
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let g = group("spotify");
        let node = mapper::group_node(&g, &streams, &indices, "snapcast.group.g1");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_playback = match endpoint.clusters.get("MediaPlayback") {
            Some(Cluster::MediaPlayback(c)) => c,
            _ => panic!("missing MediaPlayback cluster"),
        };
        assert_eq!(media_playback.current_state, PlaybackState::Playing);
    }

    #[test]
    fn client_node_exposes_volume_and_connection() {
        let c = client(false, 74, true);
        let node = mapper::client_node(&c, "snapcast.client.kitchen");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();

        let on_off = match endpoint.clusters.get("OnOff") {
            Some(Cluster::OnOff(c)) => c,
            _ => panic!("missing OnOff cluster"),
        };
        assert!(on_off.on_off);

        let level = match endpoint.clusters.get("LevelControl") {
            Some(Cluster::LevelControl(c)) => c,
            _ => panic!("missing LevelControl cluster"),
        };
        assert_eq!(level.current_level, Some(74));

        let connected = match endpoint.clusters.get("BooleanState") {
            Some(Cluster::BooleanState(c)) => c,
            _ => panic!("missing BooleanState cluster"),
        };
        assert!(connected.state_value);
    }

    #[test]
    fn group_on_command_maps_to_set_mute_false() {
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let command = ClusterCommand::OnOff(OnOffCommand::On);
        let (method, params) = mapper::command_to_rpc(
            NodeId::from_raw(1),
            &command,
            &node_to_group,
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(method, "Group.SetMute");
        let parsed: serde_json::Value = serde_json::from_str(&params.to_string()).unwrap();
        assert_eq!(parsed["id"], "g1");
        assert_eq!(parsed["mute"], false);
    }

    #[test]
    fn client_volume_command_maps_to_set_volume() {
        let mut node_to_client = HashMap::new();
        node_to_client.insert(NodeId::from_raw(2), "c1".to_string());
        let command = ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel {
            level: 42,
            transition_time: None,
        });
        let (method, params) = mapper::command_to_rpc(
            NodeId::from_raw(2),
            &command,
            &HashMap::new(),
            &node_to_client,
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(method, "Client.SetVolume");
        let parsed: serde_json::Value = serde_json::from_str(&params.to_string()).unwrap();
        assert_eq!(parsed["id"], "c1");
        assert_eq!(parsed["volume"]["percent"], 42);
        assert_eq!(parsed["volume"]["muted"], false);
    }

    #[test]
    fn media_input_select_maps_to_group_set_stream() {
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let mut index_to_stream = HashMap::new();
        index_to_stream.insert((1, "g1".to_string()), "airplay".to_string());
        let command = ClusterCommand::MediaInput(MediaInputCommand::SelectInput { index: 1 });
        let (method, params) = mapper::command_to_rpc(
            NodeId::from_raw(1),
            &command,
            &node_to_group,
            &HashMap::new(),
            &index_to_stream,
        )
        .unwrap();
        assert_eq!(method, "Group.SetStream");
        let parsed: serde_json::Value = serde_json::from_str(&params.to_string()).unwrap();
        assert_eq!(parsed["id"], "g1");
        assert_eq!(parsed["stream_id"], "airplay");
    }

    #[test]
    fn media_playback_next_maps_to_stream_control() {
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let mut index_to_stream = HashMap::new();
        index_to_stream.insert((0, "g1".to_string()), "spotify".to_string());
        let command = ClusterCommand::MediaPlayback(MediaPlaybackCommand::Next);
        let (method, params) = mapper::command_to_rpc(
            NodeId::from_raw(1),
            &command,
            &node_to_group,
            &HashMap::new(),
            &index_to_stream,
        )
        .unwrap();
        assert_eq!(method, "Stream.Control");
        let parsed: serde_json::Value = serde_json::from_str(&params.to_string()).unwrap();
        assert_eq!(parsed["id"], "spotify");
        assert_eq!(parsed["command"], "next");
    }
}
