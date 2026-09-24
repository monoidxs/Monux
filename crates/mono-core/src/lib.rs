pub mod capabilities;

pub type Result<T> = std::result::Result<T, String>;

pub struct SystemStatus {
    pub platform: String,
    pub os: String,
    pub architecture: String,
    pub kernel: String,
    pub init: String,
    pub package_manager: String,
    pub user: String,
}

pub struct NetworkStatus {
    pub addresses: String,
    pub routes: String,
}

pub struct NetworkCheck {
    pub addresses: Vec<std::net::SocketAddr>,
    pub connected: Option<std::net::SocketAddr>,
    pub failures: Vec<String>,
}

pub trait Platform {
    fn network_check(&self, host: &str, port: u16) -> Result<NetworkCheck>;

    fn status(&self) -> Result<SystemStatus>;
    fn install(&self, package: &capabilities::PackageName) -> Result<()>;
    fn network(&self) -> Result<NetworkStatus>;
}
