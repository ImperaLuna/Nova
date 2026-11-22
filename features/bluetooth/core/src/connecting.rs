//! Connecting a device: pairing and trusting a new one first, falling back
//! to classic Bluetooth when LE gives up, bringing up its audio, and
//! stopping a connect that runs past its limit.

use super::*;

impl Shared {
    /// The device at `address`: its path, and whether it's paired and
    /// connected.
    pub(super) fn device(&self, address: &str) -> Result<(OwnedObjectPath, bool, bool), String> {
        if !bluez::is_address(address) {
            return Err(format!("{address:?} isn't a Bluetooth address"));
        }
        let state = lock(&self.state);
        let device = state.objects.device(address).ok_or("Out of range")?;
        Ok((device.path, device.paired, device.connected))
    }

    /// Connects over classic Bluetooth the device whose connect over
    /// Bluetooth LE failed with `error`, through its first classic audio
    /// profile: BlueZ picks LE again for a plain `Connect`.
    pub(super) fn connect_classic(
        &self,
        address: &str,
        path: &OwnedObjectPath,
        error: zbus::Error,
    ) -> zbus::Result<()> {
        let profile =
            lock(&self.state).objects.device(address).and_then(|device| device.classic_profile().map(str::to_owned));
        let Some(profile) = profile else { return Err(error) };
        self.logger.info(LOG_SOURCE, &format!("{address}: LE connect aborted, connecting over classic Bluetooth"));
        self.connect_profile(path, &profile)
    }

    pub(super) fn connect_profile(&self, path: &OwnedObjectPath, profile: &str) -> zbus::Result<()> {
        let agent = lock(&self.state).agent.clone().ok_or_else(not_followed)?;
        zbus::block_on(agent.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "ConnectProfile", &(profile,)))
            .map(|_| ())
    }

    /// A connected device that plays music has its A2DP audio up, asked
    /// for again when it didn't come: connected over LE alone, or with
    /// A2DP refused, a headset plays nothing, or only call audio.
    pub(super) fn ensure_a2dp(&self, address: &str, path: &OwnedObjectPath) {
        {
            let mut state = lock(&self.state);
            if !state.objects.device(address).is_some_and(|device| device.offers_a2dp()) {
                return;
            }
            // It answered: the limit is for reaching it.
            state.connects.remove(&address.to_uppercase());
        }
        for attempt in 0..=AUDIO_RETRIES {
            if attempt > 0 {
                // Stopped by the user, or the whole service stopped.
                if self.go_on(address).is_err() || lock(&self.state).stopped {
                    return;
                }
                self.logger.info(LOG_SOURCE, &format!("{address}: connected without A2DP, asking for it again"));
                if let Err(e) = self.connect_profile(path, bluez::A2DP_SINK) {
                    self.logger.info(LOG_SOURCE, &format!("{address}: A2DP refused: {e}"));
                }
            }
            if self.wait_for_a2dp(path) {
                return;
            }
        }
        self.logger.info(LOG_SOURCE, &format!("{address}: connected, but A2DP never came up"));
    }

    /// Whether A2DP comes up on the device at `path` within the wait.
    pub(super) fn wait_for_a2dp(&self, path: &OwnedObjectPath) -> bool {
        let deadline = Instant::now() + self.sources.audio_wait;
        let mut state = lock(&self.state);
        loop {
            if state.objects.a2dp(path.as_str()) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || state.stopped {
                return false;
            }
            state = self.wake.wait_timeout(state, left).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }

    /// Stops what's under way on the device at `path`: pairing, or
    /// connecting (BlueZ ends a connect it's asked to disconnect).
    pub(super) fn cancel(&self, path: &OwnedObjectPath, status: Option<BluetoothStatus>) {
        let method = match status {
            Some(BluetoothStatus::Pairing) => "CancelPairing",
            _ => "Disconnect",
        };
        let result = self.calls().and_then(|calls| zbus::block_on(call(&calls, path, DEVICE, method)));
        if let Err(e) = result {
            self.logger.info(LOG_SOURCE, &format!("can't stop what {path} was doing: {e}"));
        }
    }

    /// Stops the connect to `address` starting now if it still runs after
    /// the limit, on a thread of its own.
    pub(super) fn limit_connect(shared: &Arc<Self>, address: &str, path: &OwnedObjectPath) {
        let key = address.to_uppercase();
        let number = {
            let mut state = lock(&shared.state);
            state.last_connect += 1;
            let number = state.last_connect;
            state.connects.insert(key.clone(), number);
            number
        };
        let (shared, path) = (Arc::clone(shared), path.clone());
        std::thread::spawn(move || {
            let running = |state: &State| {
                state.connects.get(&key) == Some(&number) && state.busy.get(&key) == Some(&BluetoothStatus::Connecting)
            };
            // Every change to the state wakes it, so it ends with the connect.
            // Stopped under the lock: the connect can't end, and a retry
            // can't start, between the check and the stop.
            let (mut state, _) = shared
                .wake
                .wait_timeout_while(lock(&shared.state), shared.sources.connect_limit, |state| running(state))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(calls) = state.calls.clone().filter(|_| running(&state)) else { return };
            state.timed_out.insert(key.clone());
            shared.logger.info(LOG_SOURCE, &format!("{key}: connect took too long, stopping it"));
            if let Err(e) = zbus::block_on(call(&calls, &path, DEVICE, "Disconnect")) {
                shared.logger.info(LOG_SOURCE, &format!("can't stop connecting {path}: {e}"));
            }
            drop(state);
        });
    }

    /// Whether a command on `address` may take its next step: not once the
    /// user stopped it.
    pub(super) fn go_on(&self, address: &str) -> zbus::Result<()> {
        match lock(&self.state).halted.contains(&address.to_uppercase()) {
            true => Err(zbus::Error::Failure("stopped by the user".into())),
            false => Ok(()),
        }
    }

    pub(super) fn set_busy(&self, address: &str, status: BluetoothStatus) {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.errors.remove(&key);
        state.busy.insert(key, status);
        self.publish(&mut state);
    }

    /// A command on `address` ended with `result`: its failure is kept on
    /// the device, and a code shown for it goes.
    pub(super) fn done(&self, address: &str, result: zbus::Result<()>) -> Result<(), String> {
        let mut state = lock(&self.state);
        let key = address.to_uppercase();
        state.busy.remove(&key);
        state.connects.remove(&key);
        // Stopped: however it ended, there's nothing to explain, unless
        // the limit stopped it.
        let result = if state.timed_out.remove(&key) {
            Err(zbus::Error::FDO(Box::new(zbus::fdo::Error::NoReply("connect took too long".into()))))
        } else if state.halted.remove(&key) {
            Ok(())
        } else {
            result
        };
        let shown = state.pairing.as_ref().is_some_and(|asked| asked.slot.is_none() && asked.request.address == key);
        if shown {
            state.pairing = None;
        }
        let outcome = match result {
            Ok(()) => Ok(()),
            Err(e) => {
                self.logger.info(LOG_SOURCE, &format!("{address}: {e}"));
                match bluez::failure(&e) {
                    Some(words) => {
                        state.errors.insert(key, words.clone());
                        Err(words)
                    }
                    // The user cancelled: nothing to say.
                    None => Ok(()),
                }
            }
        };
        self.publish(&mut state);
        outcome
    }
}
