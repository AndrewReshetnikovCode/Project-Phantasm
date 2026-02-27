use anyhow::Result;

pub struct AudioEngine {
    manager: Option<kira::manager::AudioManager>,
}

impl AudioEngine {
    pub fn new() -> Self {
        match kira::manager::AudioManager::<kira::manager::backend::DefaultBackend>::new(
            kira::manager::AudioManagerSettings::default(),
        ) {
            Ok(manager) => {
                log::info!("Audio engine initialized");
                Self {
                    manager: Some(manager),
                }
            }
            Err(e) => {
                log::warn!("Audio unavailable ({}), running silent", e);
                Self { manager: None }
            }
        }
    }

    pub fn is_available(&self) -> bool {
        self.manager.is_some()
    }

    pub fn play_sound(&mut self, path: &str) -> Result<()> {
        if let Some(manager) = &mut self.manager {
            let sound_data = kira::sound::static_sound::StaticSoundData::from_file(path)?;
            manager.play(sound_data)?;
            log::info!("Playing sound: {}", path);
        } else {
            log::debug!("Audio unavailable, skipping: {}", path);
        }
        Ok(())
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}
