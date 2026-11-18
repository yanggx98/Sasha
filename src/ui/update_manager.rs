//! Application-level update runtime: owns the current update state, runs the
//! blocking network work from `crate::update` on the background executor and
//! notifies the windows that observe it.

use gpui_kit::*;

use crate::model::ConfigStore;
use crate::update::{self, ReleaseInfo, UpdateError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading(ReleaseInfo),
    /// The new executable is installed; a restart switches to it.
    Installed(ReleaseInfo),
    /// A failure, with the release to fall back to when one is known.
    Failed(String, Option<ReleaseInfo>),
}

/// Global handle so any window can reach the shared manager.
#[derive(Clone)]
pub struct UpdateHandle(pub Entity<UpdateManager>);

impl Global for UpdateHandle {}

impl UpdateHandle {
    pub fn get(cx: &App) -> Entity<UpdateManager> {
        cx.global::<UpdateHandle>().0.clone()
    }
}

pub struct UpdateManager {
    state: UpdateState,
}

impl UpdateManager {
    /// Creates the manager, registers the [`UpdateHandle`] global and, when the
    /// preference allows, checks for updates once in the background.
    pub fn init(config_store: &Entity<ConfigStore>, cx: &mut App) {
        let auto_check = config_store.read(cx).config().auto_check_updates;
        let entity = cx.new(|_| Self {
            state: UpdateState::Idle,
        });
        cx.set_global(UpdateHandle(entity.clone()));
        if auto_check {
            entity.update(cx, |this, cx| this.check(cx));
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    /// True when a newer release is known (even while downloading or installed).
    pub fn has_update(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Available(_) | UpdateState::Downloading(_) | UpdateState::Installed(_)
        )
    }

    fn set(&mut self, state: UpdateState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.state,
            UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Installed(_)
        ) {
            return;
        }
        self.set(UpdateState::Checking, cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { update::fetch_latest() })
                .await;
            let _ = this.update(cx, |this, cx| {
                let state = match result {
                    Ok(release)
                        if update::is_newer(&release.version, &update::current_version()) =>
                    {
                        UpdateState::Available(release)
                    }
                    Ok(_) | Err(UpdateError::NoRelease) => UpdateState::UpToDate,
                    Err(err) => UpdateState::Failed(err.to_string(), None),
                };
                this.set(state, cx);
            });
        })
        .detach();
    }

    /// Downloads, verifies and installs the available release.
    pub fn download_and_install(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = self.state.clone() else {
            return;
        };
        let Some(asset) = release.asset.clone() else {
            return;
        };
        self.set(UpdateState::Downloading(release.clone()), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = update::download(&asset)?;
                    update::install(&path).inspect_err(|_| {
                        let _ = std::fs::remove_file(&path);
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let state = match result {
                    Ok(()) => UpdateState::Installed(release),
                    Err(err) => UpdateState::Failed(err.to_string(), Some(release)),
                };
                this.set(state, cx);
            });
        })
        .detach();
    }

    /// Launches the installed executable and quits this instance.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, UpdateState::Installed(_)) {
            return;
        }
        match update::restart() {
            Ok(()) => cx.quit(),
            Err(err) => {
                let release = match &self.state {
                    UpdateState::Installed(release) => Some(release.clone()),
                    _ => None,
                };
                self.set(UpdateState::Failed(err.to_string(), release), cx);
            }
        }
    }
}
