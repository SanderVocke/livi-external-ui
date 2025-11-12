use crate::external_ui::{ExternalUI, ExternalUIWorld, LiviExternalUIError};
use std::fmt;

/// A UI that can be used to instantiate UI instances.
pub enum UI {
    External(ExternalUI),
    Unsupported(String),
}

#[derive(Debug)]
pub enum LiviUIError {
    LiviExternalUIError(LiviExternalUIError),
}

impl fmt::Display for LiviUIError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LiviUIError::LiviExternalUIError(error) => write!(f, "External UI error: {error}")?,
        }
        Ok(())
    }
}

impl std::error::Error for LiviUIError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LiviUIError::LiviExternalUIError(error) => Some(error),
        }
    }
}

pub fn plugin_uis(world: &livi::World, plugin: &livi::Plugin) -> Result<impl Iterator<Item = UI>, LiviUIError> {
    let ui_world = ExternalUIWorld::new(world.raw());
    let uis: Option<Result<Vec<UI>, LiviUIError>> = plugin.raw().uis().map(|uis| {
        uis.into_iter()
            .map(|ui| -> Result<UI, LiviUIError> {
                if ExternalUI::is_external_ui(&ui_world, &ui) {
                    ExternalUI::from_ui(&ui_world, ui)
                        .map_err(|e| LiviUIError::LiviExternalUIError(e))
                        .map(|ui| UI::External(ui))
                } else {
                    Ok(UI::Unsupported(
                        ui.uri().as_uri().unwrap_or("unknown").to_string(),
                    ))
                }
            })
            .collect()
    });

    match uis {
        Some(Ok(uis)) => Ok(uis.into_iter()),
        Some(Err(e)) => Err(e),
        None => Ok(Vec::new().into_iter()),
    }
}
