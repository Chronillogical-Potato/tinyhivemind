//! Management request serialization remains explicit and round trips.
#[test]
fn management_request_wire_shape_keeps_host_template_and_identity_fields() {
    use super::ManagementRequest;
    use serde_json::json;
    let cases = [
        (
            ManagementRequest::CreateAgent {
                template: "research".into(),
                config: json!({"memory_ref":"private-1"}),
            },
            json!({"CreateAgent":{"template":"research","config":{"memory_ref":"private-1"}}}),
        ),
        (
            ManagementRequest::JoinHive {
                hive_id: "h".into(),
                agent_id: "a".into(),
            },
            json!({"JoinHive":{"hive_id":"h","agent_id":"a"}}),
        ),
        (
            ManagementRequest::LeaveHive {
                hive_id: "h".into(),
                agent_id: "a".into(),
            },
            json!({"LeaveHive":{"hive_id":"h","agent_id":"a"}}),
        ),
        (
            ManagementRequest::CreateHive(tinyhivemind_hives::HiveInfo {
                hive_id: "h".into(),
                name: "Hive".into(),
                description: None,
                members: vec![],
            }),
            json!({"CreateHive":{"hive_id":"h","name":"Hive","description":null,"members":[]}}),
        ),
    ];
    for (request, wire) in cases {
        assert_eq!(serde_json::to_value(&request).ok(), Some(wire.clone()));
        assert_eq!(
            serde_json::from_value::<ManagementRequest>(wire).ok(),
            Some(request)
        );
    }
}
