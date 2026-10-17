//! Pure capture request contract. No provider implementation launches a game here.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    HlaeExisting,
    RustCandidate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Camera {
    PlayerPov {
        steam_id: String,
    },
    Projectile {
        event_key: String,
    },
    ThirdPerson {
        x: f64,
        y: f64,
        z: f64,
        yaw: f64,
        pitch: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureRequest {
    pub source_sha256: String,
    pub tick_rate: u32,
    pub start_tick: u64,
    pub throw_tick: u64,
    pub end_tick: u64,
    pub camera: Camera,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    InvalidSourceHash,
    InvalidTickRate,
    InvalidTickRange,
    InsufficientPreRoll,
    MissingCameraIdentity,
    InvalidCameraPosition,
}

impl CaptureRequest {
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.source_sha256.len() != 64
            || !self
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(PlanError::InvalidSourceHash);
        }
        if self.tick_rate == 0 {
            return Err(PlanError::InvalidTickRate);
        }
        if self.start_tick >= self.end_tick
            || self.throw_tick < self.start_tick
            || self.throw_tick > self.end_tick
        {
            return Err(PlanError::InvalidTickRange);
        }
        match &self.camera {
            Camera::PlayerPov { steam_id } if steam_id.trim().is_empty() => {
                Err(PlanError::MissingCameraIdentity)
            }
            Camera::Projectile { event_key } if event_key.trim().is_empty() => {
                Err(PlanError::MissingCameraIdentity)
            }
            Camera::ThirdPerson {
                x,
                y,
                z,
                yaw,
                pitch,
            } if ![x, y, z, yaw, pitch].iter().all(|value| value.is_finite())
                || !(-89.0..=89.0).contains(pitch) =>
            {
                Err(PlanError::InvalidCameraPosition)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub player_pov: bool,
    pub projectile_camera: bool,
    pub third_person_camera: bool,
    pub audio: bool,
    pub offline_tick_seek: bool,
}

/// A provider must validate the source-bound request before any later runtime
/// implementation attempts launch, render, pause, cancel, or cleanup.
pub trait CaptureProvider {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    fn validate_plan(&self, request: &CaptureRequest) -> Result<(), PlanError> {
        request.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureAction {
    SeekTo { target_tick: u64 },
    SetCamera(Camera),
    StartRecording,
    StopRecording,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TickAction {
    pub at_tick: u64,
    pub action: CaptureAction,
}

/// Compiles a semantic offline capture schedule. It performs no game commands
/// or file operations. A runtime provider must prove these actions map to the
/// same actual demo ticks before claiming capture parity.
pub fn compile_tick_plan(
    request: &CaptureRequest,
    first_reliable_tick: u64,
) -> Result<[TickAction; 4], PlanError> {
    request.validate()?;
    let lead_ticks = u64::from(request.tick_rate)
        .checked_mul(2)
        .ok_or(PlanError::InvalidTickRange)?;
    let seek_target = request
        .start_tick
        .checked_sub(lead_ticks)
        .ok_or(PlanError::InsufficientPreRoll)?;
    let spectate_tick = seek_target
        .checked_add(u64::from(request.tick_rate))
        .ok_or(PlanError::InvalidTickRange)?;
    if seek_target <= first_reliable_tick || spectate_tick >= request.start_tick {
        return Err(PlanError::InsufficientPreRoll);
    }
    Ok([
        TickAction {
            at_tick: first_reliable_tick,
            action: CaptureAction::SeekTo {
                target_tick: seek_target,
            },
        },
        TickAction {
            at_tick: spectate_tick,
            action: CaptureAction::SetCamera(request.camera.clone()),
        },
        TickAction {
            at_tick: request.start_tick,
            action: CaptureAction::StartRecording,
        },
        TickAction {
            at_tick: request.end_tick,
            action: CaptureAction::StopRecording,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CaptureRequest {
        CaptureRequest {
            source_sha256: "a".repeat(64),
            tick_rate: 64,
            start_tick: 100,
            throw_tick: 120,
            end_tick: 160,
            camera: Camera::Projectile {
                event_key: "demo-hash:round-3:throw-120".into(),
            },
        }
    }

    #[test]
    fn valid_source_bound_tick_plan() {
        assert_eq!(request().validate(), Ok(()));
    }

    #[test]
    fn rejects_invalid_source_tick_or_camera() {
        let mut plan = request();
        plan.source_sha256 = "unknown".into();
        assert_eq!(plan.validate(), Err(PlanError::InvalidSourceHash));
        plan = request();
        plan.end_tick = 110;
        assert_eq!(plan.validate(), Err(PlanError::InvalidTickRange));
        plan = request();
        plan.camera = Camera::ThirdPerson {
            x: f64::NAN,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        };
        assert_eq!(plan.validate(), Err(PlanError::InvalidCameraPosition));
    }

    #[test]
    fn compiles_deterministic_tick_actions() {
        let mut clip = request();
        clip.start_tick = 6400;
        clip.throw_tick = 6500;
        clip.end_tick = 7040;
        let plan = compile_tick_plan(&clip, 96).unwrap();
        assert_eq!(
            plan.iter().map(|step| step.at_tick).collect::<Vec<_>>(),
            vec![96, 6336, 6400, 7040]
        );
        assert_eq!(plan[0].action, CaptureAction::SeekTo { target_tick: 6272 });
        assert_eq!(plan[2].action, CaptureAction::StartRecording);
        assert_eq!(plan[3].action, CaptureAction::StopRecording);
    }

    #[test]
    fn early_capture_refuses_to_shift_recording_bounds_silently() {
        let mut early = request();
        early.start_tick = 40;
        early.throw_tick = 60;
        early.end_tick = 120;
        assert_eq!(
            compile_tick_plan(&early, 96),
            Err(PlanError::InsufficientPreRoll)
        );
    }
}
