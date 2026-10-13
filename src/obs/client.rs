use super::transport::Session;
use super::{ObsAction, ObsConfig, ObsStatus};
use serde::{de::IgnoredAny, Deserialize};
use serde_json::json;
use std::io;

/// A blocking OBS client. New connections are made for each operation.
#[derive(Debug)]
pub struct Client {
    config: ObsConfig,
}

impl Client {
    /// Stores settings without connecting. Invalid settings fail on operation.
    pub fn new(config: ObsConfig) -> Self {
        Self { config }
    }

    /// Executes exactly one validated action, awaiting server acknowledgement.
    ///
    /// # Errors
    /// Returns validation, connection, authentication, protocol, or request errors.
    pub fn execute(&mut self, action: &ObsAction) -> io::Result<()> {
        action.validate()?;
        let mut session = Session::connect(&self.config)?;
        match action {
            ObsAction::Scene(scene) => {
                session
                    .request::<IgnoredAny>("SetCurrentProgramScene", json!({"sceneName":scene}))?;
            }
            ObsAction::ToggleRecording => {
                session.request::<IgnoredAny>("ToggleRecord", json!({}))?;
            }
            ObsAction::ToggleStreaming => {
                session.request::<IgnoredAny>("ToggleStream", json!({}))?;
            }
            ObsAction::StartRecording => {
                session.request::<IgnoredAny>("StartRecord", json!({}))?;
            }
            ObsAction::StopRecording => {
                session.request::<IgnoredAny>("StopRecord", json!({}))?;
            }
            ObsAction::PauseRecording => {
                session.request::<IgnoredAny>("PauseRecord", json!({}))?;
            }
            ObsAction::ResumeRecording => {
                session.request::<IgnoredAny>("ResumeRecord", json!({}))?;
            }
            ObsAction::StartStreaming => {
                session.request::<IgnoredAny>("StartStream", json!({}))?;
            }
            ObsAction::StopStreaming => {
                session.request::<IgnoredAny>("StopStream", json!({}))?;
            }
            ObsAction::ToggleMute(input) => {
                session.request::<IgnoredAny>("ToggleInputMute", json!({"inputName":input}))?;
            }
            ObsAction::SourceVisibility {
                scene,
                source,
                visible,
            } => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct SceneItem {
                    scene_item_id: i64,
                }
                let item: SceneItem = session.query(
                    "GetSceneItemId",
                    json!({"sceneName":scene,"sourceName":source}),
                )?;
                if item.scene_item_id < 0 {
                    return Err(super::transport::protocol_error(
                        "invalid OBS scene item ID",
                    ));
                }
                session.request::<IgnoredAny>("SetSceneItemEnabled",
                    json!({"sceneName":scene,"sceneItemId":item.scene_item_id,"sceneItemEnabled":visible}))?;
            }
        }
        Ok(())
    }

    /// Queries scenes, the current program scene, and recording/streaming state.
    ///
    /// # Errors
    /// Returns connection, authentication, malformed response, or request errors.
    pub fn probe(&mut self) -> io::Result<ObsStatus> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Scene {
            scene_name: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SceneList {
            current_program_scene_name: String,
            scenes: Vec<Scene>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OutputStatus {
            output_active: bool,
        }
        let mut session = Session::connect(&self.config)?;
        let scenes: SceneList = session.query("GetSceneList", json!({}))?;
        let recording: OutputStatus = session.query("GetRecordStatus", json!({}))?;
        let streaming: OutputStatus = session.query("GetStreamStatus", json!({}))?;
        Ok(ObsStatus {
            scenes: scenes
                .scenes
                .into_iter()
                .map(|scene| scene.scene_name)
                .collect(),
            current_scene: scenes.current_program_scene_name,
            recording: recording.output_active,
            streaming: streaming.output_active,
        })
    }
}
