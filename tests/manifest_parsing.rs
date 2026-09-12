use skillsupport::manifest::ManifestParser;
use skillsupport::manifest::YamlManifestParser;

#[test]
fn parses_manifest() {
    let parser = YamlManifestParser;
    let manifest = parser
        .parse_path(std::path::Path::new("examples/gmail-send/caps.yaml"))
        .expect("manifest should parse");

    assert_eq!(manifest.name, "gmail-send");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.capabilities, vec!["gmail"]);
}
