//! Tests for the Snapcast state-to-Matter mapper.

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::engine::NodeId;
    use crate::integrations::snapcast::mapper;
    use crate::integrations::snapcast::models::Client;
    use crate::integrations::snapcast::models::ClientConfig;
    use crate::integrations::snapcast::models::GetStatusResult;
    use crate::integrations::snapcast::models::Group;
    use crate::integrations::snapcast::models::HostInfo;
    use crate::integrations::snapcast::models::RpcResponse;
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

    /// A verbatim `Server.GetStatus` response from a real Snapserver 0.35.0.
    ///
    /// Captured rather than hand-written: the wire format differs from the
    /// obvious guess in two ways that each silently produce an empty or
    /// default model instead of an error, so a fabricated fixture would agree
    /// with a broken parser. The result nests everything under a second
    /// `server` key, and the stream properties are camelCase.
    const REAL_GET_STATUS: &str = r##"
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": {
    "server": {
      "groups": [
        {
          "clients": [
            {
              "config": {
                "instance": 1,
                "latency": 0,
                "name": "",
                "volume": {
                  "muted": false,
                  "percent": 100
                }
              },
              "connected": false,
              "host": {
                "arch": "web",
                "ip": "127.0.0.1",
                "mac": "00:00:00:00:00:00",
                "name": "Snapweb client",
                "os": "MacIntel"
              },
              "id": "9cab1381-3694-428d-a804-c1b1ac7a46aa",
              "lastSeen": {
                "sec": 1787007392,
                "usec": 916565
              },
              "snapclient": {
                "name": "snapweb",
                "protocolVersion": 2,
                "version": "0.9.2"
              }
            }
          ],
          "id": "c29b1bd6-edff-8e0c-4d4b-dea5cd72a6aa",
          "muted": false,
          "name": "",
          "stream_id": "Spotify"
        },
        {
          "clients": [
            {
              "config": {
                "instance": 1,
                "latency": 0,
                "name": "",
                "volume": {
                  "muted": false,
                  "percent": 14
                }
              },
              "connected": true,
              "host": {
                "arch": "arm64-v8a",
                "ip": "::ffff:10.239.19.16",
                "mac": "00:00:00:00:00:00",
                "name": "Portal",
                "os": "Android 10"
              },
              "id": "4d6bc7eaa1cfcd9b",
              "lastSeen": {
                "sec": 1787443604,
                "usec": 397929
              },
              "snapclient": {
                "name": "Snapclient",
                "protocolVersion": 2,
                "version": "0.35.0"
              }
            }
          ],
          "id": "b8992e86-c3bc-9140-da4a-f4b613730bc7",
          "muted": false,
          "name": "",
          "stream_id": "Meta"
        },
        {
          "clients": [
            {
              "config": {
                "instance": 0,
                "latency": 0,
                "name": "",
                "volume": {
                  "muted": false,
                  "percent": 100
                }
              },
              "connected": false,
              "host": {
                "arch": "arm64",
                "ip": "::ffff:172.20.0.13",
                "mac": "00:00:00:00:00:00",
                "name": "localhost",
                "os": "iOS 26.6"
              },
              "id": "B1659FDD-E1E7-4377-AED8-DA19C4601B81#0",
              "lastSeen": {
                "sec": 1787439414,
                "usec": 342925
              },
              "snapclient": {
                "name": "Snap.Net Stream",
                "protocolVersion": 2,
                "version": "0.24.0"
              }
            }
          ],
          "id": "d8fbaaa3-e5ff-7a43-32d2-3e374e319ede",
          "muted": false,
          "name": "",
          "stream_id": "Spotify"
        }
      ],
      "server": {
        "host": {
          "arch": "x86_64",
          "ip": "",
          "mac": "",
          "name": "stinger",
          "os": "NixOS 26.05 (Yarara)"
        },
        "snapserver": {
          "controlProtocolVersion": 1,
          "name": "Snapserver",
          "protocolVersion": 1,
          "version": "0.35.0"
        }
      },
      "streams": [
        {
          "id": "Spotify",
          "properties": {
            "canControl": false,
            "canGoNext": false,
            "canGoPrevious": false,
            "canPause": false,
            "canPlay": false,
            "canSeek": false
          },
          "status": "idle",
          "uri": {
            "fragment": "",
            "host": "",
            "path": "//nix/store/z1hbb2ylsc6zmvziab88krwg6i7v3fp1-librespot-0.8.0/bin/librespot",
            "query": {
              "bitrate": "320",
              "chunk_ms": "20",
              "codec": "flac",
              "devicename": "stinger",
              "name": "Spotify",
              "params": "--zeroconf-backend avahi --zeroconf-port 5354",
              "sampleformat": "44100:16:2"
            },
            "raw": "librespot:////nix/store/z1hbb2ylsc6zmvziab88krwg6i7v3fp1-librespot-0.8.0/bin/librespot?bitrate=320&chunk_ms=20&codec=flac&devicename=stinger&name=Spotify&params=--zeroconf-backend%20avahi%20--zeroconf-port%205354&sampleformat=44100%3A16%3A2",
            "scheme": "librespot"
          }
        },
        {
          "id": "AirPlay",
          "properties": {
            "canControl": false,
            "canGoNext": false,
            "canGoPrevious": false,
            "canPause": false,
            "canPlay": false,
            "canSeek": false
          },
          "status": "idle",
          "uri": {
            "fragment": "",
            "host": "",
            "path": "/run/snapcast/airplay",
            "query": {
              "chunk_ms": "20",
              "codec": "flac",
              "mode": "read",
              "name": "AirPlay",
              "sampleformat": "44100:16:2"
            },
            "raw": "pipe:///run/snapcast/airplay?chunk_ms=20&codec=flac&mode=read&name=AirPlay&sampleformat=44100%3A16%3A2",
            "scheme": "pipe"
          }
        },
        {
          "id": "Meta",
          "properties": {
            "canControl": false,
            "canGoNext": false,
            "canGoPrevious": false,
            "canPause": false,
            "canPlay": false,
            "canSeek": false
          },
          "status": "idle",
          "uri": {
            "fragment": "",
            "host": "",
            "path": "/Spotify/AirPlay",
            "query": {
              "chunk_ms": "20",
              "codec": "flac",
              "name": "Meta",
              "sampleformat": "44100:16:2"
            },
            "raw": "meta:///Spotify/AirPlay?chunk_ms=20&codec=flac&name=Meta&sampleformat=44100%3A16%3A2",
            "scheme": "meta"
          }
        }
      ]
    }
  }
}
"##;

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

    /// Command lookups over a single group and a single client.
    fn context<'a>(
        groups: &'a HashMap<String, Group>,
        clients: &'a HashMap<String, Client>,
        node_to_group: &'a HashMap<NodeId, String>,
        node_to_client: &'a HashMap<NodeId, String>,
        stream_by_index: &'a HashMap<u8, String>,
    ) -> mapper::CommandContext<'a> {
        mapper::CommandContext {
            node_to_group,
            node_to_client,
            groups,
            clients,
            stream_by_index,
        }
    }

    #[test]
    fn real_get_status_response_parses() {
        let response: RpcResponse<GetStatusResult> =
            serde_json::from_str(REAL_GET_STATUS).expect("captured response should parse");
        let status = response.result.expect("response carries a result").server;

        // Straight into ServerStatus this is empty: the groups live one level
        // down, under the result's `server` key.
        assert_eq!(status.groups.len(), 3);
        assert_eq!(status.streams.len(), 3);

        let ids: Vec<&str> = status.streams.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["Spotify", "AirPlay", "Meta"]);
        assert_eq!(status.streams[0].uri.scheme, "librespot");
        assert_eq!(status.streams[0].status, "idle");
    }

    #[test]
    fn real_client_state_parses() {
        let response: RpcResponse<GetStatusResult> = serde_json::from_str(REAL_GET_STATUS).unwrap();
        let status = response.result.unwrap().server;

        let client = status
            .groups
            .iter()
            .flat_map(|g| &g.clients)
            .find(|c| c.host.name == "Snapweb client")
            .expect("the snapweb client is in the capture");

        assert_eq!(client.config.volume.percent, 100);
        assert!(!client.config.volume.muted);
        assert_eq!(client.config.instance, 1);
        // Spelled `lastSeen` on the wire.
        assert!(client.last_seen.sec > 0);
    }

    #[test]
    fn stream_properties_are_read_as_camel_case() {
        let response: RpcResponse<GetStatusResult> = serde_json::from_str(REAL_GET_STATUS).unwrap();
        let status = response.result.unwrap().server;

        // Snapserver sends `canPlay`; read as snake_case every one of these
        // stays None and the stream looks like it published no properties at
        // all rather than like one that cannot be controlled.
        let properties = &status.streams[0].properties;
        assert_eq!(properties.can_play, Some(false));
        assert_eq!(properties.can_pause, Some(false));
        assert_eq!(properties.can_control, Some(false));
        assert_eq!(properties.can_go_next, Some(false));
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // Snapserver omits what does not apply, and one absent field must not
        // cost the whole status.
        let sparse = r#"{"server":{"groups":[{"id":"g1","clients":[{"id":"c1"}]}]}}"#;
        let parsed: GetStatusResult = serde_json::from_str(sparse).expect("sparse status parses");
        assert_eq!(parsed.server.groups.len(), 1);
        assert_eq!(parsed.server.groups[0].clients[0].id, "c1");
        assert!(!parsed.server.groups[0].clients[0].connected);
        assert!(parsed.server.streams.is_empty());
    }

    #[test]
    fn group_node_exposes_on_off_muted_inverse() {
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let g = group("spotify");
        let node = mapper::group_node(&g, &streams, &indices, "media_player.kitchen");
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
        let node = mapper::group_node(&g, &streams, &indices, "media_player.kitchen");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let on_off = match endpoint.clusters.get("OnOff") {
            Some(Cluster::OnOff(c)) => c,
            _ => panic!("missing OnOff cluster"),
        };
        assert!(!on_off.on_off);
    }

    #[test]
    fn media_input_lists_streams_in_index_order() {
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        streams.insert("airplay".to_string(), stream("airplay", "pipe"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);
        indices.insert("airplay".to_string(), 1);

        let g = group("airplay");
        let node = mapper::group_node(&g, &streams, &indices, "media_player.kitchen");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_input = match endpoint.clusters.get("MediaInput") {
            Some(Cluster::MediaInput(c)) => c,
            _ => panic!("missing MediaInput cluster"),
        };
        assert_eq!(media_input.input_list.len(), 2);
        assert_eq!(media_input.current_input, Some(1));
        // Ordered by index rather than by the streams map's iteration order.
        assert_eq!(media_input.input_list[0].name, "spotify");
        assert_eq!(media_input.input_list[0].input_type, InputType::Other);
        assert_eq!(media_input.input_list[1].name, "airplay");
        assert_eq!(media_input.input_list[1].input_type, InputType::Internal);
    }

    #[test]
    fn a_group_on_a_stream_the_server_did_not_describe_selects_nothing() {
        // Index 0 names a real stream, so defaulting to it would report the
        // first stream as the one this group is playing.
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let node = mapper::group_node(&group("gone"), &streams, &indices, "media_player.kitchen");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_input = match endpoint.clusters.get("MediaInput") {
            Some(Cluster::MediaInput(c)) => c,
            _ => panic!("missing MediaInput cluster"),
        };
        assert_eq!(media_input.current_input, None);
        assert_eq!(media_input.input_list.len(), 1);
    }

    #[test]
    fn a_stream_with_no_index_is_left_out_of_the_input_list() {
        // Streams past index 255 carry no index, and listing them all at 0
        // would shadow the stream that really holds it.
        let mut streams = HashMap::new();
        streams.insert("spotify".to_string(), stream("spotify", "librespot"));
        streams.insert("unindexed".to_string(), stream("unindexed", "pipe"));
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let node = mapper::group_node(
            &group("spotify"),
            &streams,
            &indices,
            "media_player.kitchen",
        );
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_input = match endpoint.clusters.get("MediaInput") {
            Some(Cluster::MediaInput(c)) => c,
            _ => panic!("missing MediaInput cluster"),
        };
        assert_eq!(media_input.input_list.len(), 1);
        assert_eq!(media_input.input_list[0].name, "spotify");
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
        let node = mapper::group_node(&g, &streams, &indices, "media_player.kitchen");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_playback = match endpoint.clusters.get("MediaPlayback") {
            Some(Cluster::MediaPlayback(c)) => c,
            _ => panic!("missing MediaPlayback cluster"),
        };
        assert_eq!(media_playback.current_state, PlaybackState::Playing);
    }

    #[test]
    fn media_playback_prefers_playback_status_over_server_status() {
        let mut streams = HashMap::new();
        let mut s = stream("spotify", "librespot");
        s.status = "playing".to_string();
        s.properties.playback_status = Some("paused".to_string());
        streams.insert("spotify".to_string(), s);
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let node = mapper::group_node(
            &group("spotify"),
            &streams,
            &indices,
            "media_player.kitchen",
        );
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let media_playback = match endpoint.clusters.get("MediaPlayback") {
            Some(Cluster::MediaPlayback(c)) => c,
            _ => panic!("missing MediaPlayback cluster"),
        };
        assert_eq!(media_playback.current_state, PlaybackState::Paused);
    }

    #[test]
    fn group_with_unknown_stream_still_exposes_playback() {
        // The cluster set a node exposes must not depend on whether the
        // server happened to describe the stream it points at.
        let node = mapper::group_node(
            &group("gone"),
            &HashMap::new(),
            &HashMap::new(),
            "media_player.kitchen",
        );
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        assert!(endpoint.clusters.contains_key("MediaPlayback"));
    }

    #[test]
    fn client_node_exposes_volume_and_connection() {
        let c = client(false, 74, true);
        let node = mapper::client_node(&c, "speaker.kitchen_speaker");
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
        // Snapcast percent scaled onto Matter's 0-254 CurrentLevel.
        assert_eq!(level.current_level, Some(mapper::percent_to_level(74)));

        let connected = match endpoint.clusters.get("BooleanState") {
            Some(Cluster::BooleanState(c)) => c,
            _ => panic!("missing BooleanState cluster"),
        };
        assert!(connected.state_value);
    }

    #[test]
    fn volume_percent_and_matter_level_round_trip() {
        assert_eq!(mapper::percent_to_level(0), 0);
        assert_eq!(mapper::percent_to_level(100), 254);
        for percent in 0..=100u8 {
            let level = mapper::percent_to_level(percent);
            assert_eq!(mapper::level_to_percent(level), percent);
        }
    }

    #[test]
    fn group_on_command_maps_to_set_mute_false() {
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let command = ClusterCommand::OnOff(OnOffCommand::On);
        let (groups, clients, node_to_client, by_index) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        let (method, params) = mapper::command_to_rpc(NodeId::from_raw(1), &command, &ctx).unwrap();
        assert_eq!(method, "Group.SetMute");
        assert_eq!(params["id"], "g1");
        assert_eq!(params["mute"], false);
    }

    #[test]
    fn client_mute_preserves_the_current_volume() {
        // Client.SetVolume carries both fields, so muting has to send the
        // percent the client already had. Inventing one resets the volume,
        // and unmuting then comes back at the invented level.
        let mut node_to_client = HashMap::new();
        node_to_client.insert(NodeId::from_raw(2), "c1".to_string());
        let mut clients = HashMap::new();
        clients.insert("c1".to_string(), client(false, 37, true));
        let (groups, node_to_group, by_index) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        let command = ClusterCommand::OnOff(OnOffCommand::Off);
        let (method, params) = mapper::command_to_rpc(NodeId::from_raw(2), &command, &ctx).unwrap();
        assert_eq!(method, "Client.SetVolume");
        assert_eq!(params["volume"]["muted"], true);
        assert_eq!(params["volume"]["percent"], 37);
    }

    #[test]
    fn client_volume_command_maps_to_set_volume() {
        let mut node_to_client = HashMap::new();
        node_to_client.insert(NodeId::from_raw(2), "c1".to_string());
        let mut clients = HashMap::new();
        clients.insert("c1".to_string(), client(true, 50, true));
        let (groups, node_to_group, by_index) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        let command = ClusterCommand::LevelControl(LevelControlCommand::MoveToLevel {
            level: mapper::percent_to_level(42),
            transition_time: None,
        });
        let (method, params) = mapper::command_to_rpc(NodeId::from_raw(2), &command, &ctx).unwrap();
        assert_eq!(method, "Client.SetVolume");
        assert_eq!(params["id"], "c1");
        assert_eq!(params["volume"]["percent"], 42);
        // Level and mute are separate axes; setting one leaves the other.
        assert_eq!(params["volume"]["muted"], true);
    }

    #[test]
    fn media_input_select_maps_to_group_set_stream() {
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let mut by_index = HashMap::new();
        by_index.insert(1u8, "airplay".to_string());
        let (groups, clients, node_to_client) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        let command = ClusterCommand::MediaInput(MediaInputCommand::SelectInput { index: 1 });
        let (method, params) = mapper::command_to_rpc(NodeId::from_raw(1), &command, &ctx).unwrap();
        assert_eq!(method, "Group.SetStream");
        assert_eq!(params["id"], "g1");
        assert_eq!(params["stream_id"], "airplay");
    }

    #[test]
    fn media_playback_controls_the_groups_current_stream() {
        // Not stream index 0: a group playing AirPlay must not have transport
        // commands land on whichever stream happens to be listed first.
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let mut groups = HashMap::new();
        groups.insert("g1".to_string(), group("airplay"));
        let mut by_index = HashMap::new();
        by_index.insert(0u8, "spotify".to_string());
        by_index.insert(1u8, "airplay".to_string());
        let (clients, node_to_client) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        let command = ClusterCommand::MediaPlayback(MediaPlaybackCommand::Next);
        let (method, params) = mapper::command_to_rpc(NodeId::from_raw(1), &command, &ctx).unwrap();
        assert_eq!(method, "Stream.Control");
        assert_eq!(params["id"], "airplay");
        assert_eq!(params["command"], "next");
    }

    #[test]
    fn commands_for_an_unknown_node_have_no_mapping() {
        let (groups, clients, node_to_group, node_to_client, by_index) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );
        let command = ClusterCommand::OnOff(OnOffCommand::On);
        assert!(mapper::command_to_rpc(NodeId::from_raw(99), &command, &ctx).is_none());
    }

    #[test]
    fn scanning_commands_snapserver_rejects_have_no_mapping() {
        // Snapserver answers fastForward and rewind with "Command not
        // supported", so offering them would only produce a request that is
        // certain to fail.
        let mut node_to_group = HashMap::new();
        node_to_group.insert(NodeId::from_raw(1), "g1".to_string());
        let mut groups = HashMap::new();
        groups.insert("g1".to_string(), group("spotify"));
        let (clients, node_to_client, by_index) = Default::default();
        let ctx = context(
            &groups,
            &clients,
            &node_to_group,
            &node_to_client,
            &by_index,
        );

        for cmd in [
            MediaPlaybackCommand::FastForward,
            MediaPlaybackCommand::Rewind,
        ] {
            let command = ClusterCommand::MediaPlayback(cmd);
            assert!(mapper::command_to_rpc(NodeId::from_raw(1), &command, &ctx).is_none());
        }
    }

    #[test]
    fn clients_sharing_a_host_name_get_distinct_entity_ids() {
        // Host names are not chosen to be unique and "localhost" is ordinary.
        // Two such clients resolving to one entity id would leave the engine
        // with a single addressable speaker.
        let mut first = client(false, 50, true);
        first.config.name = String::new();
        first.host.name = "localhost".to_string();
        first.id = "aaaaaaaa-1111".to_string();

        let mut second = first.clone();
        second.id = "bbbbbbbb-2222".to_string();

        assert_ne!(
            mapper::client_entity_id(&first),
            mapper::client_entity_id(&second)
        );
    }

    #[test]
    fn instances_of_one_device_get_distinct_entity_ids() {
        // Snapcast appends #N per instance, so only the whole id is distinct.
        let mut first = client(false, 50, true);
        first.config.name = String::new();
        first.host.name = String::new();
        first.id = "B1659FDD-E1E7-4377-AED8-DA19C4601B81#0".to_string();

        let mut second = first.clone();
        second.id = "B1659FDD-E1E7-4377-AED8-DA19C4601B81#1".to_string();

        assert_ne!(
            mapper::client_entity_id(&first),
            mapper::client_entity_id(&second)
        );
    }

    #[test]
    fn a_name_of_only_punctuation_does_not_yield_a_bare_domain() {
        let mut c = client(false, 50, true);
        c.config.name = "!!".to_string();
        c.id = "abc-123".to_string();
        assert_eq!(mapper::client_entity_id(&c), "speaker.snapcast_abc_123");
    }

    #[test]
    fn track_duration_is_reported_in_milliseconds() {
        let mut streams = HashMap::new();
        let mut s = stream("spotify", "librespot");
        s.properties.metadata = Some(crate::integrations::snapcast::models::StreamMetadata {
            duration: Some(212.5),
            ..Default::default()
        });
        streams.insert("spotify".to_string(), s);
        let mut indices = HashMap::new();
        indices.insert("spotify".to_string(), 0);

        let node = mapper::group_node(&group("spotify"), &streams, &indices, "media_player.k");
        let endpoint = node.endpoints.get(&mapper::SNAPCAST_ENDPOINT).unwrap();
        let playback = match endpoint.clusters.get("MediaPlayback") {
            Some(Cluster::MediaPlayback(c)) => c,
            _ => panic!("missing MediaPlayback cluster"),
        };
        assert_eq!(playback.duration, Some(212_500));
    }

    #[test]
    fn entity_ids_follow_the_domain_convention() {
        let mut g = group("spotify");
        g.name = "Kitchen Speakers".to_string();
        assert_eq!(mapper::group_entity_id(&g), "media_player.kitchen_speakers");

        let c = client(false, 50, true);
        assert_eq!(mapper::client_entity_id(&c), "speaker.kitchen_speaker");
    }

    #[test]
    fn unnamed_devices_fall_back_to_a_stable_id() {
        // Never derived from the stream: a group's entity id must not change
        // when it is retargeted at a different source.
        let mut g = group("spotify");
        g.name = String::new();
        g.id = "c29b1bd6-edff-8e0c-4d4b-dea5cd72a6aa".to_string();
        let first = mapper::group_entity_id(&g);
        g.stream_id = "airplay".to_string();
        assert_eq!(mapper::group_entity_id(&g), first);
        assert_eq!(
            first,
            "media_player.snapcast_c29b1bd6_edff_8e0c_4d4b_dea5cd72a6aa"
        );
    }
}
