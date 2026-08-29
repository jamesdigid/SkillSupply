#[derive(Debugug)]
struct ServerConfig {
    port: u16,
    retries: u16,
    timeout_ms: u64
}

impl Default for ServerConfig {
    fn default -> Self {
        Self {
            port: 777,
            retries: 3,
            timeout_ms: 5000
        }
    }
}

let config = ServerConfig {
    port: 666,
    ..Default::default()
};



println("{}config:?}")