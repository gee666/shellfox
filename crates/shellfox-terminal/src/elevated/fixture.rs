//! Test-support only. Fixed fake argv is rejected by the real managed helper.
use super::{Generation, PreparedLaunch, Session, StartError, VerifiedHelper};
use std::process::Command;

#[derive(Clone, Copy)]
pub enum FixtureScenario {
    Normal,
    Cancelled,
    LauncherFirst,
    Unconfirmed,
}
impl FixtureScenario {
    fn argument(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Cancelled => "cancelled",
            Self::LauncherFirst => "launcher-first",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}
/// `additional_hold` lets tests assert a whole verified bundle outlives the
/// launcher. It never grants a managed loader policy or requests elevation.
pub fn fixture_session<L: Send + 'static>(
    image: VerifiedHelper,
    additional_hold: L,
    scenario: FixtureScenario,
    generation: Generation,
    launch: PreparedLaunch,
) -> Result<Session, StartError> {
    let mut command = Command::new(image.path());
    command
        .arg("--shellfox-elevated-fixture")
        .arg(scenario.argument());
    if let Some(parent) = image.path().parent() {
        command.current_dir(parent);
    }
    command.env_clear().envs(launch.launcher_environment());
    Session::start(
        command,
        Box::new((image, additional_hold)),
        generation,
        launch,
    )
}
