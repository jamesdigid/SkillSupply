use skillsupport::manifest::{CapabilityManifest, CommunicationSpec, RuntimeSpec, RuntimeType};
use skillsupport::resolver::{DependencyResolver, PetgraphDependencyResolver};

#[test]
fn builds_dependency_graph() {
    let manifests = vec![
        CapabilityManifest {
            name: "browser-attach-runtime".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            capabilities: vec!["browser-generic".to_string()],
            dependencies: vec![],
            install: None,
            runtime: RuntimeSpec {
                kind: RuntimeType::Available,
                initialize: None,
                ready: None,
                shutdown: None,
            },
            communication: Some(CommunicationSpec {
                transport: skillsupport::manifest::TransportType::Stdio,
            }),
            prompt: None,
            tests: None,
            keywords: vec![],
        },
        CapabilityManifest {
            name: "gmail-send-runtime".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            capabilities: vec!["gmail".to_string()],
            dependencies: vec!["browser-generic".to_string()],
            install: None,
            runtime: RuntimeSpec {
                kind: RuntimeType::Available,
                initialize: None,
                ready: None,
                shutdown: None,
            },
            communication: Some(CommunicationSpec {
                transport: skillsupport::manifest::TransportType::Stdio,
            }),
            prompt: None,
            tests: None,
            keywords: vec![],
        },
    ];

    let graph = PetgraphDependencyResolver
        .build(&manifests)
        .expect("graph should build");

    assert_eq!(
        graph.nodes(),
        vec![
            "browser-attach-runtime".to_string(),
            "gmail-send-runtime".to_string()
        ]
    );
    assert_eq!(graph.unresolved_requirements(), Vec::<String>::new());
    assert_eq!(graph.topological_order().expect("toposort"), graph.nodes());
    assert_eq!(graph.edges().len(), 1);
    assert_eq!(graph.edges()[0].contract, "browser-generic");
}
