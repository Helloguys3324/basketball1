use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModConfig {
    pub mod_log_channel_id: Option<u64>,
}

pub struct ConfigStore {
    path: PathBuf,
    config: RwLock<ModConfig>,
}

impl ConfigStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let path_buf = path.as_ref().to_path_buf();
        let loaded = if path_buf.exists() {
            fs::read_to_string(&path_buf)
                .ok()
                .and_then(|data| serde_json::from_str::<ModConfig>(&data).ok())
                .unwrap_or_default()
        } else {
            ModConfig::default()
        };

        Self {
            path: path_buf,
            config: RwLock::new(loaded),
        }
    }

    pub fn get_mod_channel(&self) -> Option<u64> {
        let conf = self.config.read().unwrap();
        conf.mod_log_channel_id
    }

    pub fn set_mod_channel(&self, channel_id: u64) -> Result<(), std::io::Error> {
        let mut conf = self.config.write().unwrap();
        conf.mod_log_channel_id = Some(channel_id);
        let data = serde_json::to_string_pretty(&*conf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(&self.path, data)?;
        Ok(())
    }
}
